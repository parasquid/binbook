use binbook_diagnostic_protocol::{
    decode_frame, decode_frame_in_place_profiled, decode_key_payload, decode_raw_frame,
    decode_transfer_trace_request, encode_frame, encode_log_response_header, encode_page_response,
    encode_raw_frame, encode_refresh_mode_payload, FrameDecodeEvent, FrameHeader, FrameKind,
    LogResponseHeader, Opcode, RawFrameHeader, RefreshModeAction, RefreshModePayload, Status,
    TransferTraceRequest, MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES,
};
#[cfg(feature = "sd-storage")]
use binbook_diagnostic_protocol::{decode_sd_write_trace_request, SdWriteTraceRequest};

use crate::{diag_log::DiagLog, diag_storage::StorageHandle};

use super::{
    command::{dispatch_command, CommandContext, DispatchResult},
    resolve_log_clear, resolve_log_get, DiagnosticSnapshot, PendingAction, PendingCommand,
    SerialState, TransportError,
};

#[cfg(feature = "diagnostic-console")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeProfileEvent {
    Frame(FrameDecodeEvent),
    #[cfg(feature = "sd-storage")]
    Bulk(crate::bulk_protocol_dispatch::BulkDispatchEvent),
}

pub enum RuntimeCommand {
    Hardware(PendingCommand),
    LogGet {
        header: FrameHeader,
        cursor: u32,
        max_bytes: u16,
    },
    LogClear {
        header: FrameHeader,
    },
    Immediate {
        header: FrameHeader,
    },
    TransferTrace {
        header: FrameHeader,
        request: TransferTraceRequest,
    },
    #[cfg(feature = "sd-storage")]
    SdWriteTrace {
        header: FrameHeader,
        request: SdWriteTraceRequest,
    },
    #[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
    OwnerStorage {
        header: FrameHeader,
        command: crate::diag_owner_storage::OwnerStorageCommand,
    },
    #[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
    Bulk {
        header: FrameHeader,
        action: crate::bulk_protocol_dispatch::BulkProtocolAction,
    },
}

impl core::fmt::Debug for RuntimeCommand {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("RuntimeCommand")
            .field("header", &self.header())
            .finish_non_exhaustive()
    }
}

impl PartialEq for RuntimeCommand {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Hardware(left), Self::Hardware(right)) => left == right,
            (
                Self::LogGet {
                    header: left_header,
                    cursor: left_cursor,
                    max_bytes: left_max,
                },
                Self::LogGet {
                    header: right_header,
                    cursor: right_cursor,
                    max_bytes: right_max,
                },
            ) => {
                left_header == right_header && left_cursor == right_cursor && left_max == right_max
            }
            (Self::LogClear { header: left }, Self::LogClear { header: right })
            | (Self::Immediate { header: left }, Self::Immediate { header: right }) => {
                left == right
            }
            (
                Self::TransferTrace {
                    header: left_header,
                    request: left_request,
                },
                Self::TransferTrace {
                    header: right_header,
                    request: right_request,
                },
            ) => left_header == right_header && left_request == right_request,
            #[cfg(feature = "sd-storage")]
            (
                Self::SdWriteTrace {
                    header: left_header,
                    request: left_request,
                },
                Self::SdWriteTrace {
                    header: right_header,
                    request: right_request,
                },
            ) => left_header == right_header && left_request == right_request,
            #[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
            (
                Self::OwnerStorage {
                    header: left_header,
                    command: left_command,
                },
                Self::OwnerStorage {
                    header: right_header,
                    command: right_command,
                },
            ) => left_header == right_header && left_command == right_command,
            #[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
            (
                Self::Bulk {
                    header: left_header,
                    action: left_action,
                },
                Self::Bulk {
                    header: right_header,
                    action: right_action,
                },
            ) => {
                left_header == right_header
                    && bulk_action_kind(left_action) == bulk_action_kind(right_action)
            }
            _ => false,
        }
    }
}

impl Eq for RuntimeCommand {}

