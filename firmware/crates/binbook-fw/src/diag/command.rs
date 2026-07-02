use binbook_diagnostic_protocol::{
    decode_key_payload, decode_log_get_payload, decode_page_payload, decode_probe_payload,
    decode_store_abort_request, decode_store_delete_request, decode_store_list_request,
    decode_store_read_request, decode_store_upload_begin_request,
    decode_store_upload_commit_request, decode_store_upload_write_request, encode_hello_response,
    encode_status_payload, encode_store_upload_begin_response, encode_store_upload_write_response,
    FrameHeader, FrameKind, HelloResponse, KeyAction, KeyCode, Opcode, PageAction, PanelModeCode,
    ProbeCode, Status, StatusPayload, ALL_CAPABILITIES, MAX_FRAME_BYTES, PROTOCOL_VERSION,
};

use crate::{
    diag_storage::StorageHandle,
    input::{apply_page_turn, Button, PageTurn},
};

const FIRMWARE_NAME: &str = "binbook-fw";
const TARGET: &str = "xteink-x4";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayProbeKind {
    FullRefreshCurrent,
    ClearWhite,
    WindowCorners,
    PartialWindowBusyMatrix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchResult {
    RenderTurn { turn: PageTurn },
    RenderPage { target_page: u32 },
    NoAction,
    LogGet { cursor: u32, max_bytes: u16 },
    LogClear,
    CrashGet,
    CrashClear,
    DisplayProbe(DisplayProbeKind),
    Response { status: Status, payload_len: usize },
}

pub struct CommandContext {
    pub current_page: u32,
    pub page_count: u32,
    pub last_error: i32,
    pub panel_mode: PanelModeCode,
    pub protocol_errors: u32,
    pub dropped_records: u32,
    pub tick_ms: u32,
}

impl CommandContext {
    pub fn new(current_page: u32, page_count: u32, last_error: i32, panel_mode: u8) -> Self {
        Self {
            current_page,
            page_count,
            last_error,
            panel_mode: PanelModeCode::from_u8(panel_mode).unwrap_or(PanelModeCode::Unknown),
            protocol_errors: 0,
            dropped_records: 0,
            tick_ms: 0,
        }
    }
}

pub fn keycode_to_button(key: KeyCode) -> Option<Button> {
    match key {
        KeyCode::Left => Some(Button::Left),
        KeyCode::Right => Some(Button::Right),
        KeyCode::Up => Some(Button::Up),
        KeyCode::Down => Some(Button::Down),
        KeyCode::Select => Some(Button::Select),
        KeyCode::Back => Some(Button::Back),
        KeyCode::Power => Some(Button::Power),
    }
}

fn resolve_page_action(action: PageAction, page_index: Option<u32>, ctx: &CommandContext) -> u32 {
    match action {
        PageAction::Next => apply_page_turn(ctx.current_page, ctx.page_count, PageTurn::Next),
        PageAction::Previous => {
            apply_page_turn(ctx.current_page, ctx.page_count, PageTurn::Previous)
        }
        PageAction::First => 0,
        PageAction::Last => ctx.page_count.saturating_sub(1),
        PageAction::Goto => {
            let raw = page_index.unwrap_or(0);
            raw.min(ctx.page_count.saturating_sub(1))
        }
        PageAction::Current => ctx.current_page,
    }
}

pub fn dispatch_command(
    header: FrameHeader,
    payload: &[u8],
    ctx: &mut CommandContext,
    resp_buf: &mut [u8],
    storage: &mut dyn StorageHandle,
) -> DispatchResult {
    if header.kind != FrameKind::Request || header.status != Status::Ok {
        return DispatchResult::Response {
            status: Status::BadRequest,
            payload_len: 0,
        };
    }

    match header.opcode {
        Opcode::Hello => {
            let hello = HelloResponse {
                protocol_version: PROTOCOL_VERSION,
                max_frame_bytes: MAX_FRAME_BYTES as u16,
                capabilities: ALL_CAPABILITIES,
                firmware_name: FIRMWARE_NAME,
                target: TARGET,
            };
            match encode_hello_response(&hello, resp_buf) {
                Ok(len) => DispatchResult::Response {
                    status: Status::Ok,
                    payload_len: len,
                },
                Err(_) => DispatchResult::Response {
                    status: Status::InternalError,
                    payload_len: 0,
                },
            }
        }
        Opcode::Key => {
            let key_payload = match decode_key_payload(payload) {
                Ok(value) => value,
                Err(_) => {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    }
                }
            };
            if key_payload.action != KeyAction::Press {
                return DispatchResult::NoAction;
            }
            let button = match keycode_to_button(key_payload.key) {
                Some(b) => b,
                None => {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    }
                }
            };
            let turn = crate::input::target_page_for_button(button);
            DispatchResult::RenderTurn { turn }
        }
        Opcode::Page => {
            let page_payload = match decode_page_payload(payload) {
                Ok(value) => value,
                Err(_) => {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    }
                }
            };
            if page_payload.action == PageAction::Goto {
                let raw = page_payload.page_index.unwrap_or(0);
                if raw >= ctx.page_count {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    };
                }
            }
            let target = resolve_page_action(page_payload.action, page_payload.page_index, ctx);
            if target == ctx.current_page {
                DispatchResult::NoAction
            } else {
                DispatchResult::RenderPage {
                    target_page: target,
                }
            }
        }
        Opcode::Status => {
            let status_payload = StatusPayload {
                current_page: ctx.current_page,
                page_count: ctx.page_count,
                panel_mode: ctx.panel_mode,
                dropped_log_count: ctx.dropped_records,
                protocol_error_count: ctx.protocol_errors,
                last_error: ctx.last_error,
            };
            match encode_status_payload(status_payload, resp_buf) {
                Ok(len) => DispatchResult::Response {
                    status: Status::Ok,
                    payload_len: len,
                },
                Err(_) => DispatchResult::Response {
                    status: Status::InternalError,
                    payload_len: 0,
                },
            }
        }
        Opcode::DisplayProbe => match decode_probe_payload(payload) {
            Ok(ProbeCode::FullRefreshCurrent) => {
                DispatchResult::DisplayProbe(DisplayProbeKind::FullRefreshCurrent)
            }
            Ok(ProbeCode::ClearWhite) => DispatchResult::DisplayProbe(DisplayProbeKind::ClearWhite),
            Ok(ProbeCode::WindowCorners) => {
                DispatchResult::DisplayProbe(DisplayProbeKind::WindowCorners)
            }
            Ok(ProbeCode::PartialWindowBusyMatrix) => {
                DispatchResult::DisplayProbe(DisplayProbeKind::PartialWindowBusyMatrix)
            }
            Err(_) => DispatchResult::Response {
                status: Status::BadRequest,
                payload_len: 0,
            },
        },
        Opcode::LogGet => {
            let get_payload = match decode_log_get_payload(payload) {
                Ok(p) => p,
                Err(_) => {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    }
                }
            };
            DispatchResult::LogGet {
                cursor: get_payload.cursor_sequence,
                max_bytes: get_payload.max_bytes,
            }
        }
        Opcode::LogClear => DispatchResult::LogClear,
        Opcode::CrashGet => DispatchResult::CrashGet,
        Opcode::CrashClear => DispatchResult::CrashClear,
        Opcode::StoreList => {
            let req = match decode_store_list_request(payload) {
                Ok(r) => r,
                Err(_) => {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    }
                }
            };
            match storage.store_list(req.backend, req.path, resp_buf) {
                Ok(len) => DispatchResult::Response {
                    status: Status::Ok,
                    payload_len: len,
                },
                Err(_) => DispatchResult::Response {
                    status: Status::InternalError,
                    payload_len: 0,
                },
            }
        }
        Opcode::StoreRead => {
            let req = match decode_store_read_request(payload) {
                Ok(r) => r,
                Err(_) => {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    }
                }
            };
            match storage.store_read(req.backend, req.path, resp_buf) {
                Ok(len) => DispatchResult::Response {
                    status: Status::Ok,
                    payload_len: len,
                },
                Err(_) => DispatchResult::Response {
                    status: Status::InternalError,
                    payload_len: 0,
                },
            }
        }
        Opcode::StoreDelete => {
            let req = match decode_store_delete_request(payload) {
                Ok(r) => r,
                Err(_) => {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    }
                }
            };
            match storage.store_delete(req.backend, req.path) {
                Ok(()) => DispatchResult::Response {
                    status: Status::Ok,
                    payload_len: 0,
                },
                Err(_) => DispatchResult::Response {
                    status: Status::InternalError,
                    payload_len: 0,
                },
            }
        }
        Opcode::StoreUploadBegin => {
            let req = match decode_store_upload_begin_request(payload) {
                Ok(r) => r,
                Err(_) => {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    }
                }
            };
            match storage.store_upload_begin(
                req.backend,
                req.path,
                req.file_size,
                req.expected_crc32,
            ) {
                Ok(upload_id) => match encode_store_upload_begin_response(upload_id, resp_buf) {
                    Ok(len) => DispatchResult::Response {
                        status: Status::Ok,
                        payload_len: len,
                    },
                    Err(_) => DispatchResult::Response {
                        status: Status::InternalError,
                        payload_len: 0,
                    },
                },
                Err(_) => DispatchResult::Response {
                    status: Status::InternalError,
                    payload_len: 0,
                },
            }
        }
        Opcode::StoreUploadWrite => {
            let req = match decode_store_upload_write_request(payload) {
                Ok(r) => r,
                Err(_) => {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    }
                }
            };
            match storage.store_upload_write(req.upload_id, req.offset, req.data) {
                Ok(bytes_accepted) => {
                    match encode_store_upload_write_response(bytes_accepted, resp_buf) {
                        Ok(len) => DispatchResult::Response {
                            status: Status::Ok,
                            payload_len: len,
                        },
                        Err(_) => DispatchResult::Response {
                            status: Status::InternalError,
                            payload_len: 0,
                        },
                    }
                }
                Err(_) => DispatchResult::Response {
                    status: Status::InternalError,
                    payload_len: 0,
                },
            }
        }
        Opcode::StoreUploadCommit => {
            let upload_id = match decode_store_upload_commit_request(payload) {
                Ok(id) => id,
                Err(_) => {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    }
                }
            };
            match storage.store_upload_commit(upload_id) {
                Ok(()) => DispatchResult::Response {
                    status: Status::Ok,
                    payload_len: 0,
                },
                Err(_) => DispatchResult::Response {
                    status: Status::InternalError,
                    payload_len: 0,
                },
            }
        }
        Opcode::StoreAbort => {
            let upload_id = match decode_store_abort_request(payload) {
                Ok(id) => id,
                Err(_) => {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    }
                }
            };
            match storage.store_upload_abort(upload_id) {
                Ok(()) => DispatchResult::Response {
                    status: Status::Ok,
                    payload_len: 0,
                },
                Err(_) => DispatchResult::Response {
                    status: Status::InternalError,
                    payload_len: 0,
                },
            }
        }
    }
}
