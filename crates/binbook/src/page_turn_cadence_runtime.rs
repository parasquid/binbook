use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use binbook_diagnostic_protocol::{
    decode_frame, decode_log_record, decode_log_response_header, decode_status_payload, KeyCode,
    LogRecordPayload, Opcode, StatusPayload, EVT_CMD_RECEIPT, EVT_DISPLAY_ERROR,
    EVT_DISPLAY_RECOVERY, EVT_GRAY_OVERLAY_COMPLETE, EVT_GRAY_OVERLAY_START, EVT_PAGE_TURN,
    EVT_RENDER_FAILURE, EVT_TURN_DEQUEUED, EVT_TURN_DROPPED, LOG_RECORD_BYTES,
    LOG_RESPONSE_HEADER_BYTES, MAX_PAYLOAD_BYTES,
};
use serde_json::json;

use crate::{diag_protocol, serial_transport};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const TRIAL_TIMEOUT: Duration = Duration::from_secs(70);
const PAGE_COUNT: u32 = 16;

pub struct PageTurnCadenceOptions<'a> {
    pub port: &'a str,
    pub key: KeyCode,
    pub start_page: u32,
    pub turns: u16,
    pub rounds: u16,
    pub min_gap_ms: u64,
    pub max_gap_ms: u64,
    pub coarse_step_ms: u64,
    pub refine_step_ms: u64,
}

struct Sequence(u16);

impl Sequence {
    fn next(&mut self) -> u16 {
        let value = self.0;
        self.0 = self.0.saturating_add(1);
        value
    }
}

pub fn run_page_turn_cadence(
    port: &str,
    options: PageTurnCadenceOptions<'_>,
    output: Option<&Path>,
) -> Result<(), String> {
    let mut session = serialport::new(port, 115_200)
        .timeout(Duration::from_secs(2))
        .open()
        .map_err(|error| format!("failed to open {port}: {error}"))?;
    let mut file = match output {
        Some(path) => Some(File::create(path).map_err(|error| format!("create output: {error}"))?),
        None => None,
    };
    let mut sink: &mut dyn Write = match file.as_mut() {
        Some(file) => file,
        None => &mut std::io::sink(),
    };
    run_page_turn_cadence_io(&mut session, options, &mut sink)
}

pub fn run_page_turn_cadence_io<T: Read + Write>(
    io: &mut T,
    options: PageTurnCadenceOptions<'_>,
    evidence: &mut dyn Write,
) -> Result<(), String> {
    validate_options(&options)?;
    let mut sequence = Sequence(1);
    let mut trial = 1u32;
    write_json(
        evidence,
        json!({"kind":"run_start","schema_version":1,"host_unix_ms":unix_ms(),"port":options.port,"key":format!("{:?}", options.key),"start_page":options.start_page,"turns":options.turns,"rounds":options.rounds,"min_gap_ms":options.min_gap_ms,"max_gap_ms":options.max_gap_ms,"coarse_step_ms":options.coarse_step_ms,"refine_step_ms":options.refine_step_ms}),
    )?;
    let mut first_reliable_gap = None;
    for gap_ms in coarse_gaps(&options) {
        let mut gap_ok = true;
        for round in 1..=options.rounds {
            gap_ok &= run_trial(io, evidence, &mut sequence, &options, trial, round, gap_ms)?;
            trial = trial.saturating_add(1);
        }
        if gap_ok {
            first_reliable_gap = Some(gap_ms);
            break;
        }
    }
    if let Some(reliable_gap) = first_reliable_gap {
        let start = reliable_gap
            .saturating_sub(options.coarse_step_ms)
            .saturating_add(options.refine_step_ms)
            .max(options.min_gap_ms);
        let mut gap_ms = start;
        while gap_ms < reliable_gap {
            for round in 1..=options.rounds {
                let _ = run_trial(io, evidence, &mut sequence, &options, trial, round, gap_ms)?;
                trial = trial.saturating_add(1);
            }
            gap_ms = gap_ms.saturating_add(options.refine_step_ms);
        }
    }
    write_json(
        evidence,
        json!({"kind":"run_result","schema_version":1,"trial_count":trial.saturating_sub(1),"error_count":0}),
    )
}

fn validate_options(options: &PageTurnCadenceOptions<'_>) -> Result<(), String> {
    if options.turns == 0 || options.rounds == 0 {
        return Err("turns and rounds must be greater than zero".into());
    }
    if options.coarse_step_ms == 0 || options.refine_step_ms == 0 {
        return Err("step sizes must be greater than zero".into());
    }
    if options.min_gap_ms > options.max_gap_ms {
        return Err("min gap must not exceed max gap".into());
    }
    if expected_page(options.start_page, options.key, options.turns) >= PAGE_COUNT {
        return Err("turn sequence exceeds diagnostic fixture page count".into());
    }
    Ok(())
}

