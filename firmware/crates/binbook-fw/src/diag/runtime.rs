use binbook_diagnostic_protocol::{
    decode_frame, decode_key_payload, decode_raw_frame, encode_frame, encode_log_response_header,
    encode_page_response, encode_raw_frame, encode_refresh_mode_payload, FrameHeader, FrameKind,
    LogResponseHeader, Opcode, RawFrameHeader, RefreshModeAction, RefreshModePayload, Status,
    MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES,
};

use crate::{diag_log::DiagLog, diag_storage::StorageHandle};

use super::{
    command::{dispatch_command, CommandContext, DispatchResult},
    resolve_log_clear, resolve_log_get, DiagnosticSnapshot, PendingAction, PendingCommand,
    SerialState, TransportError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
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
    #[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
    OwnerStorage {
        header: FrameHeader,
        command: crate::diag_owner_storage::OwnerStorageCommand,
    },
}

impl RuntimeCommand {
    pub fn header(&self) -> FrameHeader {
        match self {
            RuntimeCommand::Hardware(pending) => pending.header,
            RuntimeCommand::LogGet { header, .. }
            | RuntimeCommand::LogClear { header }
            | RuntimeCommand::Immediate { header } => *header,
            #[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
            RuntimeCommand::OwnerStorage { header, .. } => *header,
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
    poll_runtime_command_inner(serial, snapshot, storage, None)
}

#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
pub fn poll_runtime_command_owner_storage(
    serial: &mut SerialState,
    snapshot: DiagnosticSnapshot,
    storage: &mut dyn StorageHandle,
    transfer: &mut [u8],
) -> Option<RuntimeCommand> {
    poll_runtime_command_inner(serial, snapshot, storage, Some(transfer))
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
) -> Option<RuntimeCommand> {
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

    let mut context = CommandContext::new(
        snapshot.current_page,
        snapshot.page_count,
        snapshot.last_error,
        snapshot.panel_mode as u8,
    );
    context.refresh_mode = snapshot.refresh_mode;
    context.protocol_errors = serial.protocol_error_count();
    context.dropped_records = snapshot.dropped_log_count;

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
                &request_payload[..payload_len],
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
        &request_payload[..payload_len],
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

fn build_response_frame(
    header: &FrameHeader,
    status: Status,
    payload: &[u8],
    state: &mut SerialState,
) {
    let resp_header = FrameHeader {
        kind: FrameKind::Response,
        opcode: header.opcode,
        status,
        sequence: header.sequence,
        payload_len: payload.len() as u16,
    };
    let mut encoded = [0u8; MAX_FRAME_BYTES];
    let tx_len = encode_frame(&resp_header, payload, &mut encoded).unwrap_or(0);
    let _ = state.replace_tx(&encoded[..tx_len]);
}
