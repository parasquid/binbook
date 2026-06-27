#![cfg(feature = "serial-device")]

use binbook_diagnostic_protocol::{
    encode_frame, decode_frame, crc16_ccitt_false, FrameHeader, FrameKind, Opcode, Status,
    FRAME_DELIMITER, MAX_FRAME_BYTES, MAGIC, PROTOCOL_VERSION,
};

const PORT: &str = "/dev/ttyACM0";

fn open_port() -> Box<dyn serialport::SerialPort> {
    serialport::new(PORT, 115200)
        .timeout(std::time::Duration::from_secs(2))
        .open()
        .unwrap_or_else(|e| panic!("failed to open {}: {}", PORT, e))
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
