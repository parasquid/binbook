use crate::{
    cobs_decode, cobs_decode_in_place, cobs_encode, crc16_ccitt_false, FrameHeader, FrameKind,
    Opcode, ProtocolError, RawFrameHeader, Status, MAGIC, MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES,
    PROTOCOL_VERSION,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameDecodeEvent {
    FramingStart,
    FramingEnd,
    CrcStart,
    CrcEnd,
    Complete {
        logical_bytes: u16,
        encoded_bytes: u16,
    },
}

pub fn decode_frame_in_place_profiled(
    buffer: &mut [u8; MAX_FRAME_BYTES],
    input_len: usize,
    mut observe: impl FnMut(FrameDecodeEvent),
) -> Result<(FrameHeader, core::ops::Range<usize>), ProtocolError> {
    if input_len > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }
    observe(FrameDecodeEvent::FramingStart);
    let raw_len = cobs_decode_in_place(buffer, input_len)?;
    observe(FrameDecodeEvent::FramingEnd);
    if raw_len < 12 || buffer[0..2] != MAGIC {
        return Err(ProtocolError::BadMagic);
    }
    if buffer[2] != PROTOCOL_VERSION {
        return Err(ProtocolError::BadVersion);
    }
    let payload_len = usize::from(u16::from_le_bytes([buffer[8], buffer[9]]));
    let total_len = 10 + payload_len + 2;
    if raw_len != total_len {
        return Err(ProtocolError::BadPayloadLength);
    }
    let expected_crc = u16::from_le_bytes([buffer[raw_len - 2], buffer[raw_len - 1]]);
    observe(FrameDecodeEvent::CrcStart);
    let actual_crc = crc16_ccitt_false(&buffer[..raw_len - 2]);
    observe(FrameDecodeEvent::CrcEnd);
    if expected_crc != actual_crc {
        return Err(ProtocolError::BadCrc);
    }
    let header = FrameHeader {
        kind: FrameKind::from_u8(buffer[3]).ok_or(ProtocolError::BadMagic)?,
        opcode: Opcode::from_u8(buffer[4]).ok_or(ProtocolError::UnknownOpcode)?,
        status: Status::from_u8(buffer[5]).ok_or(ProtocolError::BadStatus)?,
        sequence: u16::from_le_bytes([buffer[6], buffer[7]]),
        payload_len: payload_len as u16,
    };
    observe(FrameDecodeEvent::Complete {
        logical_bytes: payload_len as u16,
        encoded_bytes: input_len.min(usize::from(u16::MAX)) as u16,
    });
    Ok((header, 10..10 + payload_len))
}

pub fn decode_frame_in_place(
    buffer: &mut [u8; MAX_FRAME_BYTES],
    input_len: usize,
) -> Result<(FrameHeader, core::ops::Range<usize>), ProtocolError> {
    decode_frame_in_place_profiled(buffer, input_len, |_| {})
}

pub fn encode_frame(
    header: &FrameHeader,
    payload: &[u8],
    output: &mut [u8; MAX_FRAME_BYTES],
) -> Result<usize, ProtocolError> {
    encode_raw_frame(
        &RawFrameHeader {
            kind: header.kind as u8,
            opcode: header.opcode as u8,
            status: header.status as u8,
            sequence: header.sequence,
            payload_len: header.payload_len,
        },
        payload,
        output,
    )
}

pub fn encode_raw_frame(
    header: &RawFrameHeader,
    payload: &[u8],
    output: &mut [u8; MAX_FRAME_BYTES],
) -> Result<usize, ProtocolError> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(ProtocolError::PayloadTooLarge);
    }
    if header.payload_len as usize != payload.len() {
        return Err(ProtocolError::BadPayloadLength);
    }

    let mut raw = [0u8; MAX_FRAME_BYTES];
    raw[0..2].copy_from_slice(&MAGIC);
    raw[2] = PROTOCOL_VERSION;
    raw[3] = header.kind;
    raw[4] = header.opcode;
    raw[5] = header.status;
    raw[6..8].copy_from_slice(&header.sequence.to_le_bytes());
    raw[8..10].copy_from_slice(&header.payload_len.to_le_bytes());
    raw[10..10 + payload.len()].copy_from_slice(payload);
    let pos = 10 + payload.len();

    let crc = crc16_ccitt_false(&raw[..pos]);
    raw[pos..pos + 2].copy_from_slice(&crc.to_le_bytes());
    let total_len = pos + 2;

    cobs_encode(&raw[..total_len], output)
}

