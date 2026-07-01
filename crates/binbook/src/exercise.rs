use std::io::{Read, Write};
use std::time::{Duration, Instant};

use binbook_diagnostic_protocol::{KeyCode, Opcode, EVT_BW_BASE_SYNC_COMPLETE};

use crate::exercise_decode::{decode_log_evidence, decode_status_evidence, validate_text};
use crate::{diag_protocol, serial_transport};

pub use crate::exercise_validation::validate_staged_gray_evidence;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const DISPLAY_REQUEST_TIMEOUT: Duration = Duration::from_secs(70);
const BATCH_DISPLAY_TIMEOUT: Duration = Duration::from_secs(210);
const LOG_POLL_INTERVAL: Duration = Duration::from_millis(500);
const LOG_BUDGET: u16 = 496;
const OVERLAY_CANCELLATION_OFFSET: Duration = Duration::from_millis(360);

pub fn run_staged_gray(port: &str) -> Result<(), String> {
    let mut session = serialport::new(port, 115_200)
        .timeout(Duration::from_secs(2))
        .open()
        .map_err(|e| format!("Failed to open {port}: {e}"))?;
    run_staged_gray_io(&mut session, port)
}

pub fn run_staged_gray_io<T: Read + Write>(io: &mut T, _port: &str) -> Result<(), String> {
    let start = Instant::now();
    let mut cursor = 0u32;
    let mut evidence = Vec::new();

    let page_three = diag_protocol::page_goto_request(1, 3);
    validate_text(
        &serial_transport::send_and_receive_io(
            io,
            &page_three,
            Opcode::Page,
            1,
            DISPLAY_REQUEST_TIMEOUT,
        )?,
        Opcode::Page,
        1,
        "current_page=3",
    )?;
    phase("nonzero baseline", start);

    let page_zero = diag_protocol::page_goto_request(2, 0);
    validate_text(
        &serial_transport::send_and_receive_io(
            io,
            &page_zero,
            Opcode::Page,
            2,
            DISPLAY_REQUEST_TIMEOUT,
        )?,
        Opcode::Page,
        2,
        "current_page=0",
    )?;
    phase("page-0 baseline", start);

    let status = diag_protocol::status_request(3);
    validate_text(
        &serial_transport::send_and_receive_io(io, &status, Opcode::Status, 3, REQUEST_TIMEOUT)?,
        Opcode::Status,
        3,
        "current_page=0",
    )?;
    phase("status baseline", start);

    let clear = diag_protocol::log_clear_request(4);
    validate_text(
        &serial_transport::send_and_receive_io(io, &clear, Opcode::LogClear, 4, REQUEST_TIMEOUT)?,
        Opcode::LogClear,
        4,
        "record_count=0",
    )?;
    phase("clear logs", start);

    let key_right = diag_protocol::key_request(5, KeyCode::Right);
    validate_text(
        &serial_transport::send_and_receive_io(
            io,
            &key_right,
            Opcode::Key,
            5,
            DISPLAY_REQUEST_TIMEOUT,
        )?,
        Opcode::Key,
        5,
        "ok",
    )?;
    phase("prime gray", start);
    let poll_deadline = Instant::now() + DISPLAY_REQUEST_TIMEOUT;
    loop {
        let refresh_poll = diag_protocol::log_get_request(6, cursor, LOG_BUDGET);
        let refresh_response = serial_transport::send_and_receive_io(
            io,
            &refresh_poll,
            Opcode::LogGet,
            6,
            REQUEST_TIMEOUT,
        )?;
        let (next, mut records) = decode_log_evidence(&refresh_response, 6)?;
        cursor = next;
        evidence.append(&mut records);
        if evidence
            .iter()
            .any(|record| record.event == EVT_BW_BASE_SYNC_COMPLETE && record.arg0 == 1)
        {
            break;
        }
        if Instant::now() >= poll_deadline {
            return Err("timed out waiting for idle staged refinement".into());
        }
        std::thread::sleep(LOG_POLL_INTERVAL);
    }
    phase("idle staged refinement complete", start);

    let key_right = diag_protocol::key_request(7, KeyCode::Right);
    validate_text(
        &serial_transport::send_and_receive_io(
            io,
            &key_right,
            Opcode::Key,
            7,
            DISPLAY_REQUEST_TIMEOUT,
        )?,
        Opcode::Key,
        7,
        "ok",
    )?;
    let cancel_at = Instant::now() + OVERLAY_CANCELLATION_OFFSET;
    let overlay_poll = diag_protocol::log_get_request(10, cursor, LOG_BUDGET);
    let overlay_response = serial_transport::send_and_receive_io(
        io,
        &overlay_poll,
        Opcode::LogGet,
        10,
        REQUEST_TIMEOUT,
    )?;
    let (next, mut records) = decode_log_evidence(&overlay_response, 10)?;
    cursor = next;
    evidence.append(&mut records);
    if let Some(remaining) = cancel_at.checked_duration_since(Instant::now()) {
        std::thread::sleep(remaining);
    }
    phase("page-2 cancellation window", start);

    let mut batched = Vec::new();
    for (sequence, key) in [(8, KeyCode::Right), (9, KeyCode::Left)] {
        batched.extend_from_slice(&diag_protocol::key_request(sequence, key));
    }
    let batch_responses = serial_transport::send_batch_and_receive_io(
        io,
        &batched,
        Opcode::Key,
        &[8, 9],
        BATCH_DISPLAY_TIMEOUT,
    )?;
    for (frame, sequence) in batch_responses.into_iter().zip([8u16, 9]) {
        validate_text(&frame, Opcode::Key, sequence, "ok")?;
    }
    phase("batched turns", start);
    let poll_deadline = Instant::now() + DISPLAY_REQUEST_TIMEOUT;
    loop {
        let reseed_poll = diag_protocol::log_get_request(10, cursor, LOG_BUDGET);
        let reseed_response = serial_transport::send_and_receive_io(
            io,
            &reseed_poll,
            Opcode::LogGet,
            10,
            REQUEST_TIMEOUT,
        )?;
        let (next, mut records) = decode_log_evidence(&reseed_response, 10)?;
        cursor = next;
        evidence.append(&mut records);
        if evidence
            .iter()
            .any(|record| record.event == EVT_BW_BASE_SYNC_COMPLETE && record.arg0 == 2)
        {
            break;
        }
        if Instant::now() >= poll_deadline {
            return Err("timed out waiting for BW base-sync completion".into());
        }
        std::thread::sleep(LOG_POLL_INTERVAL);
    }
    phase("BW base sync complete", start);

    let key_right = diag_protocol::key_request(11, KeyCode::Right);
    validate_text(
        &serial_transport::send_and_receive_io(
            io,
            &key_right,
            Opcode::Key,
            11,
            DISPLAY_REQUEST_TIMEOUT,
        )?,
        Opcode::Key,
        11,
        "ok",
    )?;
    phase("final turn", start);

    let final_status = diag_protocol::status_request(12);
    let status_response = serial_transport::send_and_receive_io(
        io,
        &final_status,
        Opcode::Status,
        12,
        REQUEST_TIMEOUT,
    )?;
    let final_status = decode_status_evidence(&status_response, 12)?;
    phase("final status", start);

    let final_logs = diag_protocol::log_get_request(13, cursor, LOG_BUDGET);
    let final_response = serial_transport::send_and_receive_io(
        io,
        &final_logs,
        Opcode::LogGet,
        13,
        REQUEST_TIMEOUT,
    )?;
    let (_, mut records) = decode_log_evidence(&final_response, 13)?;
    evidence.append(&mut records);
    validate_staged_gray_evidence(final_status, &evidence)?;
    phase("final logs", start);

    Ok(())
}

fn phase(name: &str, start: Instant) {
    println!(
        "[exercise] phase={name} elapsed_ms={}",
        start.elapsed().as_millis()
    );
}
