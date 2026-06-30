#![cfg(feature = "serial-device")]

use std::io::{self, Read, Write};

use binbook_diagnostic_protocol::{
    crc16_ccitt_false, decode_frame, decode_page_payload, encode_frame, encode_log_record,
    encode_log_response_header, encode_page_response, encode_status_payload, FrameHeader,
    FrameKind, LogRecordPayload, LogResponseHeader, Opcode, PageAction, PanelModeCode, Status,
    StatusPayload, FRAME_DELIMITER, MAGIC, MAX_FRAME_BYTES, PROTOCOL_VERSION,
};

const PORT: &str = "/dev/ttyACM0";

fn open_port() -> Box<dyn serialport::SerialPort> {
    serialport::new(PORT, 115200)
        .timeout(std::time::Duration::from_secs(2))
        .open()
        .unwrap_or_else(|e| panic!("failed to open {}: {}", PORT, e))
}

struct ScriptedExerciseIo {
    reads: Vec<Vec<u8>>,
    index: usize,
    written: Vec<u8>,
}

impl ScriptedExerciseIo {
    fn new(reads: Vec<Vec<u8>>) -> Self {
        Self {
            reads,
            index: 0,
            written: Vec::new(),
        }
    }
}

impl Read for ScriptedExerciseIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.index >= self.reads.len() {
            return Ok(0);
        }
        let chunk = &self.reads[self.index];
        self.index += 1;
        buf[..chunk.len()].copy_from_slice(chunk);
        Ok(chunk.len())
    }
}