fn coarse_gaps(options: &PageTurnCadenceOptions<'_>) -> Vec<u64> {
    let mut gaps = Vec::new();
    let mut gap = options.min_gap_ms;
    while gap <= options.max_gap_ms {
        gaps.push(gap);
        let Some(next) = gap.checked_add(options.coarse_step_ms) else {
            break;
        };
        if next == gap {
            break;
        }
        gap = next;
    }
    if gaps.last().copied() != Some(options.max_gap_ms) {
        gaps.push(options.max_gap_ms);
    }
    gaps.sort_unstable();
    gaps.dedup();
    gaps
}

fn run_trial<T: Read + Write>(
    io: &mut T,
    evidence: &mut dyn Write,
    sequence: &mut Sequence,
    options: &PageTurnCadenceOptions<'_>,
    trial: u32,
    round: u16,
    gap_ms: u64,
) -> Result<bool, String> {
    let trial_start_unix_ms = unix_ms();
    write_json(
        evidence,
        json!({"kind":"trial_start","schema_version":1,"trial":trial,"round":round,"gap_ms":gap_ms,"host_unix_ms":trial_start_unix_ms}),
    )?;
    let goto_sequence = sequence.next();
    let goto = diag_protocol::page_goto_request(goto_sequence, options.start_page);
    let response = serial_transport::send_and_receive_io(
        io,
        &goto,
        Opcode::Page,
        goto_sequence,
        TRIAL_TIMEOUT,
    )?;
    if decode_page(&response, goto_sequence)? != options.start_page {
        return Err(format!("trial {trial}: PAGE GOTO failed"));
    }

    let clear_sequence = sequence.next();
    let clear = diag_protocol::log_clear_request(clear_sequence);
    let clear_response = serial_transport::send_and_receive_io(
        io,
        &clear,
        Opcode::LogClear,
        clear_sequence,
        REQUEST_TIMEOUT,
    )?;
    let mut cursor = decode_log_batch(&clear_response, clear_sequence)?.0;
    let start = Instant::now();
    let batch_start_unix_ms = unix_ms();
    let mut key_sequences = Vec::new();
    let mut batch = Vec::new();
    for turn_index in 0..options.turns {
        let request_sequence = sequence.next();
        key_sequences.push(request_sequence);
        let key_host_unix_ms =
            batch_start_unix_ms.saturating_add(u128::from(turn_index) * u128::from(gap_ms));
        write_json(
            evidence,
            json!({"kind":"key_sent","schema_version":1,"trial":trial,"round":round,"gap_ms":gap_ms,"turn_index":turn_index,"sequence":request_sequence,"host_elapsed_ms":u128::from(turn_index) * u128::from(gap_ms),"host_unix_ms":key_host_unix_ms}),
        )?;
        batch.extend_from_slice(&diag_protocol::key_request(request_sequence, options.key));
    }
    let responses = serial_transport::send_batch_observed_io(
        io,
        &batch,
        Opcode::Key,
        &key_sequences,
        TRIAL_TIMEOUT,
        gap_ms,
    )?;
    for response in responses {
        let turn_index = key_sequences
            .iter()
            .position(|sequence| *sequence == response.sequence)
            .unwrap_or(0);
        write_json(
            evidence,
            json!({"kind":"key_response","schema_version":1,"trial":trial,"round":round,"gap_ms":gap_ms,"turn_index":turn_index,"sequence":response.sequence,"response_elapsed_ms":response.elapsed_ms,"host_elapsed_ms":start.elapsed().as_millis(),"host_unix_ms":batch_start_unix_ms.saturating_add(response.elapsed_ms)}),
        )?;
    }

    let status_sequence = sequence.next();
    let status_request = diag_protocol::status_request(status_sequence);
    let status_response = serial_transport::send_and_receive_io(
        io,
        &status_request,
        Opcode::Status,
        status_sequence,
        REQUEST_TIMEOUT,
    )?;
    let status = decode_status(&status_response, status_sequence)?;

    let deadline = Instant::now() + TRIAL_TIMEOUT;
    let mut records = Vec::new();
    while Instant::now() < deadline {
        let log_sequence = sequence.next();
        let request = diag_protocol::log_get_request(log_sequence, cursor, 496);
        let response = serial_transport::send_and_receive_io(
            io,
            &request,
            Opcode::LogGet,
            log_sequence,
            REQUEST_TIMEOUT,
        )?;
        let (next, page) = decode_log_batch(&response, log_sequence)?;
        cursor = next;
        for record in &page {
            write_log(evidence, trial, round, gap_ms, *record)?;
        }
        records.extend(page);
        if records
            .iter()
            .any(|record| record.event == EVT_GRAY_OVERLAY_COMPLETE)
        {
            break;
        }
    }
    let expected = expected_page(options.start_page, options.key, options.turns);
    let all_ok = validate_records(&records, &key_sequences, expected)
        && status.current_page == expected
        && status.dropped_log_count == 0
        && status.protocol_error_count == 0
        && status.last_error == 0;
    write_json(
        evidence,
        json!({"kind":"trial_result","schema_version":1,"trial":trial,"round":round,"gap_ms":gap_ms,"all_ok":all_ok,"expected_page":expected,"status_page":status.current_page,"error_count":if all_ok { 0 } else { 1 }}),
    )?;
    Ok(all_ok)
}

