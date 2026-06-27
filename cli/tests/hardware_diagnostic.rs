#![cfg(feature = "serial-device")]

use std::io::{self, Read, Write};

use binbook_diagnostic_protocol::{
    crc16_ccitt_false, decode_frame, encode_frame, encode_log_record, encode_log_response_header,
    encode_page_response, encode_status_payload, FrameHeader, FrameKind, LogRecordPayload,
    LogResponseHeader, Opcode, PanelModeCode, Status, StatusPayload, FRAME_DELIMITER, MAGIC,
    MAX_FRAME_BYTES, PROTOCOL_VERSION,
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

fn send_and_receive(port: &mut Box<dyn serialport::SerialPort>, frame: &[u8]) -> Vec<u8> {
    port.write_all(frame).unwrap();
    port.flush().unwrap();

    let mut response = Vec::new();
    let mut buf = [0u8; 512];
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(2) {
        match port.read(&mut buf) {
            Ok(n) if n > 0 => {
                response.extend_from_slice(&buf[..n]);
                if response.contains(&FRAME_DELIMITER) {
                    break;
                }
            }
            _ => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
    }
    response
}

fn find_frame(data: &[u8]) -> Option<&[u8]> {
    data.iter()
        .position(|&b| b == FRAME_DELIMITER)
        .map(|pos| &data[..=pos])
}

#[test]
fn deferred_gray_exercise_uses_the_planned_transport_script() {
    let reads = deferred_gray_responses();
    let mut io = ScriptedExerciseIo::new(reads);

    let result = binbook_cli::exercise::run_deferred_gray_io(&mut io, PORT);

    assert!(result.is_ok(), "deferred-gray exercise should run: {:?}", result);
    assert_eq!(io.index, io.reads.len(), "exercise should consume all scripted reads");
    assert_eq!(
        decoded_request_transcript(&io.written),
        expected_request_transcript(),
        "exercise should issue the planned request order",
    );
}

#[test]
#[ignore]
fn hardware_deferred_gray_exercise() {
    let result = binbook_cli::exercise::run_deferred_gray(PORT);
    assert!(result.is_ok(), "hardware deferred-gray exercise should run: {:?}", result);
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
        (Opcode::Status, 2),
        (Opcode::LogClear, 3),
        (Opcode::Key, 4),
        (Opcode::LogGet, 5),
        (Opcode::Key, 6),
        (Opcode::Key, 7),
        (Opcode::Key, 8),
        (Opcode::LogGet, 9),
        (Opcode::Key, 10),
        (Opcode::Status, 11),
        (Opcode::LogGet, 12),
    ]
}

fn deferred_gray_responses() -> Vec<Vec<u8>> {
    let mut reads = Vec::new();
    reads.extend(fragment(page_response(1, 0)));
    reads.extend(fragment(status_response(
        2,
        StatusPayload {
            current_page: 0,
            page_count: 4,
            panel_mode: PanelModeCode::Unknown,
            dropped_log_count: 0,
            protocol_error_count: 0,
            last_error: 0,
        },
    )));
    reads.extend(fragment(log_response(Opcode::LogClear, 3, 4, 0, &[])));
    reads.extend(fragment(ack(4, Opcode::Key)));
    reads.extend(fragment(log_response(
        Opcode::LogGet,
        5,
        6,
        0,
        &[log_record(4, 1, binbook_diagnostic_protocol::EVT_REFRESH_PHASE, 0, 0, 0)],
    )));
    reads.extend(fragment(ack(6, Opcode::Key)));
    reads.extend(fragment(ack(7, Opcode::Key)));
    reads.extend(fragment(ack(8, Opcode::Key)));
    reads.extend(fragment(log_response(
        Opcode::LogGet,
        9,
        10,
        0,
        &[log_record(
            5,
            2,
            binbook_diagnostic_protocol::EVT_RESEED_COMPLETE,
            2,
            0,
            0,
        )],
    )));
    reads.extend(fragment(ack(10, Opcode::Key)));
    reads.extend(fragment(status_response(
        11,
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
        12,
        13,
        0,
        &[
            log_record(6, 3, binbook_diagnostic_protocol::EVT_TURN_QUEUED, 1, 0, 0),
            log_record(7, 4, binbook_diagnostic_protocol::EVT_TURN_DEQUEUED, 1, 2, 0),
            log_record(
                8,
                5,
                binbook_diagnostic_protocol::EVT_RESEED_START,
                2,
                0,
                0,
            ),
            log_record(
                9,
                6,
                binbook_diagnostic_protocol::EVT_RESEED_COMPLETE,
                2,
                0,
                0,
            ),
        ],
    )));
    reads
}

fn log_record(
    sequence: u32,
    tick_ms: u32,
    event: u16,
    arg0: i32,
    arg1: i32,
    arg2: i32,
) -> Vec<u8> {
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
    let mut payload = [0u8; 256];
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
