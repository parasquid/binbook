use crate::{ProtocolError, RefreshModeAction, RefreshModeCode, RefreshModePayload};

pub fn encode_refresh_mode_payload(
    value: RefreshModePayload,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    if out.len() < 2 {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = value.action as u8;
    out[1] = value.mode as u8;
    Ok(2)
}

pub fn decode_refresh_mode_payload(payload: &[u8]) -> Result<RefreshModePayload, ProtocolError> {
    if payload.len() != 2 {
        return Err(ProtocolError::BadPayloadLength);
    }
    Ok(RefreshModePayload {
        action: RefreshModeAction::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)?,
        mode: RefreshModeCode::from_u8(payload[1]).ok_or(ProtocolError::InvalidValue)?,
    })
}