fn expected_page(start: u32, key: KeyCode, turns: u16) -> u32 {
    let mut page = start;
    for _ in 0..turns {
        page = match key {
            KeyCode::Left | KeyCode::Up => page.saturating_sub(1),
            KeyCode::Right | KeyCode::Down => page.saturating_add(1),
            KeyCode::Select | KeyCode::Back | KeyCode::Power => page,
        };
    }
    page
}

fn validate_records(records: &[LogRecordPayload], sequences: &[u16], expected: u32) -> bool {
    if records.iter().any(|record| {
        matches!(
            record.event,
            EVT_TURN_DROPPED | EVT_DISPLAY_ERROR | EVT_RENDER_FAILURE | EVT_DISPLAY_RECOVERY
        )
    }) {
        return false;
    }
    let Some(last_sequence) = sequences.last() else {
        return false;
    };
    let final_completion = records.iter().any(|record| {
        record.event == EVT_TURN_DEQUEUED
            && record.arg0 == i32::from(*last_sequence)
            && record.arg1 == expected as i32
    });
    final_completion && cadence_receipts_are_after_bw_before_gray(records, sequences)
}

fn cadence_receipts_are_after_bw_before_gray(
    records: &[LogRecordPayload],
    sequences: &[u16],
) -> bool {
    if sequences.len() < 2 {
        return true;
    }
    for pair in sequences.windows(2) {
        let previous_sequence = pair[0];
        let next_sequence = pair[1];
        let Some(previous_turn) = records
            .iter()
            .find(|record| {
                record.event == EVT_TURN_DEQUEUED && record.arg0 == i32::from(previous_sequence)
            })
            .and_then(|completion| {
                records
                    .iter()
                    .find(|record| record.event == EVT_PAGE_TURN && record.arg1 == completion.arg1)
                    .map(|record| record.tick_ms)
            })
        else {
            return false;
        };
        let Some(next_receipt) = records
            .iter()
            .find(|record| {
                record.event == EVT_CMD_RECEIPT && record.arg1 == i32::from(next_sequence)
            })
            .map(|record| record.tick_ms)
        else {
            return false;
        };
        let gray_start = records
            .iter()
            .filter(|record| {
                record.event == EVT_GRAY_OVERLAY_START && record.tick_ms > previous_turn
            })
            .map(|record| record.tick_ms)
            .min();
        if next_receipt <= previous_turn {
            return false;
        }
        if gray_start.is_some_and(|tick| next_receipt >= tick) {
            return false;
        }
    }
    true
}

fn decode_page(frame: &[u8], sequence: u16) -> Result<u32, String> {
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

fn decode_status(frame: &[u8], sequence: u16) -> Result<StatusPayload, String> {
    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    let (header, len) =
        decode_frame(frame, &mut payload).map_err(|error| format!("status frame: {error:?}"))?;
    if header.sequence != sequence {
        return Err("status response sequence mismatch".into());
    }
    decode_status_payload(&payload[..len]).map_err(|error| format!("status payload: {error:?}"))
}

fn decode_log_batch(frame: &[u8], sequence: u16) -> Result<(u32, Vec<LogRecordPayload>), String> {
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

fn write_log(
    output: &mut dyn Write,
    trial: u32,
    round: u16,
    gap_ms: u64,
    record: LogRecordPayload,
) -> Result<(), String> {
    write_json(
        output,
        json!({"kind":"log","schema_version":1,"trial":trial,"round":round,"gap_ms":gap_ms,"sequence":record.sequence,"tick_ms":record.tick_ms,"level":record.level,"subsystem":record.subsystem,"event":record.event,"arg0":record.arg0,"arg1":record.arg1,"arg2":record.arg2}),
    )
}

fn write_json(output: &mut dyn Write, value: serde_json::Value) -> Result<(), String> {
    writeln!(output, "{value}").map_err(|error| format!("write evidence: {error}"))
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}
