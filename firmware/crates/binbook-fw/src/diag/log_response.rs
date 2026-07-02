use binbook_diagnostic_protocol::{
    encode_log_record, encode_log_response_header, LogRecordPayload, LogResponseHeader,
    LOG_RECORD_BYTES, LOG_RESPONSE_HEADER_BYTES,
};

use crate::diag_log::{DiagLog, DiagLogRecord};

/// Wire format: `[next_cursor: u32 LE, dropped_log_count: u32 LE, records...]`
/// Each record is `LOG_RECORD_BYTES` (24) bytes. Only whole records within `max_bytes` are included.
pub fn resolve_log_get<const N: usize>(
    log: &DiagLog<N>,
    cursor: u32,
    max_bytes: u16,
    resp_buf: &mut [u8],
) -> usize {
    let budget = (max_bytes as usize).min(resp_buf.len());
    if budget < LOG_RESPONSE_HEADER_BYTES {
        return 0;
    }
    let record_budget = budget - LOG_RESPONSE_HEADER_BYTES;
    let max_records = record_budget / LOG_RECORD_BYTES;
    if max_records == 0 {
        let header = LogResponseHeader {
            next_cursor: cursor.max(log.oldest_sequence().unwrap_or(log.next_sequence())),
            dropped_log_count: log.dropped_records(),
            record_count: 0,
        };
        return encode_log_response_header(header, resp_buf).unwrap_or(0);
    }

    let mut records = [DiagLogRecord::default(); 64];
    let read_limit = max_records.min(64);
    let result = log.read_from_sequence(cursor, &mut records[..read_limit]);

    let mut pos = LOG_RESPONSE_HEADER_BYTES;

    for record in records.iter().take(usize::from(result.record_count)) {
        if pos + LOG_RECORD_BYTES > budget {
            break;
        }
        let rec = LogRecordPayload {
            sequence: record.sequence,
            tick_ms: record.tick_ms,
            level: record.level,
            subsystem: record.subsystem,
            event: record.event,
            arg0: record.arg0,
            arg1: record.arg1,
            arg2: record.arg2,
        };
        if let Ok(n) = encode_log_record(rec, &mut resp_buf[pos..pos + LOG_RECORD_BYTES]) {
            pos += n;
        }
    }

    let record_count = ((pos - LOG_RESPONSE_HEADER_BYTES) / LOG_RECORD_BYTES) as u16;
    let _ = encode_log_response_header(
        LogResponseHeader {
            next_cursor: result.next_cursor,
            dropped_log_count: result.dropped_log_count,
            record_count,
        },
        resp_buf,
    );

    pos
}

/// Returns `(next_cursor, dropped_log_count)` after clearing.
pub fn resolve_log_clear<const N: usize>(log: &mut DiagLog<N>) -> (u32, u32) {
    log.clear();
    (log.next_sequence(), log.dropped_records())
}