impl Write for ScriptedExerciseIo {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.written.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn find_frame(data: &[u8]) -> Option<&[u8]> {
    data.iter()
        .position(|&b| b == FRAME_DELIMITER)
        .map(|pos| &data[..=pos])
}

#[test]
fn staged_gray_exercise_uses_the_planned_transport_script() {
    let reads = staged_gray_responses();
    let mut io = ScriptedExerciseIo::new(reads);

    let result = binbook_cli::exercise::run_staged_gray_io(&mut io, PORT);

    assert!(
        result.is_ok(),
        "staged-gray exercise should run: {:?}",
        result
    );
    assert_eq!(
        io.index,
        io.reads.len(),
        "exercise should consume all scripted reads"
    );
    assert_eq!(
        decoded_request_transcript(&io.written),
        expected_request_transcript(),
        "exercise should issue the planned request order",
    );
    let first_frame = find_frame(&io.written).expect("first request frame");
    let mut payload = [0_u8; MAX_FRAME_BYTES];
    let (_, payload_len) = decode_frame(first_frame, &mut payload).unwrap();
    let page = decode_page_payload(&payload[..payload_len]).unwrap();
    assert_eq!(page.action, PageAction::Goto);
    assert_eq!(page.page_index, Some(3));
}

#[test]
fn nav_burst_exercise_uses_key_batches_and_validates_logs() {
    let mut io = ScriptedExerciseIo::new(nav_burst_responses());
    let mut evidence = Vec::new();
    let result = binbook_cli::nav_burst::run_nav_burst_io(
        &mut io,
        binbook_cli::nav_burst::NavBurstOptions {
            port: PORT,
            rounds: 1,
            inter_key_ms: 0,
        },
        &mut evidence,
    );

    assert!(result.is_ok(), "nav-burst exercise should run: {result:?}");
    assert_eq!(io.index, io.reads.len());
    let transcript = decoded_request_transcript(&io.written);
    assert_eq!(transcript.first(), Some(&(Opcode::Page, 1)));
    assert!(transcript.contains(&(Opcode::Key, 3)));
    assert!(transcript.contains(&(Opcode::Key, 18)));
    assert!(transcript.contains(&(Opcode::Page, 22)));
    let text = String::from_utf8(evidence).unwrap();
    assert!(text.contains("\"kind\":\"run_result\""));
    assert!(text.contains("\"boundary_key_count\":5"));
}

fn nav_burst_responses() -> Vec<Vec<u8>> {
    let expected = binbook_cli::nav_burst::INTERIOR_EXPECTED;
    let mut records = Vec::new();
    let mut from = 8;
    let mut record_sequence = 1;
    for (protocol_sequence, &target) in (3u16..=18).zip(expected.iter()) {
        records.push(log_record(
            record_sequence,
            100,
            binbook_diagnostic_protocol::EVT_TURN_STARTED,
            i32::from(protocol_sequence),
            from as i32,
            target as i32,
        ));
        record_sequence += 1;
        records.push(log_record(
            record_sequence,
            110,
            binbook_diagnostic_protocol::EVT_TURN_DEQUEUED,
            i32::from(protocol_sequence),
            target as i32,
            0,
        ));
        record_sequence += 1;
        from = target;
    }
    records.push(log_record(
        record_sequence,
        500,
        binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_COMPLETE,
        10,
        0,
        0,
    ));

    let mut reads = Vec::new();
    reads.extend(fragment(page_response(1, 8)));
    reads.extend(fragment(log_response(Opcode::LogClear, 2, 1, 0, &[])));
    for sequence in 3..=18 {
        reads.extend(fragment(ack(sequence, Opcode::Key)));
    }
    reads.extend(fragment(status_response(19, nav_status(10))));
    reads.extend(fragment(log_response(
        Opcode::LogGet,
        20,
        20,
        0,
        &records[..19],
    )));
    reads.extend(fragment(log_response(
        Opcode::LogGet,
        21,
        34,
        0,
        &records[19..],
    )));

    let boundary_expected = [0, 1, 0, 0, 1];
    let mut boundary = Vec::new();
    let mut from = 0;
    let mut log_sequence = 34;
    for (protocol_sequence, &target) in (24u16..=28).zip(boundary_expected.iter()) {
        let event = if target == from {
            binbook_diagnostic_protocol::EVT_TURN_BOUNDARY_NOOP
        } else {
            binbook_diagnostic_protocol::EVT_TURN_STARTED
        };
        boundary.push(log_record(
            log_sequence,
            600,
            event,
            i32::from(protocol_sequence),
            from,
            target,
        ));
        log_sequence += 1;
        boundary.push(log_record(
            log_sequence,
            610,
            binbook_diagnostic_protocol::EVT_TURN_DEQUEUED,
            i32::from(protocol_sequence),
            target,
            0,
        ));
        log_sequence += 1;
        from = target;
    }
    boundary.push(log_record(
        log_sequence,
        900,
        binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_COMPLETE,
        1,
        0,
        0,
    ));
    reads.extend(fragment(page_response(22, 0)));
    reads.extend(fragment(log_response(Opcode::LogClear, 23, 34, 0, &[])));
    for sequence in 24..=28 {
        reads.extend(fragment(ack(sequence, Opcode::Key)));
    }
    reads.extend(fragment(status_response(29, nav_status(1))));
    reads.extend(fragment(log_response(Opcode::LogGet, 30, 45, 0, &boundary)));
    reads
}

fn nav_status(current_page: u32) -> StatusPayload {
    StatusPayload {
        current_page,
        page_count: 16,
        panel_mode: PanelModeCode::Bw,
        dropped_log_count: 0,
        protocol_error_count: 0,
        last_error: 0,
    }
}

fn valid_status() -> StatusPayload {
    StatusPayload {
        current_page: 3,
        page_count: 4,
        panel_mode: PanelModeCode::Bw,
        dropped_log_count: 0,
        protocol_error_count: 0,
        last_error: 0,
    }
}

fn evidence_record(
    sequence: u32,
    tick_ms: u32,
    event: u16,
    arg0: i32,
    arg1: i32,
) -> LogRecordPayload {
    LogRecordPayload {
        sequence,
        tick_ms,
        level: 1,
        subsystem: 3,
        event,
        arg0,
        arg1,
        arg2: 0,
    }
}

fn valid_evidence() -> Vec<LogRecordPayload> {
    vec![
        evidence_record(1, 100, binbook_diagnostic_protocol::EVT_TURN_DEQUEUED, 5, 1),
        evidence_record(
            2,
            450,
            binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_START,
            1,
            0,
        ),
        evidence_record(
            3,
            460,
            binbook_diagnostic_protocol::EVT_WAVEFORM_SELECTED,
            2,
            1,
        ),
        evidence_record(
            4,
            461,
            binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_ACTIVATE,
            1,
            0,
        ),
        evidence_record(
            5,
            500,
            binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_COMPLETE,
            1,
            0,
        ),
        evidence_record(
            6,
            501,
            binbook_diagnostic_protocol::EVT_BW_BASE_SYNC_START,
            1,
            0,
        ),
        evidence_record(
            7,
            510,
            binbook_diagnostic_protocol::EVT_BW_BASE_SYNC_COMPLETE,
            1,
            0,
        ),
        evidence_record(8, 600, binbook_diagnostic_protocol::EVT_TURN_DEQUEUED, 7, 2),
        evidence_record(
            9,
            950,
            binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_START,
            2,
            0,
        ),
        evidence_record(
            10,
            960,
            binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_CANCELLED,
            2,
            0,
        ),
        evidence_record(
            11,
            970,
            binbook_diagnostic_protocol::EVT_TURN_DEQUEUED,
            8,
            3,
        ),
        evidence_record(
            12,
            980,
            binbook_diagnostic_protocol::EVT_TURN_DEQUEUED,
            9,
            2,
        ),
        evidence_record(
            13,
            1_330,
            binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_START,
            2,
            0,
        ),
        evidence_record(
            14,
            1_340,
            binbook_diagnostic_protocol::EVT_WAVEFORM_SELECTED,
            2,
            1,
        ),
        evidence_record(
            15,
            1_341,
            binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_ACTIVATE,
            2,
            0,
        ),
        evidence_record(
            16,
            1_380,
            binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_COMPLETE,
            2,
            0,
        ),
        evidence_record(
            17,
            1_381,
            binbook_diagnostic_protocol::EVT_BW_BASE_SYNC_START,
            2,
            0,
        ),
        evidence_record(
            18,
            1_390,
            binbook_diagnostic_protocol::EVT_BW_BASE_SYNC_COMPLETE,
            2,
            0,
        ),
        evidence_record(
            19,
            1_450,
            binbook_diagnostic_protocol::EVT_TURN_DEQUEUED,
            11,
            3,
        ),
    ]
}

#[test]
fn staged_gray_script_rejects_premature_grayscale() {
    let mut records = valid_evidence();
    records[1].tick_ms = 449;
    assert!(
        binbook_cli::exercise::validate_staged_gray_evidence(valid_status(), &records).is_err()
    );
}

#[test]
fn staged_gray_script_rejects_false_sync_markers() {
    let mut records = valid_evidence();
    records[17].arg0 = 3;
    assert!(
        binbook_cli::exercise::validate_staged_gray_evidence(valid_status(), &records).is_err()
    );
}

#[test]
fn staged_gray_script_rejects_wrong_page_order() {
    let mut records = valid_evidence();
    records[10].arg1 = 2;
    assert!(
        binbook_cli::exercise::validate_staged_gray_evidence(valid_status(), &records).is_err()
    );
}

#[test]
fn staged_gray_script_rejects_dropped_turns() {
    let mut records = valid_evidence();
    records.push(evidence_record(
        9,
        1_010,
        binbook_diagnostic_protocol::EVT_TURN_DROPPED,
        1,
        0,
    ));
    assert!(
        binbook_cli::exercise::validate_staged_gray_evidence(valid_status(), &records).is_err()
    );
}

#[test]
fn staged_gray_script_rejects_mismatched_completion_sequences() {
    let mut records = valid_evidence();
    records[11].arg0 = 99;
    assert!(
        binbook_cli::exercise::validate_staged_gray_evidence(valid_status(), &records).is_err()
    );
}

#[test]
fn staged_gray_script_rejects_missing_waveform_revision() {
    let mut records = valid_evidence();
    records[2].arg1 = 2;
    assert!(
        binbook_cli::exercise::validate_staged_gray_evidence(valid_status(), &records).is_err()
    );
}

#[test]
fn staged_gray_script_rejects_absolute_refresh_activation() {
    let mut records = valid_evidence();
    records.push(evidence_record(
        20,
        1_451,
        binbook_diagnostic_protocol::EVT_REFRESH_DECISION,
        0xC7,
        0,
    ));
    assert!(
        binbook_cli::exercise::validate_staged_gray_evidence(valid_status(), &records).is_err()
    );
}

#[test]
fn staged_gray_script_rejects_completion_for_cancelled_attempt() {
    let mut records = valid_evidence();
    records.insert(
        10,
        evidence_record(
            10,
            965,
            binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_COMPLETE,
            2,
            0,
        ),
    );
    assert!(
        binbook_cli::exercise::validate_staged_gray_evidence(valid_status(), &records).is_err()
    );
}

#[test]
#[ignore]
fn hardware_staged_gray_exercise() {
    let result = binbook_cli::exercise::run_staged_gray(PORT);
    assert!(
        result.is_ok(),
        "hardware staged-gray exercise should run: {:?}",
        result
    );
}

fn decoded_request_transcript(bytes: &[u8]) -> Vec<(Opcode, u16)> {
    let mut rest = bytes;
    let mut transcript = Vec::new();

    while let Some(frame) = find_frame(rest) {
        let mut payload = [0u8; MAX_FRAME_BYTES];
        let (header, _) = decode_frame(frame, &mut payload).unwrap();
        transcript.push((header.opcode, header.sequence));
        rest = &rest[frame.len()..];
    }

    transcript
}

fn expected_request_transcript() -> Vec<(Opcode, u16)> {
    vec![
        (Opcode::Page, 1),
        (Opcode::Page, 2),
        (Opcode::Status, 3),
        (Opcode::LogClear, 4),
        (Opcode::Key, 5),
        (Opcode::LogGet, 6),
        (Opcode::Key, 7),
        (Opcode::LogGet, 10),
        (Opcode::Key, 8),
        (Opcode::Key, 9),
        (Opcode::LogGet, 10),
        (Opcode::Key, 11),
        (Opcode::Status, 12),
        (Opcode::LogGet, 13),
    ]
}

fn staged_gray_responses() -> Vec<Vec<u8>> {
    let mut reads = Vec::new();
    reads.extend(fragment(page_response(1, 3)));
    reads.extend(fragment(page_response(2, 0)));
    reads.extend(fragment(status_response(
        3,
        StatusPayload {
            current_page: 0,
            page_count: 4,
            panel_mode: PanelModeCode::Unknown,
            dropped_log_count: 0,
            protocol_error_count: 0,
            last_error: 0,
        },
    )));
    reads.extend(fragment(log_response(Opcode::LogClear, 4, 4, 0, &[])));
    reads.extend(fragment(ack(5, Opcode::Key)));
    reads.extend(fragment(log_response(
        Opcode::LogGet,
        6,
        11,
        0,
        &[
            log_record(
                4,
                100,
                binbook_diagnostic_protocol::EVT_TURN_DEQUEUED,
                5,
                1,
                0,
            ),
            log_record(
                5,
                450,
                binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_START,
                1,
                0,
                0,
            ),
            log_record(
                6,
                460,
                binbook_diagnostic_protocol::EVT_WAVEFORM_SELECTED,
                2,
                1,
                0,
            ),
            log_record(
                7,
                461,
                binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_ACTIVATE,
                1,
                0,
                0,
            ),
            log_record(
                8,
                500,
                binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_COMPLETE,
                1,
                0,
                0,
            ),
            log_record(
                9,
                501,
                binbook_diagnostic_protocol::EVT_BW_BASE_SYNC_START,
                1,
                0,
                0,
            ),
            log_record(
                10,
                510,
                binbook_diagnostic_protocol::EVT_BW_BASE_SYNC_COMPLETE,
                1,
                0,
                0,
            ),
        ],
    )));
    reads.extend(fragment(ack(7, Opcode::Key)));
    reads.extend(fragment(log_response(
        Opcode::LogGet,
        10,
        12,
        0,
        &[log_record(
            11,
            600,
            binbook_diagnostic_protocol::EVT_TURN_DEQUEUED,
            7,
            2,
            0,
        )],
    )));
    reads.extend(fragment(ack(8, Opcode::Key)));
    reads.extend(fragment(ack(9, Opcode::Key)));
    reads.extend(fragment(log_response(
        Opcode::LogGet,
        10,
        22,
        0,
        &[
            log_record(
                12,
                950,
                binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_START,
                2,
                0,
                0,
            ),
            log_record(
                13,
                960,
                binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_CANCELLED,
                2,
                0,
                0,
            ),
            log_record(
                14,
                970,
                binbook_diagnostic_protocol::EVT_TURN_DEQUEUED,
                8,
                3,
                0,
            ),
            log_record(
                15,
                980,
                binbook_diagnostic_protocol::EVT_TURN_DEQUEUED,
                9,
                2,
                0,
            ),
            log_record(
                16,
                1_330,
                binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_START,
                2,
                0,
                0,
            ),
            log_record(
                17,
                1_340,
                binbook_diagnostic_protocol::EVT_WAVEFORM_SELECTED,
                2,
                1,
                0,
            ),
            log_record(
                18,
                1_341,
                binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_ACTIVATE,
                2,
                0,
                0,
            ),
            log_record(
                19,
                1_380,
                binbook_diagnostic_protocol::EVT_GRAY_OVERLAY_COMPLETE,
                2,
                0,
                0,
            ),
            log_record(
                20,
                1_381,
                binbook_diagnostic_protocol::EVT_BW_BASE_SYNC_START,
                2,
                0,
                0,
            ),
            log_record(
                21,
                1_390,
                binbook_diagnostic_protocol::EVT_BW_BASE_SYNC_COMPLETE,
                2,
                0,
                0,
            ),
        ],
    )));
    reads.extend(fragment(ack(11, Opcode::Key)));
    reads.extend(fragment(status_response(
        12,
        StatusPayload {
            current_page: 3,
            page_count: 4,
            panel_mode: PanelModeCode::Bw,
            dropped_log_count: 0,
            protocol_error_count: 0,
            last_error: 0,
        },
    )));
    reads.extend(fragment(log_response(
        Opcode::LogGet,
        13,
        23,
        0,
        &[log_record(
            22,
            1_450,
            binbook_diagnostic_protocol::EVT_TURN_DEQUEUED,
            11,
            3,
            0,
        )],
    )));
    reads
}

fn log_record(sequence: u32, tick_ms: u32, event: u16, arg0: i32, arg1: i32, arg2: i32) -> Vec<u8> {
    let mut buf = [0u8; 24];
    encode_log_record(
        LogRecordPayload {
            sequence,
            tick_ms,
            level: 1,
            subsystem: 3,
            event,
            arg0,
            arg1,
            arg2,
        },
        &mut buf,
    )
    .unwrap();
    buf.to_vec()
}

fn log_response(
    opcode: Opcode,
    sequence: u16,
    next_cursor: u32,
    dropped_log_count: u32,
    records: &[Vec<u8>],
) -> Vec<u8> {
    let mut payload = [0u8; 496];
    let header_len = encode_log_response_header(
        LogResponseHeader {
            next_cursor,
            dropped_log_count,
            record_count: records.len() as u16,
        },
        &mut payload,
    )
    .unwrap();
    let mut offset = header_len;
    for record in records {
        payload[offset..offset + record.len()].copy_from_slice(record);
        offset += record.len();
    }
    response_frame(opcode, sequence, Status::Ok, &payload[..offset])
}

fn page_response(sequence: u16, page: u32) -> Vec<u8> {
    let mut payload = [0u8; 8];
    let len = encode_page_response(page, &mut payload).unwrap();
    response_frame(Opcode::Page, sequence, Status::Ok, &payload[..len])
}

fn status_response(sequence: u16, payload: StatusPayload) -> Vec<u8> {
    let mut buf = [0u8; 64];
    let len = encode_status_payload(payload, &mut buf).unwrap();
    response_frame(Opcode::Status, sequence, Status::Ok, &buf[..len])
}

fn ack(sequence: u16, opcode: Opcode) -> Vec<u8> {
    response_frame(opcode, sequence, Status::Ok, &[])
}

fn response_frame(opcode: Opcode, sequence: u16, status: Status, payload: &[u8]) -> Vec<u8> {
    let header = FrameHeader {
        kind: FrameKind::Response,
        opcode,
        status,
        sequence,
        payload_len: payload.len() as u16,
    };
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, payload, &mut buf).unwrap();
    buf[..len].to_vec()
}

