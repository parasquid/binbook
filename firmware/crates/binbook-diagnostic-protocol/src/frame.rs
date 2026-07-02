use crate::{
    cobs_decode, cobs_encode, crc16_ccitt_false, FrameHeader, FrameKind, Opcode, ProtocolError,
    RawFrameHeader, Status, MAGIC, MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES, PROTOCOL_VERSION,
};

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
    if input.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }

    let mut raw = [0u8; MAX_FRAME_BYTES];
    let raw_len = cobs_decode(input, &mut raw)?;

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
    let actual_crc = crc16_ccitt_false(&raw[..raw_len - 2]);
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
    let (raw, payload_len) = decode_raw_frame(input, payload_out)?;
    Ok((
        FrameHeader {
            kind: FrameKind::from_u8(raw.kind).ok_or(ProtocolError::BadMagic)?,
            opcode: Opcode::from_u8(raw.opcode).ok_or(ProtocolError::UnknownOpcode)?,
            status: Status::from_u8(raw.status).ok_or(ProtocolError::BadStatus)?,
            sequence: raw.sequence,
            payload_len: raw.payload_len,
        },
        payload_len,
    ))
}
