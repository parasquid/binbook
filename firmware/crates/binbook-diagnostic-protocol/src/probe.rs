use crate::{ProbeCode, ProtocolError};

pub fn encode_probe_payload(probe: ProbeCode, out: &mut [u8]) -> Result<usize, ProtocolError> {
    if out.is_empty() {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = probe as u8;
    Ok(1)
}

pub fn decode_probe_payload(payload: &[u8]) -> Result<ProbeCode, ProtocolError> {
    if payload.len() != 1 {
        return Err(ProtocolError::BadPayloadLength);
    }
    ProbeCode::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)
}
