use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use binbook_diagnostic_protocol::{
    decode_frame, decode_log_record, decode_log_response_header, decode_status_payload, KeyCode,
    LogRecordPayload, StatusPayload, EVT_DISPLAY_ERROR, EVT_DISPLAY_RECOVERY,
    EVT_GRAY_OVERLAY_COMPLETE, EVT_RENDER_FAILURE, EVT_TURN_BOUNDARY_NOOP, EVT_TURN_DEQUEUED,
    EVT_TURN_DROPPED, EVT_TURN_STARTED, LOG_RECORD_BYTES, LOG_RESPONSE_HEADER_BYTES,
    MAX_PAYLOAD_BYTES,
};
use serde_json::json;

use crate::serial_transport::ObservedResponse;

pub(super) fn write_keys(output: &mut dyn Write, evidence: KeyEvidence<'_>) -> Result<(), String> {
    for (index, ((key, sequence), target)) in evidence
        .keys
        .iter()
        .zip(evidence.sequences)
        .zip(evidence.expected)
        .enumerate()
    {
        let order = evidence
            .observed
            .iter()
            .position(|item| item.sequence == *sequence)
            .ok_or_else(|| format!("missing observed response sequence {sequence}"))?;
        let response = &evidence.observed[order];
        let from = if index == 0 {
            evidence.start
        } else {
            evidence.expected[index - 1]
        };
        write_json(
            output,
            json!({"kind":"key","schema_version":1,"round":evidence.round,"sequence":sequence,"key":format!("{key:?}"),"expected_from":from,"expected_to":target,"response_order":order,"response_elapsed_ms":response.elapsed_ms,"host_unix_ms":evidence.batch_ms+response.elapsed_ms}),
        )?;
    }
    Ok(())
}

pub(super) struct KeyEvidence<'a> {
    pub round: u16,
    pub start: u32,
    pub keys: &'a [KeyCode],
    pub sequences: &'a [u16],
    pub expected: &'a [u32],
    pub observed: &'a [ObservedResponse],
    pub batch_ms: u128,
}

pub(super) fn validate_records(
    round: u16,
    records: &[LogRecordPayload],
    sequences: &[u16],
    start: u32,
    expected: &[u32],
    expected_noops: usize,
) -> Result<(), String> {
    if records.iter().any(|record| {
        matches!(
            record.event,
            EVT_TURN_DROPPED | EVT_DISPLAY_ERROR | EVT_RENDER_FAILURE | EVT_DISPLAY_RECOVERY
        )
    }) {
        return Err(format!(
            "round {round}: error, drop, or recovery event observed"
        ));
    }
    let mut from = start;
    for (&sequence, &target) in sequences.iter().zip(expected) {
        if target != from
            && !records.iter().any(|record| {
                record.event == EVT_TURN_STARTED
                    && record.arg0 == i32::from(sequence)
                    && record.arg1 == from as i32
                    && record.arg2 == target as i32
            })
        {
            return Err(format!("round {round} sequence {sequence}: missing or mismatched TURN_STARTED; localized subsystem=reservation/request channel or FIFO state"));
        }
        if !records.iter().any(|record| {
            record.event == EVT_TURN_DEQUEUED
                && record.arg0 == i32::from(sequence)
                && record.arg1 == target as i32
        }) {
            if target == from {
                return Err(format!("round {round} sequence {sequence}: boundary request had no engine completion/no-op evidence; localized subsystem=command dispatch/reservation/request channel"));
            }
            return Err(format!("round {round} sequence {sequence}: missing completion; localized subsystem=display engine/BUSY/recovery"));
        }
        from = target;
    }
    let noops = records
        .iter()
        .filter(|record| record.event == EVT_TURN_BOUNDARY_NOOP)
        .count();
    if noops != expected_noops {
        return Err(format!(
            "round {round}: expected {expected_noops} boundary no-ops, got {noops}"
        ));
    }
    Ok(())
}

pub(super) fn evidence_complete(
    records: &[LogRecordPayload],
    _sequences: &[u16],
    _expected_noops: usize,
) -> bool {
    records
        .iter()
        .any(|record| record.event == EVT_GRAY_OVERLAY_COMPLETE)
}

pub(super) fn decode_page(frame: &[u8], sequence: u16) -> Result<u32, String> {
    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    let (header, len) =
        decode_frame(frame, &mut payload).map_err(|error| format!("page frame: {error:?}"))?;
    if header.sequence != sequence || len != 4 {
        return Err("page response mismatch".into());
    }
    Ok(u32::from_le_bytes([
        payload[0], payload[1], payload[2], payload[3],
    ]))
}

pub(super) fn decode_status(frame: &[u8], sequence: u16) -> Result<StatusPayload, String> {
    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    let (header, len) =
        decode_frame(frame, &mut payload).map_err(|error| format!("status frame: {error:?}"))?;
    if header.sequence != sequence {
        return Err("status response sequence mismatch".into());
    }
    decode_status_payload(&payload[..len]).map_err(|error| format!("status payload: {error:?}"))
}

pub(super) fn decode_log_batch(
    frame: &[u8],
    sequence: u16,
) -> Result<(u32, Vec<LogRecordPayload>), String> {
    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    let (response, len) =
        decode_frame(frame, &mut payload).map_err(|error| format!("log frame: {error:?}"))?;
    if response.sequence != sequence {
        return Err("log response sequence mismatch".into());
    }
    let header = decode_log_response_header(&payload[..len])
        .map_err(|error| format!("log header: {error:?}"))?;
    let body = &payload[LOG_RESPONSE_HEADER_BYTES..len];
    if body.len() != usize::from(header.record_count) * LOG_RECORD_BYTES {
        return Err("log record count mismatch".into());
    }
    let records = body
        .chunks_exact(LOG_RECORD_BYTES)
        .map(|chunk| decode_log_record(chunk).map_err(|error| format!("log record: {error:?}")))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((header.next_cursor, records))
}

pub(super) fn validate_status(
    round: u16,
    status: StatusPayload,
    expected: u32,
) -> Result<(), String> {
    if status.current_page != expected
        || status.page_count != 16
        || status.dropped_log_count != 0
        || status.protocol_error_count != 0
        || status.last_error != 0
    {
        Err(format!("round {round}: STATUS diverged expected_page={expected} actual_page={} page_count={} drops={} protocol_errors={} last_error={}", status.current_page, status.page_count, status.dropped_log_count, status.protocol_error_count, status.last_error))
    } else {
        Ok(())
    }
}

pub(super) fn write_status(
    output: &mut dyn Write,
    round: u16,
    status: StatusPayload,
) -> Result<(), String> {
    write_json(
        output,
        json!({"kind":"status","schema_version":1,"round":round,"host_unix_ms":unix_ms(),"current_page":status.current_page,"page_count":status.page_count,"panel_mode":status.panel_mode as u8,"dropped_log_count":status.dropped_log_count,"protocol_error_count":status.protocol_error_count,"last_error":status.last_error}),
    )
}
pub(super) fn write_log(
    output: &mut dyn Write,
    round: u16,
    record: LogRecordPayload,
) -> Result<(), String> {
    write_json(
        output,
        json!({"kind":"log","schema_version":1,"round":round,"sequence":record.sequence,"tick_ms":record.tick_ms,"level":record.level,"subsystem":record.subsystem,"event":record.event,"arg0":record.arg0,"arg1":record.arg1,"arg2":record.arg2}),
    )
}
pub(super) fn write_json(output: &mut dyn Write, value: serde_json::Value) -> Result<(), String> {
    writeln!(output, "{value}").map_err(|error| format!("write evidence: {error}"))
}
pub(super) fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}