#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
fn bulk_action_kind(action: &crate::bulk_protocol_dispatch::BulkProtocolAction) -> u8 {
    match action {
        crate::bulk_protocol_dispatch::BulkProtocolAction::Owner { .. } => 0,
        crate::bulk_protocol_dispatch::BulkProtocolAction::Immediate { .. } => 1,
        crate::bulk_protocol_dispatch::BulkProtocolAction::AcceptedEvent => 2,
        crate::bulk_protocol_dispatch::BulkProtocolAction::DeferredStatus { .. } => 3,
        crate::bulk_protocol_dispatch::BulkProtocolAction::DeferredFinish { .. } => 4,
    }
}

impl RuntimeCommand {
    pub fn header(&self) -> FrameHeader {
        match self {
            RuntimeCommand::Hardware(pending) => pending.header,
            RuntimeCommand::LogGet { header, .. }
            | RuntimeCommand::LogClear { header }
            | RuntimeCommand::Immediate { header }
            | RuntimeCommand::TransferTrace { header, .. } => *header,
            #[cfg(feature = "sd-storage")]
            RuntimeCommand::SdWriteTrace { header, .. } => *header,
            #[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
            RuntimeCommand::OwnerStorage { header, .. } => *header,
            #[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
            RuntimeCommand::Bulk { header, .. } => *header,
        }
    }
}

/// Parse one request without reading or mutating diagnostic snapshot/log state.
/// The runtime aggregator supplies the committed snapshot and services log queries.
pub fn poll_runtime_command(
    serial: &mut SerialState,
    snapshot: DiagnosticSnapshot,
    storage: &mut dyn StorageHandle,
) -> Option<RuntimeCommand> {
    poll_runtime_command_inner(serial, snapshot, storage, None, None, None)
}

#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
pub fn poll_runtime_command_owner_storage(
    serial: &mut SerialState,
    snapshot: DiagnosticSnapshot,
    storage: &mut dyn StorageHandle,
    transfer: &mut [u8],
) -> Option<RuntimeCommand> {
    poll_runtime_command_inner(serial, snapshot, storage, Some(transfer), None, None)
}

#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
pub fn poll_runtime_command_owner_storage_profiled(
    serial: &mut SerialState,
    snapshot: DiagnosticSnapshot,
    storage: &mut dyn StorageHandle,
    transfer: &mut [u8],
    observe: &mut dyn FnMut(RuntimeProfileEvent),
) -> Option<RuntimeCommand> {
    poll_runtime_command_inner(
        serial,
        snapshot,
        storage,
        Some(transfer),
        None,
        Some(observe),
    )
}

#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
pub fn poll_runtime_command_bulk_owner_storage(
    serial: &mut SerialState,
    snapshot: DiagnosticSnapshot,
    storage: &mut dyn StorageHandle,
    transfer: &mut [u8],
    bulk: &mut crate::bulk_transfer_coordinator::BulkTransferCoordinator,
) -> Option<RuntimeCommand> {
    poll_runtime_command_inner(serial, snapshot, storage, Some(transfer), Some(bulk), None)
}

#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
pub fn poll_runtime_command_bulk_owner_storage_profiled(
    serial: &mut SerialState,
    snapshot: DiagnosticSnapshot,
    storage: &mut dyn StorageHandle,
    transfer: &mut [u8],
    bulk: &mut crate::bulk_transfer_coordinator::BulkTransferCoordinator,
    observe: &mut dyn FnMut(RuntimeProfileEvent),
) -> Option<RuntimeCommand> {
    poll_runtime_command_inner(
        serial,
        snapshot,
        storage,
        Some(transfer),
        Some(bulk),
        Some(observe),
    )
}