pub fn decode_raw_frame(
    input: &[u8],
    payload_out: &mut [u8],
) -> Result<(RawFrameHeader, usize), ProtocolError> {
    decode_raw_frame_profiled(input, payload_out, |_| {})
}

fn decode_raw_frame_profiled(
    input: &[u8],
    payload_out: &mut [u8],
    mut observe: impl FnMut(FrameDecodeEvent),
) -> Result<(RawFrameHeader, usize), ProtocolError> {
    if input.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }

    let mut raw = [0u8; MAX_FRAME_BYTES];
    observe(FrameDecodeEvent::FramingStart);
    let decoded = cobs_decode(input, &mut raw);
    observe(FrameDecodeEvent::FramingEnd);
    let raw_len = decoded?;

    if raw_len < 12 {
        return Err(ProtocolError::BadMagic);
    }

    if raw[0..2] != MAGIC {
        return Err(ProtocolError::BadMagic);
    }

    if raw[2] != PROTOCOL_VERSION {
        return Err(ProtocolError::BadVersion);
    }

    let sequence = u16::from_le_bytes([raw[6], raw[7]]);
    let payload_len = u16::from_le_bytes([raw[8], raw[9]]) as usize;

    let header_len = 10;
    let total_len = header_len + payload_len + 2;

    if raw_len != total_len {
        return Err(ProtocolError::BadPayloadLength);
    }

    let expected_crc = u16::from_le_bytes([raw[raw_len - 2], raw[raw_len - 1]]);
    observe(FrameDecodeEvent::CrcStart);
    let actual_crc = crc16_ccitt_false(&raw[..raw_len - 2]);
    observe(FrameDecodeEvent::CrcEnd);
    if expected_crc != actual_crc {
        return Err(ProtocolError::BadCrc);
    }

    if payload_out.len() < payload_len {
        return Err(ProtocolError::OutputTooSmall);
    }
    payload_out[..payload_len].copy_from_slice(&raw[header_len..header_len + payload_len]);

    Ok((
        RawFrameHeader {
            kind: raw[3],
            opcode: raw[4],
            status: raw[5],
            sequence,
            payload_len: payload_len as u16,
        },
        payload_len,
    ))
}

pub fn decode_frame(
    input: &[u8],
    payload_out: &mut [u8],
) -> Result<(FrameHeader, usize), ProtocolError> {
    decode_frame_profiled(input, payload_out, |_| {})
}

pub fn decode_frame_profiled(
    input: &[u8],
    payload_out: &mut [u8],
    mut observe: impl FnMut(FrameDecodeEvent),
) -> Result<(FrameHeader, usize), ProtocolError> {
    let (raw, payload_len) = decode_raw_frame_profiled(input, payload_out, &mut observe)?;
    let header = FrameHeader {
        kind: FrameKind::from_u8(raw.kind).ok_or(ProtocolError::BadMagic)?,
        opcode: Opcode::from_u8(raw.opcode).ok_or(ProtocolError::UnknownOpcode)?,
        status: Status::from_u8(raw.status).ok_or(ProtocolError::BadStatus)?,
        sequence: raw.sequence,
        payload_len: raw.payload_len,
    };
    observe(FrameDecodeEvent::Complete {
        logical_bytes: raw.payload_len,
        encoded_bytes: input.len().min(usize::from(u16::MAX)) as u16,
    });
    Ok((header, payload_len))
}