fn fragment(frame: Vec<u8>) -> Vec<Vec<u8>> {
    let split = (frame.len() / 2).max(1).min(frame.len().saturating_sub(1));
    vec![frame[..split].to_vec(), frame[split..].to_vec()]
}

#[test]
#[ignore]
fn hardware_byte_by_byte_status_request() {
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Status,
        status: Status::Ok,
        sequence: 41,
        payload_len: 0,
    };
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();

    let mut port = open_port();
    for &byte in &buf[..len] {
        port.write_all(&[byte]).unwrap();
        port.flush().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    let mut response = Vec::new();
    let mut rbuf = [0u8; 512];
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(2) {
        match port.read(&mut rbuf) {
            Ok(n) if n > 0 => {
                response.extend_from_slice(&rbuf[..n]);
                if response.contains(&FRAME_DELIMITER) {
                    break;
                }
            }
            _ => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
    }

    let frame = find_frame(&response).expect("should receive a complete frame");
    let mut payload = [0u8; 496];
    let (h, _) = decode_frame(frame, &mut payload).unwrap();
    assert_eq!(h.opcode, Opcode::Status);
    assert_eq!(h.sequence, 41);
    assert_eq!(h.status, Status::Ok);
}

#[test]
#[ignore]
fn hardware_two_frame_batched_request() {
    let mut combined = [0u8; MAX_FRAME_BYTES * 2];
    let mut offset = 0;

    for seq in [42u16, 43] {
        let header = FrameHeader {
            kind: FrameKind::Request,
            opcode: Opcode::Status,
            status: Status::Ok,
            sequence: seq,
            payload_len: 0,
        };
        let mut buf = [0u8; MAX_FRAME_BYTES];
        let len = encode_frame(&header, &[], &mut buf).unwrap();
        combined[offset..offset + len].copy_from_slice(&buf[..len]);
        offset += len;
    }

    let mut port = open_port();
    port.write_all(&combined[..offset]).unwrap();
    port.flush().unwrap();

    let mut response = Vec::new();
    let mut rbuf = [0u8; 512];
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(4) {
        match port.read(&mut rbuf) {
            Ok(n) if n > 0 => {
                response.extend_from_slice(&rbuf[..n]);
                if response.windows(1).any(|w| w == [FRAME_DELIMITER])
                    && response.iter().filter(|&&b| b == FRAME_DELIMITER).count() >= 2
                {
                    break;
                }
            }
            _ => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
    }

    let mut remaining = &response[..];
    let mut sequences = Vec::new();
    while let Some(frame) = find_frame(remaining) {
        let mut payload = [0u8; 496];
        if let Ok((h, _)) = decode_frame(frame, &mut payload) {
            sequences.push(h.sequence);
        }
        remaining = &remaining[frame.len()..];
    }

    assert!(sequences.contains(&42), "should contain sequence 42");
    assert!(sequences.contains(&43), "should contain sequence 43");
}

#[test]
#[ignore]
fn hardware_malformed_frame_does_not_wedge_stream() {
    let mut raw = [0u8; 16];
    raw[0..2].copy_from_slice(&MAGIC);
    raw[2] = PROTOCOL_VERSION;
    raw[3] = FrameKind::Request as u8;
    raw[4] = Opcode::Status as u8;
    raw[5] = Status::Ok as u8;
    raw[6..8].copy_from_slice(&77u16.to_le_bytes());
    raw[8..10].copy_from_slice(&0u16.to_le_bytes());
    let crc = crc16_ccitt_false(&raw[..10]);
    raw[10..12].copy_from_slice(&crc.to_le_bytes());
    raw[12] = 0xFF;
    raw[13] = FRAME_DELIMITER;

    let mut port = open_port();
    port.write_all(&raw[..14]).unwrap();
    port.flush().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Status,
        status: Status::Ok,
        sequence: 78,
        payload_len: 0,
    };
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();
    port.write_all(&buf[..len]).unwrap();
    port.flush().unwrap();

    let mut response = Vec::new();
    let mut rbuf = [0u8; 512];
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(2) {
        match port.read(&mut rbuf) {
            Ok(n) if n > 0 => {
                response.extend_from_slice(&rbuf[..n]);
                if response.contains(&FRAME_DELIMITER) {
                    break;
                }
            }
            _ => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
    }

    let frame = find_frame(&response).expect("should receive a response after malformed frame");
    let mut payload = [0u8; 496];
    let (h, _) = decode_frame(frame, &mut payload).unwrap();
    assert_eq!(h.opcode, Opcode::Status);
    assert_eq!(h.sequence, 78);
}
