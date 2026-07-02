use crate::{
    LogGetPayload, LogRecordPayload, LogResponseHeader, PanelModeCode, ProtocolError,
    StatusPayload, CRASH_SUMMARY_BYTES, LOG_RESPONSE_HEADER_BYTES,
};

pub fn encode_status_payload(value: StatusPayload, out: &mut [u8]) -> Result<usize, ProtocolError> {
    let total = 4 + 4 + 1 + 4 + 4 + 4;
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    let mut pos = 0;
    out[pos..pos + 4].copy_from_slice(&value.current_page.to_le_bytes());
    pos += 4;
    out[pos..pos + 4].copy_from_slice(&value.page_count.to_le_bytes());
    pos += 4;
    out[pos] = value.panel_mode as u8;
    pos += 1;
    out[pos..pos + 4].copy_from_slice(&value.dropped_log_count.to_le_bytes());
    pos += 4;
    out[pos..pos + 4].copy_from_slice(&value.protocol_error_count.to_le_bytes());
    pos += 4;
    out[pos..pos + 4].copy_from_slice(&value.last_error.to_le_bytes());
    pos += 4;
    Ok(pos)
}

pub fn decode_status_payload(payload: &[u8]) -> Result<StatusPayload, ProtocolError> {
    let expected = 4 + 4 + 1 + 4 + 4 + 4;
    if payload.len() != expected {
        return Err(ProtocolError::BadPayloadLength);
    }
    let mut pos = 0;
    let current_page = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let page_count = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let panel_mode = PanelModeCode::from_u8(payload[pos]).ok_or(ProtocolError::InvalidValue)?;
    pos += 1;
    let dropped_log_count = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let protocol_error_count = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let last_error = i32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    Ok(StatusPayload {
        current_page,
        page_count,
        panel_mode,
        dropped_log_count,
        protocol_error_count,
        last_error,
    })
}

pub fn encode_log_get_payload(
    value: LogGetPayload,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let total = 4 + 2;
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&value.cursor_sequence.to_le_bytes());
    out[4..6].copy_from_slice(&value.max_bytes.to_le_bytes());
    Ok(total)
}

pub fn decode_log_get_payload(payload: &[u8]) -> Result<LogGetPayload, ProtocolError> {
    let expected = 4 + 2;
    if payload.len() != expected {
        return Err(ProtocolError::BadPayloadLength);
    }
    let cursor_sequence = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let max_bytes = u16::from_le_bytes([payload[4], payload[5]]);
    Ok(LogGetPayload {
        cursor_sequence,
        max_bytes,
    })
}

pub fn encode_log_response_header(
    value: LogResponseHeader,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    if out.len() < LOG_RESPONSE_HEADER_BYTES {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&value.next_cursor.to_le_bytes());
    out[4..8].copy_from_slice(&value.dropped_log_count.to_le_bytes());
    out[8..10].copy_from_slice(&value.record_count.to_le_bytes());
    Ok(LOG_RESPONSE_HEADER_BYTES)
}

pub fn decode_log_response_header(payload: &[u8]) -> Result<LogResponseHeader, ProtocolError> {
    if payload.len() < LOG_RESPONSE_HEADER_BYTES {
        return Err(ProtocolError::BadPayloadLength);
    }
    Ok(LogResponseHeader {
        next_cursor: u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]),
        dropped_log_count: u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]),
        record_count: u16::from_le_bytes([payload[8], payload[9]]),
    })
}

pub fn encode_crash_response(
    summary: Option<&[u8; CRASH_SUMMARY_BYTES]>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let required = if summary.is_some() {
        1 + CRASH_SUMMARY_BYTES
    } else {
        1
    };
    if out.len() < required {
        return Err(ProtocolError::OutputTooSmall);
    }
    match summary {
        Some(summary) => {
            out[0] = 1;
            out[1..required].copy_from_slice(summary);
        }
        None => out[0] = 0,
    }
    Ok(required)
}

pub fn decode_crash_response(payload: &[u8]) -> Result<Option<&[u8]>, ProtocolError> {
    match payload {
        [0] => Ok(None),
        bytes if bytes.len() == 1 + CRASH_SUMMARY_BYTES && bytes[0] == 1 => Ok(Some(&bytes[1..])),
        _ => Err(ProtocolError::BadPayloadLength),
    }
}

pub const LOG_RECORD_BYTES: usize = 24;

pub fn encode_log_record(value: LogRecordPayload, out: &mut [u8]) -> Result<usize, ProtocolError> {
    if out.len() < LOG_RECORD_BYTES {
        return Err(ProtocolError::OutputTooSmall);
    }
    let mut pos = 0;
    out[pos..pos + 4].copy_from_slice(&value.sequence.to_le_bytes());
    pos += 4;
    out[pos..pos + 4].copy_from_slice(&value.tick_ms.to_le_bytes());
    pos += 4;
    out[pos] = value.level;
    pos += 1;
    out[pos] = value.subsystem;
    pos += 1;
    out[pos..pos + 2].copy_from_slice(&value.event.to_le_bytes());
    pos += 2;
    out[pos..pos + 4].copy_from_slice(&value.arg0.to_le_bytes());
    pos += 4;
    out[pos..pos + 4].copy_from_slice(&value.arg1.to_le_bytes());
    pos += 4;
    out[pos..pos + 4].copy_from_slice(&value.arg2.to_le_bytes());
    pos += 4;
    Ok(pos)
}

pub fn decode_log_record(payload: &[u8]) -> Result<LogRecordPayload, ProtocolError> {
    if payload.len() < LOG_RECORD_BYTES {
        return Err(ProtocolError::PayloadTooLarge);
    }
    let mut pos = 0;
    let sequence = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let tick_ms = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let level = payload[pos];
    pos += 1;
    let subsystem = payload[pos];
    pos += 1;
    let event = u16::from_le_bytes([payload[pos], payload[pos + 1]]);
    pos += 2;
    let arg0 = i32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let arg1 = i32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let arg2 = i32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    Ok(LogRecordPayload {
        sequence,
        tick_ms,
        level,
        subsystem,
        event,
        arg0,
        arg1,
        arg2,
    })
}
