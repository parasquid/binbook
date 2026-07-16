use binbook_diagnostic_protocol::{
    decode_store_bulk_abort_request, decode_store_bulk_credit, decode_store_bulk_data,
    decode_store_bulk_finish_request, decode_store_bulk_open_request,
    decode_store_bulk_status_request, encode_store_bulk_abort_request,
    encode_store_bulk_finish_response, encode_store_bulk_status_response, FrameHeader, FrameKind,
    Opcode, Status, TransferDirection, STORE_BULK_STATUS_RESPONSE_BYTES,
};

use crate::{
    bulk_transfer_coordinator::{BulkTransferCoordinator, BulkTransferError},
    storage_owner::OwnedRequest,
};

pub const BULK_CONTROL_PAYLOAD_BYTES: usize = STORE_BULK_STATUS_RESPONSE_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkDispatchEvent {
    CrcStart,
    CrcEnd,
}

pub enum BulkProtocolAction {
    Owner {
        request: OwnedRequest<'static>,
        reply_to: Option<FrameHeader>,
        reply_stream_id: Option<u32>,
    },
    Immediate {
        status: Status,
        payload: [u8; BULK_CONTROL_PAYLOAD_BYTES],
        payload_len: usize,
    },
    AcceptedEvent,
    DeferredStatus {
        stream_id: u32,
    },
    DeferredFinish {
        stream_id: u32,
    },
}

pub fn dispatch_bulk_protocol(
    coordinator: &mut BulkTransferCoordinator,
    header: FrameHeader,
    payload: &[u8],
) -> BulkProtocolAction {
    dispatch_bulk_protocol_profiled(coordinator, header, payload, |_| {})
}

pub fn dispatch_bulk_protocol_profiled(
    coordinator: &mut BulkTransferCoordinator,
    header: FrameHeader,
    payload: &[u8],
    mut observe: impl FnMut(BulkDispatchEvent),
) -> BulkProtocolAction {
    if header.status != Status::Ok {
        return bad_request();
    }
    match header.opcode {
        Opcode::StoreBulkOpen if is_request(header) => {
            let Ok(open) = decode_store_bulk_open_request(payload) else {
                return bad_request();
            };
            owner_or_error(coordinator.begin_open(open), Some(header), None)
        }
        Opcode::StoreBulkData if is_event(header) => {
            let Ok(data) = decode_store_bulk_data(payload) else {
                return bad_request();
            };
            match coordinator.admit_upload(data) {
                Ok(request) => BulkProtocolAction::Owner {
                    request,
                    reply_to: None,
                    reply_stream_id: None,
                },
                Err(_) => bad_request(),
            }
        }
        Opcode::StoreBulkCredit if is_event(header) => {
            let Ok(credit) = decode_store_bulk_credit(payload) else {
                return bad_request();
            };
            observe(BulkDispatchEvent::CrcStart);
            let applied = coordinator.apply_readback_credit(credit);
            observe(BulkDispatchEvent::CrcEnd);
            match applied {
                Ok(()) => BulkProtocolAction::AcceptedEvent,
                Err(_) => bad_request(),
            }
        }
        Opcode::StoreBulkStatus if is_request(header) => {
            let Ok(stream_id) = decode_store_bulk_status_request(payload) else {
                return bad_request();
            };
            if coordinator.direction() == TransferDirection::Upload
                && coordinator.pending_owner() != 0
            {
                return BulkProtocolAction::DeferredStatus { stream_id };
            }
            let Ok(status) = coordinator.status(stream_id) else {
                return bad_request();
            };
            encode_immediate(|out| encode_store_bulk_status_response(&status, out))
        }
        Opcode::StoreBulkFinish if is_request(header) => {
            let Ok(stream_id) = decode_store_bulk_finish_request(payload) else {
                return bad_request();
            };
            match coordinator.direction() {
                TransferDirection::Upload if coordinator.pending_owner() != 0 => {
                    BulkProtocolAction::DeferredFinish { stream_id }
                }
                TransferDirection::Upload => owner_or_error(
                    coordinator.begin_upload_finish(stream_id),
                    Some(header),
                    None,
                ),
                TransferDirection::Readback => {
                    let Ok(finish) = coordinator.finish_readback(stream_id) else {
                        return bad_request();
                    };
                    encode_immediate(|out| encode_store_bulk_finish_response(&finish, out))
                }
                TransferDirection::Unspecified => bad_request(),
            }
        }
        Opcode::StoreBulkAbort if is_request(header) => {
            let Ok(stream_id) = decode_store_bulk_abort_request(payload) else {
                return bad_request();
            };
            match coordinator.direction() {
                TransferDirection::Upload => owner_or_error(
                    coordinator.begin_upload_abort(stream_id),
                    Some(header),
                    Some(stream_id),
                ),
                TransferDirection::Readback => {
                    if coordinator.abort_readback(stream_id).is_err() {
                        return bad_request();
                    }
                    encode_immediate(|out| encode_store_bulk_abort_request(stream_id, out))
                }
                TransferDirection::Unspecified => bad_request(),
            }
        }
        _ => bad_request(),
    }
}

fn owner_or_error(
    result: Result<OwnedRequest<'static>, BulkTransferError>,
    reply_to: Option<FrameHeader>,
    reply_stream_id: Option<u32>,
) -> BulkProtocolAction {
    match result {
        Ok(request) => BulkProtocolAction::Owner {
            request,
            reply_to,
            reply_stream_id,
        },
        Err(_) => bad_request(),
    }
}

fn encode_immediate(
    encode: impl FnOnce(&mut [u8]) -> Result<usize, binbook_diagnostic_protocol::ProtocolError>,
) -> BulkProtocolAction {
    let mut payload = [0; BULK_CONTROL_PAYLOAD_BYTES];
    match encode(&mut payload) {
        Ok(payload_len) => BulkProtocolAction::Immediate {
            status: Status::Ok,
            payload,
            payload_len,
        },
        Err(_) => BulkProtocolAction::Immediate {
            status: Status::InternalError,
            payload,
            payload_len: 0,
        },
    }
}

fn bad_request() -> BulkProtocolAction {
    BulkProtocolAction::Immediate {
        status: Status::BadRequest,
        payload: [0; BULK_CONTROL_PAYLOAD_BYTES],
        payload_len: 0,
    }
}

fn is_request(header: FrameHeader) -> bool {
    header.kind == FrameKind::Request && header.sequence != 0
}

fn is_event(header: FrameHeader) -> bool {
    header.kind == FrameKind::Event && header.sequence == 0
}
