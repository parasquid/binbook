use super::{diagnostic_bulk_types::DeferredControl, STORAGE_REQUEST_CHANNEL};
use binbook_diagnostic_protocol::{
    encode_store_bulk_data, encode_store_bulk_status_response, Opcode, Status, MAX_PAYLOAD_BYTES,
};
use binbook_fw::{
    bulk_idle_timeout::BulkIdleTimeout,
    bulk_reply_tracker::{BulkReplyTracker, PendingBulkReply},
    bulk_slots::{TRANSFER_DATA_BYTES, TRANSFER_SLOT_COUNT},
    bulk_transfer_coordinator::BulkTransferCoordinator,
    diag::{queue_runtime_event, queue_runtime_response, SerialState},
    storage_owner::OwnedResponse,
};

const BULK_CONTROL_PAYLOAD_BYTES: usize = 64;

pub(super) struct DiagnosticBulkRuntime {
    pub(super) coordinator: BulkTransferCoordinator,
    pub(super) idle_timeout: BulkIdleTimeout,
    pub(super) replies: BulkReplyTracker<4>,
    pub(super) deferred: Option<DeferredControl>,
    pool_was_full: bool,
}

impl DiagnosticBulkRuntime {
    pub(super) fn new(
        buffers: [&'static mut [u8; TRANSFER_DATA_BYTES]; TRANSFER_SLOT_COUNT],
    ) -> Self {
        Self {
            coordinator: BulkTransferCoordinator::new(buffers),
            idle_timeout: BulkIdleTimeout::new(),
            replies: BulkReplyTracker::new(),
            deferred: None,
            pool_was_full: false,
        }
    }

    pub(super) fn coordinator(&mut self) -> &mut BulkTransferCoordinator {
        &mut self.coordinator
    }

    pub(super) fn pending_owner(&self) -> usize {
        self.coordinator.pending_owner()
    }

    pub(super) fn mark_commit_timing(
        &self,
        sequence: u32,
        transfer_trace: &mut binbook_fw::transfer_trace_command::TransferTraceCommand,
    ) {
        if self.replies.is_finish(sequence) {
            transfer_trace.mark_commit_sequence(sequence);
        }
    }

    pub(super) async fn handle_owner(
        &mut self,
        serial: &mut SerialState,
        response: OwnedResponse<'static>,
        now_ms: u64,
        transfer_trace: &mut binbook_fw::transfer_trace_command::TransferTraceCommand,
    ) {
        self.touch_idle_timeout(now_ms);
        let sequence = response.envelope.sequence;
        let upload_crc = self.coordinator.pending_owner_is_upload_write(sequence);
        if upload_crc {
            transfer_trace.enter_phase(
                binbook_fw::transfer_metrics::TransferPhase::Crc,
                embassy_time::Instant::now().as_micros(),
            );
        }
        if self.coordinator.pending_owner_is_read(sequence) {
            match self.coordinator.complete_read_owner(response) {
                Ok(Some(frame)) => {
                    let mut payload = [0; MAX_PAYLOAD_BYTES];
                    if let Ok(len) = encode_store_bulk_data(&frame, &mut payload) {
                        queue_runtime_event(serial, Opcode::StoreBulkData, &payload[..len]);
                    }
                }
                Ok(None) => self.coordinator.reap_quiescent(),
                Err(_) => self.fail_reply(serial, sequence),
            }
        } else {
            match self.coordinator.complete_owner(response) {
                Ok(event) => {
                    self.handle_owner_event(serial, sequence, event, now_ms, transfer_trace)
                        .await
                }
                Err(_) => self.fail_reply(serial, sequence),
            }
        }
        if upload_crc {
            transfer_trace.exit_phase(
                binbook_fw::transfer_metrics::TransferPhase::Crc,
                embassy_time::Instant::now().as_micros(),
            );
        }
        self.service_deferred(serial).await;
        self.record_pressure(transfer_trace);
    }

    pub(super) fn record_pressure(
        &mut self,
        transfer_trace: &mut binbook_fw::transfer_trace_command::TransferTraceCommand,
    ) {
        let occupied = TRANSFER_SLOT_COUNT
            .saturating_sub(usize::from(self.coordinator.available_leases()))
            as u8;
        if let Some(metrics) = transfer_trace.metrics_mut() {
            metrics.record_slots(occupied);
            let full = usize::from(occupied) == TRANSFER_SLOT_COUNT;
            if full && !self.pool_was_full {
                metrics.record_queue_stall();
            }
            self.pool_was_full = full;
        } else {
            self.pool_was_full = false;
        }
    }

    pub(super) async fn fill_read_window(&mut self) {
        while self.coordinator.pending_owner() < TRANSFER_SLOT_COUNT {
            let Ok(request) = self.coordinator.begin_read() else {
                break;
            };
            STORAGE_REQUEST_CHANNEL.send(request).await;
        }
    }

    async fn service_deferred(&mut self, serial: &mut SerialState) {
        if self.coordinator.pending_owner() != 0 {
            return;
        }
        let Some(deferred) = self.deferred.take() else {
            return;
        };
        let mut payload = [0; BULK_CONTROL_PAYLOAD_BYTES];
        match deferred {
            DeferredControl::Status { header, stream_id } => {
                match self.coordinator.status(stream_id) {
                    Ok(status) => match encode_store_bulk_status_response(&status, &mut payload) {
                        Ok(len) => {
                            queue_runtime_response(serial, header, Status::Ok, &payload[..len])
                        }
                        Err(_) => {
                            queue_runtime_response(serial, header, Status::InternalError, &[])
                        }
                    },
                    Err(_) => queue_runtime_response(serial, header, Status::BadRequest, &[]),
                }
            }
            DeferredControl::Finish { header, stream_id } => {
                if self.replies.is_full() {
                    queue_runtime_response(serial, header, Status::InternalError, &[]);
                    return;
                }
                match self.coordinator.begin_upload_finish(stream_id) {
                    Ok(request) => {
                        if self
                            .replies
                            .insert(PendingBulkReply {
                                owner_sequence: request.envelope.sequence,
                                header,
                                stream_id: None,
                            })
                            .is_err()
                        {
                            queue_runtime_response(serial, header, Status::InternalError, &[]);
                        }
                        STORAGE_REQUEST_CHANNEL.send(request).await;
                    }
                    Err(_) => queue_runtime_response(serial, header, Status::BadRequest, &[]),
                }
            }
        }
    }

    pub(super) fn defer(&mut self, serial: &mut SerialState, control: DeferredControl) {
        if self.deferred.is_some() {
            let header = match control {
                DeferredControl::Status { header, .. } | DeferredControl::Finish { header, .. } => {
                    header
                }
            };
            queue_runtime_response(serial, header, Status::BadRequest, &[]);
        } else {
            self.deferred = Some(control);
        }
    }

    pub(super) fn complete_reply(
        &mut self,
        serial: &mut SerialState,
        sequence: u32,
        payload: &[u8],
    ) {
        if let Some(reply) = self.take_reply(sequence) {
            queue_runtime_response(serial, reply.header, Status::Ok, payload);
        }
    }

    fn fail_reply(&mut self, serial: &mut SerialState, sequence: u32) {
        self.fail_reply_with_status(serial, sequence, Status::Error);
    }

    pub(super) fn fail_reply_with_status(
        &mut self,
        serial: &mut SerialState,
        sequence: u32,
        status: Status,
    ) {
        if let Some(reply) = self.take_reply(sequence) {
            queue_runtime_response(serial, reply.header, status, &[]);
        }
    }

    pub(super) fn take_reply(&mut self, sequence: u32) -> Option<PendingBulkReply> {
        self.replies.take(sequence)
    }
}
