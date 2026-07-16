use binbook_diagnostic_protocol::{
    encode_store_bulk_abort_request, encode_store_bulk_credit, encode_store_bulk_finish_response,
    encode_store_bulk_open_response, Opcode, Status, StoreUploadCommitResult, TransferDirection,
    TransferTraceState,
};
use binbook_fw::{
    bulk_transfer_coordinator::BulkOwnerEvent,
    diag::{queue_runtime_event, queue_runtime_response, SerialState},
};

use super::{
    diagnostic_bulk::DiagnosticBulkRuntime, diagnostic_owner_storage::request_library_refresh,
};

const BULK_CONTROL_PAYLOAD_BYTES: usize = 64;

impl DiagnosticBulkRuntime {
    pub(super) async fn handle_owner_event(
        &mut self,
        serial: &mut SerialState,
        sequence: u32,
        event: BulkOwnerEvent,
        now_ms: u64,
        transfer_trace: &mut binbook_fw::transfer_trace_command::TransferTraceCommand,
    ) {
        let mut payload = [0; BULK_CONTROL_PAYLOAD_BYTES];
        match event {
            BulkOwnerEvent::Opened(opened) => {
                self.arm_idle_timeout(now_ms);
                transfer_trace.set_negotiated_shape(opened.initial_credits, opened.selected_chunk);
                transfer_trace.enter_phase(
                    binbook_fw::transfer_metrics::TransferPhase::DataPhase,
                    embassy_time::Instant::now().as_micros(),
                );
                if let Ok(len) = encode_store_bulk_open_response(&opened, &mut payload) {
                    self.complete_reply(serial, sequence, &payload[..len]);
                } else {
                    self.fail_reply_with_status(serial, sequence, Status::InternalError);
                }
                if opened.direction == TransferDirection::Readback {
                    self.fill_read_window().await;
                }
            }
            BulkOwnerEvent::UploadCredit(Some(credit)) => {
                self.touch_idle_timeout(now_ms);
                if let Ok(len) = encode_store_bulk_credit(&credit, &mut payload) {
                    queue_runtime_event(serial, Opcode::StoreBulkCredit, &payload[..len]);
                }
            }
            BulkOwnerEvent::UploadCredit(None) => {}
            BulkOwnerEvent::UploadFinished {
                response,
                published,
            } => {
                self.disarm_idle_timeout();
                transfer_trace.exit_phase(
                    binbook_fw::transfer_metrics::TransferPhase::DataPhase,
                    embassy_time::Instant::now().as_micros(),
                );
                let trace_id = transfer_trace.active_trace_id();
                let succeeded = response.result == StoreUploadCommitResult::Ok;
                let _ = transfer_trace.finish(
                    if succeeded {
                        TransferTraceState::Succeeded
                    } else {
                        TransferTraceState::Failed
                    },
                    serial.protocol_error_count(),
                    embassy_time::Instant::now().as_micros(),
                    published && trace_id.is_some(),
                );
                if let Ok(len) = encode_store_bulk_finish_response(&response, &mut payload) {
                    self.complete_reply(serial, sequence, &payload[..len]);
                } else {
                    self.fail_reply_with_status(serial, sequence, Status::InternalError);
                }
                if published {
                    request_library_refresh(trace_id).await;
                }
            }
            BulkOwnerEvent::UploadAborted => {
                self.disarm_idle_timeout();
                transfer_trace.exit_phase(
                    binbook_fw::transfer_metrics::TransferPhase::DataPhase,
                    embassy_time::Instant::now().as_micros(),
                );
                let _ = transfer_trace.finish(
                    TransferTraceState::Aborted,
                    serial.protocol_error_count(),
                    embassy_time::Instant::now().as_micros(),
                    false,
                );
                let Some(reply) = self.take_reply(sequence) else {
                    return;
                };
                if let Some(stream_id) = reply.stream_id {
                    if let Ok(len) = encode_store_bulk_abort_request(stream_id, &mut payload) {
                        queue_runtime_response(serial, reply.header, Status::Ok, &payload[..len]);
                    } else {
                        queue_runtime_response(serial, reply.header, Status::InternalError, &[]);
                    }
                }
            }
        }
    }
}