fn poll_runtime_command_inner(
    serial: &mut SerialState,
    snapshot: DiagnosticSnapshot,
    storage: &mut dyn StorageHandle,
    #[cfg_attr(
        not(all(feature = "diagnostic-console", feature = "sd-storage")),
        allow(unused_variables)
    )]
    owner_transfer: Option<&mut [u8]>,
    #[cfg_attr(
        not(all(feature = "diagnostic-console", feature = "sd-storage")),
        allow(unused_variables)
    )]
    bulk: Option<&mut crate::bulk_transfer_coordinator::BulkTransferCoordinator>,
    mut profile_observer: Option<&mut dyn FnMut(RuntimeProfileEvent)>,
) -> Option<RuntimeCommand> {
    let mut frame_buf = [0u8; MAX_FRAME_BYTES];
    let frame_len = serial.next_frame(&mut frame_buf)?;
    let decoded = decode_frame_in_place_profiled(&mut frame_buf, frame_len, |event| {
        if let Some(observe) = profile_observer.as_mut() {
            observe(RuntimeProfileEvent::Frame(event));
        }
    });
    let (header, payload_range) = match decoded {
        Ok(value) => value,
        Err(binbook_diagnostic_protocol::ProtocolError::UnknownOpcode) => {
            queue_unknown_decoded_response(serial, &frame_buf);
            serial.note_protocol_error();
            return None;
        }
        Err(_) => {
            serial.note_protocol_error();
            return None;
        }
    };

    let mut context = CommandContext::new(
        snapshot.current_page,
        snapshot.page_count,
        snapshot.last_error,
        snapshot.panel_mode as u8,
    );
    context.refresh_mode = snapshot.refresh_mode;
    context.protocol_errors = serial.protocol_error_count();
    context.dropped_records = snapshot.dropped_log_count;

    if header.opcode == Opcode::StoreTransferTrace {
        return match decode_transfer_trace_request(&frame_buf[payload_range.clone()]) {
            Ok(request) => Some(RuntimeCommand::TransferTrace { header, request }),
            Err(_) => {
                queue_runtime_response(serial, header, Status::BadRequest, &[]);
                Some(RuntimeCommand::Immediate { header })
            }
        };
    }

    #[cfg(feature = "sd-storage")]
    if header.opcode == Opcode::StoreSdWriteTrace {
        return match decode_sd_write_trace_request(&frame_buf[payload_range.clone()]) {
            Ok(request) => Some(RuntimeCommand::SdWriteTrace { header, request }),
            Err(_) => {
                queue_runtime_response(serial, header, Status::BadRequest, &[]);
                Some(RuntimeCommand::Immediate { header })
            }
        };
    }

    #[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
    if let Some(bulk) = bulk {
        if matches!(
            header.opcode,
            Opcode::StoreBulkOpen
                | Opcode::StoreBulkData
                | Opcode::StoreBulkCredit
                | Opcode::StoreBulkStatus
                | Opcode::StoreBulkFinish
                | Opcode::StoreBulkAbort
        ) {
            let action = match profile_observer.as_mut() {
                Some(observe) => crate::bulk_protocol_dispatch::dispatch_bulk_protocol_profiled(
                    bulk,
                    header,
                    &frame_buf[payload_range.clone()],
                    |event| observe(RuntimeProfileEvent::Bulk(event)),
                ),
                None => crate::bulk_protocol_dispatch::dispatch_bulk_protocol(
                    bulk,
                    header,
                    &frame_buf[payload_range.clone()],
                ),
            };
            return Some(RuntimeCommand::Bulk { header, action });
        }
    }

    #[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
    if let Some(transfer) = owner_transfer {
        if matches!(
            header.opcode,
            Opcode::StoreList
                | Opcode::StoreRead
                | Opcode::StoreDelete
                | Opcode::StoreUploadBegin
                | Opcode::StoreUploadWrite
                | Opcode::StoreUploadCommit
                | Opcode::StoreAbort
        ) {
            return match crate::diag_owner_storage::decode_owner_storage_command(
                header.opcode,
                &frame_buf[payload_range.clone()],
                transfer,
            ) {
                Ok(command) => Some(RuntimeCommand::OwnerStorage { header, command }),
                Err(error) => {
                    let status = match error {
                        crate::diag_owner_storage::OwnerStorageDecodeError::UnsupportedBackend => {
                            Status::Unsupported
                        }
                        crate::diag_owner_storage::OwnerStorageDecodeError::BadPayload
                        | crate::diag_owner_storage::OwnerStorageDecodeError::Name
                        | crate::diag_owner_storage::OwnerStorageDecodeError::TooLarge
                        | crate::diag_owner_storage::OwnerStorageDecodeError::UnsupportedOpcode => {
                            Status::BadRequest
                        }
                    };
                    queue_runtime_response(serial, header, status, &[]);
                    Some(RuntimeCommand::Immediate { header })
                }
            };
        }
    }
    let mut response_payload = [0u8; MAX_PAYLOAD_BYTES];
    let result = dispatch_command(
        header,
        &frame_buf[payload_range],
        &mut context,
        &mut response_payload,
        storage,
    );
    match result {
        DispatchResult::KeyPress { button } => Some(RuntimeCommand::Hardware(PendingCommand {
            header,
            action: PendingAction::KeyPress { button },
        })),
        DispatchResult::RenderTurn { turn } => Some(RuntimeCommand::Hardware(PendingCommand {
            header,
            action: PendingAction::RenderTurn { turn },
        })),
        DispatchResult::RenderPage { target_page } => {
            Some(RuntimeCommand::Hardware(PendingCommand {
                header,
                action: PendingAction::RenderPage { target_page },
            }))
        }
        DispatchResult::SetRefreshMode { mode } => Some(RuntimeCommand::Hardware(PendingCommand {
            header,
            action: PendingAction::SetRefreshMode { mode },
        })),
        DispatchResult::DisplayProbe(probe) => Some(RuntimeCommand::Hardware(PendingCommand {
            header,
            action: PendingAction::DisplayProbe(probe),
        })),
        DispatchResult::CrashGet => Some(RuntimeCommand::Hardware(PendingCommand {
            header,
            action: PendingAction::CrashGet,
        })),
        DispatchResult::CrashClear => Some(RuntimeCommand::Hardware(PendingCommand {
            header,
            action: PendingAction::CrashClear,
        })),
        DispatchResult::LogGet { cursor, max_bytes } => Some(RuntimeCommand::LogGet {
            header,
            cursor,
            max_bytes,
        }),
        DispatchResult::LogClear => Some(RuntimeCommand::LogClear { header }),
        DispatchResult::NoAction => {
            let payload_len = if header.opcode == Opcode::Page {
                encode_page_response(snapshot.current_page, &mut response_payload).unwrap_or(0)
            } else {
                0
            };
            queue_runtime_response(serial, header, Status::Ok, &response_payload[..payload_len]);
            Some(RuntimeCommand::Immediate { header })
        }
        DispatchResult::Response {
            status,
            payload_len,
        } => {
            queue_runtime_response(serial, header, status, &response_payload[..payload_len]);
            Some(RuntimeCommand::Immediate { header })
        }
    }
}

