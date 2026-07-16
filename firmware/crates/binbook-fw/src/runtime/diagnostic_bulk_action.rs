use binbook_diagnostic_protocol::{FrameHeader, Opcode, Status, TransferDirection};
use binbook_fw::{
    bulk_protocol_dispatch::BulkProtocolAction,
    bulk_reply_tracker::PendingBulkReply,
    diag::{queue_runtime_response, SerialState},
};

use super::{
    diagnostic_bulk::DiagnosticBulkRuntime, diagnostic_bulk_types::DeferredControl,
    STORAGE_REQUEST_CHANNEL,
};

impl DiagnosticBulkRuntime {
    pub(super) async fn handle_action(
        &mut self,
        serial: &mut SerialState,
        header: FrameHeader,
        action: BulkProtocolAction,
        now_ms: u64,
        transfer_trace: &mut binbook_fw::transfer_trace_command::TransferTraceCommand,
    ) {
        match action {
            BulkProtocolAction::Owner {
                request,
                reply_to,
                reply_stream_id,
            } => {
                if header.opcode == Opcode::StoreBulkData {
                    self.touch_idle_timeout(now_ms);
                }
                if let Some(reply) = reply_to {
                    if self
                        .replies
                        .insert(PendingBulkReply {
                            owner_sequence: request.envelope.sequence,
                            header: reply,
                            stream_id: reply_stream_id,
                        })
                        .is_err()
                    {
                        queue_runtime_response(serial, header, Status::InternalError, &[]);
                    }
                }
                // The coordinator has already admitted this owner operation.
                // Always submit it even if control-reply bookkeeping saturates,
                // so leases and storage state can drain instead of becoming stuck.
                STORAGE_REQUEST_CHANNEL.send(request).await;
            }
            BulkProtocolAction::Immediate {
                status,
                payload,
                payload_len,
            } => {
                if status == Status::Ok {
                    if self.coordinator.is_idle() {
                        self.disarm_idle_timeout();
                    } else {
                        self.touch_idle_timeout(now_ms);
                    }
                    let terminal = match header.opcode {
                        Opcode::StoreBulkFinish => {
                            Some(binbook_diagnostic_protocol::TransferTraceState::Succeeded)
                        }
                        Opcode::StoreBulkAbort => {
                            Some(binbook_diagnostic_protocol::TransferTraceState::Aborted)
                        }
                        _ => None,
                    };
                    if let Some(state) = terminal {
                        transfer_trace.exit_phase(
                            binbook_fw::transfer_metrics::TransferPhase::DataPhase,
                            embassy_time::Instant::now().as_micros(),
                        );
                        let _ = transfer_trace.finish(
                            state,
                            serial.protocol_error_count(),
                            embassy_time::Instant::now().as_micros(),
                            false,
                        );
                    }
                }
                queue_runtime_response(serial, header, status, &payload[..payload_len]);
            }
            BulkProtocolAction::AcceptedEvent => {
                self.touch_idle_timeout(now_ms);
                if self.coordinator.direction() == TransferDirection::Readback {
                    self.fill_read_window().await;
                }
            }
            BulkProtocolAction::DeferredStatus { stream_id } => {
                self.touch_idle_timeout(now_ms);
                self.defer(serial, DeferredControl::Status { header, stream_id });
            }
            BulkProtocolAction::DeferredFinish { stream_id } => {
                self.touch_idle_timeout(now_ms);
                self.defer(serial, DeferredControl::Finish { header, stream_id });
            }
        }
        self.record_pressure(transfer_trace);
    }
}
