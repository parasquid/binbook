use crate::{
    HelloResponse, HelloResponseRef, KeyAction, KeyCode, KeyPayload, PageAction, PagePayload,
    ProtocolError, HELLO_ID_MAX_BYTES,
};

pub fn encode_hello_response(
    value: &HelloResponse<'_>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let name_bytes = value.firmware_name.as_bytes();
    let target_bytes = value.target.as_bytes();
    if name_bytes.len() > HELLO_ID_MAX_BYTES || target_bytes.len() > HELLO_ID_MAX_BYTES {
        return Err(ProtocolError::InvalidValue);
    }
    let total = 1 + 2 + 4 + 1 + name_bytes.len() + 1 + target_bytes.len();
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    let mut pos = 0;
    out[pos] = value.protocol_version;
    pos += 1;
    out[pos..pos + 2].copy_from_slice(&value.max_frame_bytes.to_le_bytes());
    pos += 2;
    out[pos..pos + 4].copy_from_slice(&value.capabilities.to_le_bytes());
    pos += 4;
    out[pos] = name_bytes.len() as u8;
    pos += 1;
    out[pos..pos + name_bytes.len()].copy_from_slice(name_bytes);
    pos += name_bytes.len();
    out[pos] = target_bytes.len() as u8;
    pos += 1;
    out[pos..pos + target_bytes.len()].copy_from_slice(target_bytes);
    pos += target_bytes.len();
    Ok(pos)
}

pub fn decode_hello_response(payload: &[u8]) -> Result<HelloResponseRef<'_>, ProtocolError> {
    if payload.len() < 8 {
        return Err(ProtocolError::PayloadTooLarge);
    }
    let mut pos = 0;
    let protocol_version = payload[pos];
    pos += 1;
    let max_frame_bytes = u16::from_le_bytes([payload[pos], payload[pos + 1]]);
    pos += 2;
    let capabilities = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let name_len = payload[pos] as usize;
    pos += 1;
    if payload.len() < pos + name_len + 1 {
        return Err(ProtocolError::PayloadTooLarge);
    }
    let firmware_name = &payload[pos..pos + name_len];
    pos += name_len;
    let target_len = payload[pos] as usize;
    pos += 1;
    if payload.len() < pos + target_len {
        return Err(ProtocolError::PayloadTooLarge);
    }
    if payload.len() != pos + target_len
        || name_len > HELLO_ID_MAX_BYTES
        || target_len > HELLO_ID_MAX_BYTES
    {
        return Err(ProtocolError::BadPayloadLength);
    }
    let target = &payload[pos..pos + target_len];
    Ok(HelloResponseRef {
        protocol_version,
        max_frame_bytes,
        capabilities,
        firmware_name,
        target,
    })
}

pub fn encode_key_payload(
    key: KeyCode,
    action: KeyAction,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    if out.len() < 2 {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = key as u8;
    out[1] = action as u8;
    Ok(2)
}

pub fn decode_key_payload(payload: &[u8]) -> Result<KeyPayload, ProtocolError> {
    if payload.len() != 2 {
        return Err(ProtocolError::BadPayloadLength);
    }
    let key = KeyCode::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)?;
    let action = KeyAction::from_u8(payload[1]).ok_or(ProtocolError::InvalidValue)?;
    Ok(KeyPayload { key, action })
}

pub fn encode_page_payload(
    action: PageAction,
    page: Option<u32>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    match action {
        PageAction::Goto => {
            let page_index = page.ok_or(ProtocolError::InvalidValue)?;
            if out.len() < 5 {
                return Err(ProtocolError::OutputTooSmall);
            }
            out[0] = action as u8;
            out[1..5].copy_from_slice(&page_index.to_le_bytes());
            Ok(5)
        }
        _ => {
            if out.is_empty() {
                return Err(ProtocolError::OutputTooSmall);
            }
            out[0] = action as u8;
            Ok(1)
        }
    }
}

pub fn decode_page_payload(payload: &[u8]) -> Result<PagePayload, ProtocolError> {
    if payload.is_empty() {
        return Err(ProtocolError::PayloadTooLarge);
    }
    let action = PageAction::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)?;
    match action {
        PageAction::Goto => {
            if payload.len() != 5 {
                return Err(ProtocolError::BadPayloadLength);
            }
            let page_index = u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]);
            Ok(PagePayload {
                action,
                page_index: Some(page_index),
            })
        }
        _ if payload.len() == 1 => Ok(PagePayload {
            action,
            page_index: None,
        }),
        _ => Err(ProtocolError::BadPayloadLength),
    }
}

pub fn encode_page_response(page: u32, out: &mut [u8]) -> Result<usize, ProtocolError> {
    if out.len() < 4 {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&page.to_le_bytes());
    Ok(4)
}
