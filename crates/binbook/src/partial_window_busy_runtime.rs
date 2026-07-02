use std::fs::{create_dir_all, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use binbook_diagnostic_protocol::{
    decode_frame, decode_log_record, decode_log_response_header, LogRecordPayload, Opcode,
    EVT_DISPLAY_ERROR, EVT_PARTIAL_WINDOW_CASE_END, EVT_RENDER_FAILURE, LOG_RECORD_BYTES,
    LOG_RESPONSE_HEADER_BYTES, MAX_PAYLOAD_BYTES,
};
use serde_json::json;

use crate::{diag_protocol, serial_transport};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_TIMEOUT: Duration = Duration::from_secs(70);
const EXPECTED_LAST_CASE: i32 = 5;

pub struct PartialWindowBusyOptions<'a> {
    pub port: &'a str,
    pub rounds: u16,
}

struct Sequence(u16);

impl Sequence {
    fn next(&mut self) -> u16 {
        let value = self.0;
        self.0 = self.0.saturating_add(1);
        value
    }
}

pub fn run_partial_window_busy(
    port: &str,
    options: PartialWindowBusyOptions<'_>,
    output: Option<&Path>,
) -> Result<(), String> {
    let mut session = serialport::new(port, 115_200)
        .timeout(Duration::from_secs(2))
        .open()
        .map_err(|error| format!("failed to open {port}: {error}"))?;
    let mut file = match output {
        Some(path) => Some(File::create(output_path(path)?).map_err(|error| {
            format!("create output: {error}")
        })?),
        None => None,
    };
    let mut sink: &mut dyn Write = match file.as_mut() {
        Some(file) => file,
        None => &mut std::io::sink(),
    };
    run_partial_window_busy_io(&mut session, options, &mut sink)
}

pub fn run_partial_window_busy_io<T: Read + Write>(
    io: &mut T,
    options: PartialWindowBusyOptions<'_>,
    evidence: &mut dyn Write,
) -> Result<(), String> {
    if options.rounds == 0 {
        return Err("rounds must be greater than zero".into());
    }
    let mut sequence = Sequence(1);
    write_json(
        evidence,
        json!({"kind":"run_start","schema_version":1,"host_unix_ms":unix_ms(),"port":options.port,"rounds":options.rounds}),
    )?;

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
    let mut error_count = 0u32;

    for round in 1..=options.rounds {
        let probe_sequence = sequence.next();
        write_json(
            evidence,
            json!({"kind":"probe_request","schema_version":1,"round":round,"sequence":probe_sequence,"host_unix_ms":unix_ms()}),
        )?;
        let probe = diag_protocol::display_probe_request(
            probe_sequence,
            diag_protocol::ProbeChoice::PartialWindowBusyMatrix,
        );
        let started = Instant::now();
        let _response = serial_transport::send_and_receive_io(
            io,
            &probe,
            Opcode::DisplayProbe,
            probe_sequence,
            PROBE_TIMEOUT,
        )?;
        write_json(
            evidence,
            json!({"kind":"probe_response","schema_version":1,"round":round,"sequence":probe_sequence,"response_elapsed_ms":started.elapsed().as_millis()}),
        )?;
        let records = collect_round_logs(io, evidence, &mut sequence, round, &mut cursor)?;
        if records.iter().any(|record| {
            matches!(
                record.event,
                EVT_RENDER_FAILURE | EVT_DISPLAY_ERROR
            )
        }) {
            error_count = error_count.saturating_add(1);
        }
    }

    write_json(
        evidence,
        json!({"kind":"run_result","schema_version":1,"rounds":options.rounds,"error_count":error_count}),
    )
}

fn collect_round_logs<T: Read + Write>(
    io: &mut T,
    evidence: &mut dyn Write,
    sequence: &mut Sequence,
    round: u16,
    cursor: &mut u32,
) -> Result<Vec<LogRecordPayload>, String> {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    let mut records = Vec::new();
    while Instant::now() < deadline {
        let log_sequence = sequence.next();
        let request = diag_protocol::log_get_request(log_sequence, *cursor, 496);
        let response = serial_transport::send_and_receive_io(
            io,
            &request,
            Opcode::LogGet,
            log_sequence,
            REQUEST_TIMEOUT,
        )?;
        let (next, page) = decode_log_batch(&response, log_sequence)?;
        *cursor = next;
        for record in &page {
            write_log(evidence, round, *record)?;
        }
        records.extend(page);
        if records.iter().any(|record| {
            record.event == EVT_PARTIAL_WINDOW_CASE_END && record.arg0 == EXPECTED_LAST_CASE
        }) {
            break;
        }
    }
    Ok(records)
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

fn output_path(path: &Path) -> Result<PathBuf, String> {
    if path.extension().is_some() {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                create_dir_all(parent).map_err(|error| format!("create output dir: {error}"))?;
            }
        }
        return Ok(path.to_path_buf());
    }
    create_dir_all(path).map_err(|error| format!("create output dir: {error}"))?;
    Ok(path.join("evidence.jsonl"))
}

fn write_log(
    output: &mut dyn Write,
    round: u16,
    record: LogRecordPayload,
) -> Result<(), String> {
    write_json(
        output,
        json!({"kind":"log","schema_version":1,"round":round,"sequence":record.sequence,"tick_ms":record.tick_ms,"level":record.level,"subsystem":record.subsystem,"event":record.event,"arg0":record.arg0,"arg1":record.arg1,"arg2":record.arg2}),
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