pub fn queue_runtime_response(
    serial: &mut SerialState,
    header: FrameHeader,
    status: Status,
    payload: &[u8],
) {
    build_response_frame(&header, status, payload, serial);
}

pub fn queue_runtime_event(serial: &mut SerialState, opcode: Opcode, payload: &[u8]) {
    let _ = serial.replace_tx_event(opcode, payload);
}

/// Parse one complete request. Immediate commands queue their response here;
/// hardware-backed commands return a token and intentionally queue nothing.
#[allow(clippy::too_many_arguments)]
pub fn poll_pending_command<const N: usize>(
    serial: &mut SerialState,
    current_page: u32,
    page_count: u32,
    last_error: i32,
    panel_mode: u8,
    log: &mut DiagLog<N>,
    tick_ms: u32,
    storage: &mut dyn StorageHandle,
) -> Option<PendingCommand> {
    let mut frame_buf = [0u8; MAX_FRAME_BYTES];
    let frame_len = serial.next_frame(&mut frame_buf)?;
    let mut request_payload = [0u8; MAX_PAYLOAD_BYTES];
    let (header, payload_len) = match decode_frame(&frame_buf[..frame_len], &mut request_payload) {
        Ok(value) => value,
        Err(binbook_diagnostic_protocol::ProtocolError::UnknownOpcode) => {
            queue_unknown_opcode_response(serial, &frame_buf[..frame_len], &mut request_payload);
            serial.note_protocol_error();
            return None;
        }
        Err(_) => {
            serial.note_protocol_error();
            return None;
        }
    };
    log.push(
        tick_ms,
        crate::diag_log::DiagEvent {
            level: crate::diag_log::LEVEL_INFO,
            subsystem: crate::diag_log::SUB_SERIAL,
            event: crate::diag_log::EVT_CMD_RECEIPT,
            arg0: header.opcode as i32,
            arg1: header.sequence as i32,
            arg2: 0,
        },
    );
    let mut context = CommandContext::new(current_page, page_count, last_error, panel_mode);
    context.protocol_errors = serial.protocol_error_count();
    context.dropped_records = log.dropped_records();
    context.tick_ms = tick_ms;
    let mut response_payload = [0u8; MAX_PAYLOAD_BYTES];
    let result = dispatch_command(
        header,
        &request_payload[..payload_len],
        &mut context,
        &mut response_payload,
        storage,
    );
    if header.opcode == Opcode::Key {
        if let Ok(key) = decode_key_payload(&request_payload[..payload_len]) {
            log.push(
                tick_ms,
                crate::diag_log::DiagEvent {
                    level: crate::diag_log::LEVEL_INFO,
                    subsystem: crate::diag_log::SUB_INPUT,
                    event: crate::diag_log::EVT_KEY_PRESS,
                    arg0: key.key as i32,
                    arg1: key.action as i32,
                    arg2: 0,
                },
            );
        }
    }
    if header.opcode == Opcode::Page {
        let target = match result {
            DispatchResult::RenderPage { target_page } => target_page,
            DispatchResult::NoAction | DispatchResult::SetRefreshMode { .. } => current_page,
            _ => current_page,
        };
        log.push(
            tick_ms,
            crate::diag_log::DiagEvent {
                level: crate::diag_log::LEVEL_INFO,
                subsystem: crate::diag_log::SUB_NAV,
                event: crate::diag_log::EVT_PAGE_DECISION,
                arg0: current_page as i32,
                arg1: target as i32,
                arg2: 0,
            },
        );
    }
    let pending = match result {
        DispatchResult::KeyPress { button } => Some(PendingAction::KeyPress { button }),
        DispatchResult::RenderTurn { turn } => Some(PendingAction::RenderTurn { turn }),
        DispatchResult::RenderPage { target_page } => {
            Some(PendingAction::RenderPage { target_page })
        }
        DispatchResult::SetRefreshMode { mode } => Some(PendingAction::SetRefreshMode { mode }),
        DispatchResult::DisplayProbe(probe) => Some(PendingAction::DisplayProbe(probe)),
        DispatchResult::CrashGet => Some(PendingAction::CrashGet),
        DispatchResult::CrashClear => Some(PendingAction::CrashClear),
        DispatchResult::NoAction => {
            let payload_len = if header.opcode == Opcode::Page {
                encode_page_response(current_page, &mut response_payload).unwrap_or(0)
            } else {
                0
            };
            build_response_frame(
                &header,
                Status::Ok,
                &response_payload[..payload_len],
                serial,
            );
            None
        }
        DispatchResult::LogGet { cursor, max_bytes } => {
            let payload_len = resolve_log_get(log, cursor, max_bytes, &mut response_payload);
            build_response_frame(
                &header,
                Status::Ok,
                &response_payload[..payload_len],
                serial,
            );
            None
        }
        DispatchResult::LogClear => {
            let (next_cursor, dropped_log_count) = resolve_log_clear(log);
            let payload_len = encode_log_response_header(
                LogResponseHeader {
                    next_cursor,
                    dropped_log_count,
                    record_count: 0,
                },
                &mut response_payload,
            )
            .unwrap_or(0);
            build_response_frame(
                &header,
                Status::Ok,
                &response_payload[..payload_len],
                serial,
            );
            None
        }
        DispatchResult::Response {
            status,
            payload_len,
        } => {
            if status != Status::Ok {
                log.push(
                    tick_ms,
                    crate::diag_log::DiagEvent {
                        level: crate::diag_log::LEVEL_ERROR,
                        subsystem: crate::diag_log::SUB_SERIAL,
                        event: crate::diag_log::EVT_CMD_ERROR,
                        arg0: header.opcode as i32,
                        arg1: status as i32,
                        arg2: 0,
                    },
                );
            }
            build_response_frame(&header, status, &response_payload[..payload_len], serial);
            None
        }
    };
    pending.map(|action| PendingCommand { header, action })
}

