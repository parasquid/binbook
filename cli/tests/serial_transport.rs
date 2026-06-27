#![cfg(feature = "serial-device")]

use std::io::{self, Read, Write};
use std::time::Duration;

use binbook_cli::serial_transport::{send_and_receive_io, send_batch_and_receive_io};
use binbook_diagnostic_protocol::{
    decode_frame, encode_frame, FrameHeader, FrameKind, KeyCode, Opcode, Status, MAX_FRAME_BYTES,
};

struct FakeIo {
    reads: Vec<Vec<u8>>,
    index: usize,
    written: Vec<u8>,
}

impl Read for FakeIo {
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

impl Write for FakeIo {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.written.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn response(opcode: Opcode, sequence: u16, status: Status) -> Vec<u8> {
    let header = FrameHeader {
        kind: FrameKind::Response,
        opcode,
        status,
        sequence,
        payload_len: 0,
    };
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();
    buf[..len].to_vec()
}

fn key_batch(requests: &[(u16, KeyCode)]) -> Vec<u8> {
    let mut batch = Vec::new();
    for &(sequence, key) in requests {
        batch.extend_from_slice(&binbook_cli::diag_protocol::key_request(sequence, key));
    }
    batch
}

fn decoded_sequences(frames: &[Vec<u8>]) -> Vec<u16> {
    frames
        .iter()
        .map(|frame| {
            let mut payload = [0u8; MAX_FRAME_BYTES];
            let (header, _) = decode_frame(frame, &mut payload).unwrap();
            header.sequence
        })
        .collect()
}

fn responses_for(sequences: &[u16]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    frames.push(response(Opcode::Status, 9, Status::Ok));
    for &sequence in sequences {
        frames.push(response(Opcode::Key, sequence, Status::Ok));
    }

    let mut fragments = Vec::new();
    for frame in frames {
        let split = (frame.len() / 2).max(1).min(frame.len().saturating_sub(1));
        fragments.push(frame[..split].to_vec());
        fragments.push(frame[split..].to_vec());
    }
    fragments
}

#[test]
fn serial_transport_reassembles_fragmented_cobs_frame() {
    let frame = response(Opcode::Status, 41, Status::Ok);
    let split = frame.len() / 2;
    let mut io = FakeIo {
        reads: vec![frame[..split].to_vec(), frame[split..].to_vec()],
        index: 0,
        written: vec![],
    };
    let received = send_and_receive_io(
        &mut io,
        b"request",
        Opcode::Status,
        41,
        Duration::from_millis(20),
    )
    .unwrap();
    assert_eq!(received, frame);
}

#[test]
fn serial_transport_skips_mismatched_sequence() {
    let unrelated = response(Opcode::Status, 40, Status::Ok);
    let expected = response(Opcode::Status, 41, Status::Ok);
    let mut both = unrelated;
    both.extend_from_slice(&expected);
    let mut io = FakeIo {
        reads: vec![both],
        index: 0,
        written: vec![],
    };
    assert_eq!(
        send_and_receive_io(
            &mut io,
            b"request",
            Opcode::Status,
            41,
            Duration::from_millis(20)
        )
        .unwrap(),
        expected
    );
}

#[test]
fn serial_transport_rejects_non_ok_status() {
    let mut io = FakeIo {
        reads: vec![response(Opcode::Status, 41, Status::InternalError)],
        index: 0,
        written: vec![],
    };
    let error = send_and_receive_io(
        &mut io,
        b"request",
        Opcode::Status,
        41,
        Duration::from_millis(20),
    )
    .unwrap_err();
    assert!(error.contains("InternalError"));
}

#[test]
fn serial_transport_times_out_without_matching_response() {
    let mut io = FakeIo {
        reads: vec![response(Opcode::Status, 40, Status::Ok)],
        index: 0,
        written: vec![],
    };
    let error = send_and_receive_io(
        &mut io,
        b"request",
        Opcode::Status,
        41,
        Duration::from_millis(2),
    )
    .unwrap_err();
    assert!(error.contains("timeout"));
}

#[test]
fn batch_transport_collects_every_sequence_checked_response() {
    let requests = key_batch(&[(10, KeyCode::Right), (11, KeyCode::Right), (12, KeyCode::Left)]);
    let mut io = FakeIo {
        reads: responses_for(&[10, 11, 12]),
        index: 0,
        written: vec![],
    };

    let responses = send_batch_and_receive_io(
        &mut io,
        &requests,
        Opcode::Key,
        &[10, 11, 12],
        Duration::from_secs(5),
    )
    .unwrap();

    assert_eq!(io.written, requests);
    assert_eq!(decoded_sequences(&responses), [10, 11, 12]);
}

#[test]
fn batch_transport_rejects_wrong_opcode_for_matching_sequence() {
    let requests = key_batch(&[(10, KeyCode::Right)]);
    let mut io = FakeIo {
        reads: vec![response(Opcode::Page, 10, Status::Ok)],
        index: 0,
        written: vec![],
    };

    let error = send_batch_and_receive_io(
        &mut io,
        &requests,
        Opcode::Key,
        &[10],
        Duration::from_secs(2),
    )
    .unwrap_err();

    assert!(error.contains("opcode"));
}

#[test]
fn batch_transport_rejects_duplicate_sequence() {
    let requests = key_batch(&[(10, KeyCode::Right), (11, KeyCode::Left)]);
    let mut io = FakeIo {
        reads: vec![
            response(Opcode::Key, 10, Status::Ok),
            response(Opcode::Key, 10, Status::Ok),
            response(Opcode::Key, 11, Status::Ok),
        ],
        index: 0,
        written: vec![],
    };

    let error = send_batch_and_receive_io(
        &mut io,
        &requests,
        Opcode::Key,
        &[10, 11],
        Duration::from_secs(2),
    )
    .unwrap_err();

    assert!(error.contains("duplicate"));
}

#[test]
fn batch_transport_rejects_missing_sequence() {
    let requests = key_batch(&[(10, KeyCode::Right), (11, KeyCode::Left)]);
    let mut io = FakeIo {
        reads: vec![response(Opcode::Key, 10, Status::Ok)],
        index: 0,
        written: vec![],
    };

    let error = send_batch_and_receive_io(
        &mut io,
        &requests,
        Opcode::Key,
        &[10, 11],
        Duration::from_millis(20),
    )
    .unwrap_err();

    assert!(error.contains("missing"));
}

#[test]
fn batch_transport_rejects_non_ok_status() {
    let requests = key_batch(&[(10, KeyCode::Right)]);
    let mut io = FakeIo {
        reads: vec![response(Opcode::Key, 10, Status::InternalError)],
        index: 0,
        written: vec![],
    };

    let error = send_batch_and_receive_io(
        &mut io,
        &requests,
        Opcode::Key,
        &[10],
        Duration::from_secs(2),
    )
    .unwrap_err();

    assert!(error.contains("InternalError"));
}

#[test]
fn batch_transport_times_out_without_all_sequences() {
    let requests = key_batch(&[(10, KeyCode::Right), (11, KeyCode::Left)]);
    let mut io = FakeIo {
        reads: vec![response(Opcode::Key, 10, Status::Ok)],
        index: 0,
        written: vec![],
    };

    let error = send_batch_and_receive_io(
        &mut io,
        &requests,
        Opcode::Key,
        &[10, 11],
        Duration::from_millis(20),
    )
    .unwrap_err();

    assert!(error.contains("timeout"));
}
