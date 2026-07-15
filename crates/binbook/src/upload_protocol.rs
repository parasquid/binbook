use binbook_diagnostic_protocol::{
    decode_frame, encode_frame, encode_store_abort_request, FrameHeader, FrameKind, Opcode,
    ProtocolError, Status, MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES,
};

use crate::upload_types::{UploadError, UploadTransport};

pub(crate) fn encode_request<F>(
    sequence: u16,
    opcode: Opcode,
    encode: F,
) -> Result<Vec<u8>, UploadError>
where
    F: FnOnce(&mut [u8]) -> Result<usize, ProtocolError>,
{
    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    let payload_len = encode(&mut payload)?;
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode,
        status: Status::Ok,
        sequence,
        payload_len: payload_len as u16,
    };
    let mut frame = [0u8; MAX_FRAME_BYTES];
    let frame_len = encode_frame(&header, &payload[..payload_len], &mut frame)?;
    Ok(frame[..frame_len].to_vec())
}

pub(crate) fn checked_payload(
    frame: &[u8],
    opcode: Opcode,
    sequence: u16,
) -> Result<Vec<u8>, UploadError> {
    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    let (header, payload_len) = decode_frame(frame, &mut payload)?;
    if header.kind != FrameKind::Response || header.opcode != opcode || header.sequence != sequence
    {
        return Err(UploadError::Response(format!(
            "expected response {opcode:?}/{sequence}, got {:?}/{:?}/{}/{:?}",
            header.kind, header.opcode, header.sequence, header.status
        )));
    }
    if header.status != Status::Ok {
        return Err(UploadError::DeviceStatus {
            opcode,
            sequence,
            status: header.status,
            cleanup_confirmed: false,
        });
    }
    Ok(payload[..payload_len].to_vec())
}

pub(crate) fn abort<T: UploadTransport>(
    transport: &mut T,
    upload_id: u32,
    sequence: &mut Sequence,
) -> bool {
    let abort_sequence = sequence.take();
    let Ok(frame) = encode_request(abort_sequence, Opcode::StoreAbort, |payload| {
        encode_store_abort_request(upload_id, payload)
    }) else {
        return false;
    };
    transport
        .exchange(&frame, Opcode::StoreAbort, abort_sequence)
        .and_then(|response| checked_payload(&response, Opcode::StoreAbort, abort_sequence))
        .is_ok()
}

pub(crate) fn with_abort<T: UploadTransport>(
    error: UploadError,
    transport: &mut T,
    upload_id: u32,
    sequence: &mut Sequence,
) -> UploadError {
    let cleanup_confirmed = abort(transport, upload_id, sequence);
    match error {
        UploadError::DeviceStatus {
            opcode,
            sequence,
            status,
            ..
        } => UploadError::DeviceStatus {
            opcode,
            sequence,
            status,
            cleanup_confirmed,
        },
        error => UploadError::Transport {
            message: error.to_string(),
            cleanup_confirmed,
        },
    }
}

pub(crate) struct Sequence(u16);

impl Sequence {
    pub(crate) const fn new() -> Self {
        Self(1)
    }

    pub(crate) fn take(&mut self) -> u16 {
        let value = self.0;
        self.0 = self.0.wrapping_add(1).max(1);
        value
    }
}