pub fn complete_pending_command(
    serial: &mut SerialState,
    pending: PendingCommand,
    status: Status,
    resulting_page: u32,
    action_payload: &[u8],
) -> Result<(), TransportError> {
    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    let payload_len = if status != Status::Ok {
        0
    } else if pending.header.opcode == Opcode::Page {
        encode_page_response(resulting_page, &mut payload)
            .map_err(|_| TransportError::BufferFull)?
    } else if let PendingAction::SetRefreshMode { mode } = pending.action {
        encode_refresh_mode_payload(
            RefreshModePayload {
                action: RefreshModeAction::Set,
                mode,
            },
            &mut payload,
        )
        .map_err(|_| TransportError::BufferFull)?
    } else {
        if action_payload.len() > payload.len() {
            return Err(TransportError::BufferFull);
        }
        payload[..action_payload.len()].copy_from_slice(action_payload);
        action_payload.len()
    };
    let response_header = FrameHeader {
        kind: FrameKind::Response,
        opcode: pending.header.opcode,
        status,
        sequence: pending.header.sequence,
        payload_len: payload_len as u16,
    };
    let mut encoded = [0u8; MAX_FRAME_BYTES];
    let encoded_len = encode_frame(&response_header, &payload[..payload_len], &mut encoded)
        .map_err(|_| TransportError::BufferFull)?;
    serial.queue_tx(&encoded[..encoded_len])
}

