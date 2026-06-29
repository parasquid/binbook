use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use binbook_diagnostic_protocol::{KeyCode, Opcode};
use serde_json::json;

use crate::diag_protocol;
use crate::serial_transport;

#[path = "nav_burst_evidence.rs"]
mod evidence_model;
use evidence_model::*;

use super::{expected_pages, BOUNDARY_BURST, INTERIOR_BURST};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const ROUND_TIMEOUT: Duration = Duration::from_secs(70);

pub struct NavBurstOptions<'a> {
    pub port: &'a str,
    pub rounds: u16,
    pub inter_key_ms: u64,
}

struct Sequence(u16);

impl Sequence {
    fn next(&mut self) -> u16 {
        let value = self.0;
        self.0 = self.0.saturating_add(1);
        value
    }
}

pub fn run_nav_burst(
    port: &str,
    rounds: u16,
    inter_key_ms: u64,
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
    run_nav_burst_io(
        &mut session,
        NavBurstOptions {
            port,
            rounds,
            inter_key_ms,
        },
        &mut sink,
    )
}

pub fn run_nav_burst_io<T: Read + Write>(
    io: &mut T,
    options: NavBurstOptions<'_>,
    evidence: &mut dyn Write,
) -> Result<(), String> {
    let mut sequence = Sequence(1);
    write_json(
        evidence,
        json!({"kind":"run_start","schema_version":1,"host_unix_ms":unix_ms(),"port":options.port,"rounds":options.rounds,"inter_key_ms":options.inter_key_ms}),
    )?;
    for round in 1..=options.rounds {
        if let Err(error) = run_case(
            io,
            evidence,
            &mut sequence,
            round,
            8,
            &INTERIOR_BURST,
            options.inter_key_ms,
            0,
        ) {
            write_json(
                evidence,
                json!({"kind":"run_result","schema_version":1,"rounds_completed":round-1,"key_count":u32::from(round-1)*16,"boundary_key_count":0,"error_count":1,"error":error}),
            )?;
            return Err(error);
        }
    }
    if let Err(error) = run_case(
        io,
        evidence,
        &mut sequence,
        options.rounds + 1,
        0,
        &BOUNDARY_BURST,
        options.inter_key_ms,
        2,
    ) {
        write_json(
            evidence,
            json!({"kind":"run_result","schema_version":1,"rounds_completed":options.rounds,"key_count":u32::from(options.rounds)*16,"boundary_key_count":5,"error_count":1,"error":error}),
        )?;
        return Err(error);
    }
    write_json(
        evidence,
        json!({"kind":"run_result","schema_version":1,"rounds_completed":options.rounds,"key_count":u32::from(options.rounds)*16,"boundary_key_count":5,"error_count":0}),
    )
}

fn run_case<T: Read + Write>(
    io: &mut T,
    evidence: &mut dyn Write,
    sequence: &mut Sequence,
    round: u16,
    start_page: u32,
    keys: &[KeyCode],
    inter_key_ms: u64,
    expected_noops: usize,
) -> Result<(), String> {
    let goto_sequence = sequence.next();
    let goto = diag_protocol::page_goto_request(goto_sequence, start_page);
    let response = serial_transport::send_and_receive_io(
        io,
        &goto,
        Opcode::Page,
        goto_sequence,
        ROUND_TIMEOUT,
    )?;
    if decode_page(&response, goto_sequence)? != start_page {
        return Err(format!(
            "round {round}: PAGE GOTO did not reach {start_page}"
        ));
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

    let expected = expected_pages(start_page, 16, keys);
    let key_sequences: Vec<u16> = keys.iter().map(|_| sequence.next()).collect();
    let mut batch = Vec::new();
    for (&request_sequence, &key) in key_sequences.iter().zip(keys) {
        batch.extend_from_slice(&diag_protocol::key_request(request_sequence, key));
    }
    let batch_unix_ms = unix_ms();
    let observed = serial_transport::send_batch_observed_io(
        io,
        &batch,
        Opcode::Key,
        &key_sequences,
        ROUND_TIMEOUT,
        inter_key_ms,
    )?;
    write_keys(
        evidence,
        round,
        start_page,
        keys,
        &key_sequences,
        &expected,
        &observed,
        batch_unix_ms,
    )?;

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
    let expected_page = *expected.last().unwrap_or(&start_page);
    write_status(evidence, round, status)?;

    let deadline = Instant::now() + ROUND_TIMEOUT;
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
        let (next, mut page) = decode_log_batch(&response, log_sequence)?;
        cursor = next;
        for record in &page {
            write_log(evidence, round, *record)?;
        }
        records.append(&mut page);
        if evidence_complete(&records, &key_sequences, expected_noops) {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    validate_records(
        round,
        &records,
        &key_sequences,
        start_page,
        &expected,
        expected_noops,
    )?;
    validate_status(round, status, expected_page)?;
    write_json(
        evidence,
        json!({"kind":"round_result","schema_version":1,"round":round,"expected_page":expected_page,"status_page":status.current_page,"key_count":keys.len(),"error_count":0}),
    )
}
