use binbook_diagnostic_protocol::{
    decode_frame, decode_log_record, decode_log_response_header, decode_status_payload,
    LogRecordPayload, Opcode, StatusPayload, LOG_RECORD_BYTES, LOG_RESPONSE_HEADER_BYTES,
    MAX_PAYLOAD_BYTES,
};

use crate::diag_protocol;

pub(crate) fn validate_text(
    frame: &[u8],
    opcode: Opcode,
    sequence: u16,
    expected: &str,
) -> Result<String, String> {
    let text = diag_protocol::format_response(frame, opcode, sequence)?;
    if !text.contains(expected) {
        return Err(format!("missing expected marker {expected}"));
    }
    Ok(text)
}

pub(crate) fn decode_log_evidence(
    frame: &[u8],
    sequence: u16,
) -> Result<(u32, Vec<LogRecordPayload>), String> {
    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    let (header, len) =
        decode_frame(frame, &mut payload).map_err(|e| format!("invalid log frame: {e:?}"))?;
    if header.opcode != Opcode::LogGet || header.sequence != sequence {
        return Err(format!(
            "mismatched log response sequence {}",
            header.sequence
        ));
    }
    let body = &payload[..len];
    let log_header =
        decode_log_response_header(body).map_err(|e| format!("invalid log header: {e:?}"))?;
    let expected = LOG_RESPONSE_HEADER_BYTES + log_header.record_count as usize * LOG_RECORD_BYTES;
    if body.len() != expected {
        return Err("log record count does not match payload".into());
    }
    let mut records = Vec::with_capacity(log_header.record_count as usize);
    for chunk in body[LOG_RESPONSE_HEADER_BYTES..].chunks_exact(LOG_RECORD_BYTES) {
        records.push(decode_log_record(chunk).map_err(|e| format!("invalid log record: {e:?}"))?);
    }
    Ok((log_header.next_cursor, records))
}

pub(crate) fn decode_status_evidence(frame: &[u8], sequence: u16) -> Result<StatusPayload, String> {
    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    let (header, len) =
        decode_frame(frame, &mut payload).map_err(|e| format!("invalid status frame: {e:?}"))?;
    if header.opcode != Opcode::Status || header.sequence != sequence {
        return Err(format!(
            "mismatched status response sequence {}",
            header.sequence
        ));
    }
    decode_status_payload(&payload[..len]).map_err(|e| format!("invalid status payload: {e:?}"))
}