fn queue_unknown_opcode_response(
    serial: &mut SerialState,
    frame: &[u8],
    request_payload: &mut [u8],
) {
    if let Ok((raw, _)) = decode_raw_frame(frame, request_payload) {
        if raw.kind == FrameKind::Request as u8 {
            let response = RawFrameHeader {
                kind: FrameKind::Response as u8,
                opcode: raw.opcode,
                status: Status::BadRequest as u8,
                sequence: raw.sequence,
                payload_len: 0,
            };
            let mut encoded = [0u8; MAX_FRAME_BYTES];
            if let Ok(len) = encode_raw_frame(&response, &[], &mut encoded) {
                let _ = serial.queue_tx(&encoded[..len]);
            }
        }
    }
}

fn queue_unknown_decoded_response(serial: &mut SerialState, raw: &[u8; MAX_FRAME_BYTES]) {
    if raw[3] != FrameKind::Request as u8 {
        return;
    }
    let response = RawFrameHeader {
        kind: FrameKind::Response as u8,
        opcode: raw[4],
        status: Status::BadRequest as u8,
        sequence: u16::from_le_bytes([raw[6], raw[7]]),
        payload_len: 0,
    };
    let mut encoded = [0u8; MAX_FRAME_BYTES];
    if let Ok(len) = encode_raw_frame(&response, &[], &mut encoded) {
        let _ = serial.queue_tx(&encoded[..len]);
    }
}

fn build_response_frame(
    header: &FrameHeader,
    status: Status,
    payload: &[u8],
    state: &mut SerialState,
) {
    let _ = state.replace_tx_response(*header, status, payload);
}
