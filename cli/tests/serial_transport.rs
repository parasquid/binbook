#![cfg(feature = "serial-device")]

use std::io::{self, Read, Write};
use std::time::Duration;

use binbook_diagnostic_protocol::{encode_frame, FrameHeader, FrameKind, Opcode, Status, MAX_FRAME_BYTES};
use binbook_cli::serial_transport::send_and_receive_io;

struct FakeIo {
    reads: Vec<Vec<u8>>,
    index: usize,
    written: Vec<u8>,
}

impl Read for FakeIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.index >= self.reads.len() { return Ok(0); }
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
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

fn response(opcode: Opcode, sequence: u16, status: Status) -> Vec<u8> {
    let header = FrameHeader { kind: FrameKind::Response, opcode, status, sequence, payload_len: 0 };
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();
    buf[..len].to_vec()
}

#[test]
fn serial_transport_reassembles_fragmented_cobs_frame() {
    let frame = response(Opcode::Status, 41, Status::Ok);
    let split = frame.len() / 2;
    let mut io = FakeIo { reads: vec![frame[..split].to_vec(), frame[split..].to_vec()], index: 0, written: vec![] };
    let received = send_and_receive_io(&mut io, b"request", Opcode::Status, 41, Duration::from_millis(20)).unwrap();
    assert_eq!(received, frame);
}

#[test]
fn serial_transport_skips_mismatched_sequence() {
    let unrelated = response(Opcode::Status, 40, Status::Ok);
    let expected = response(Opcode::Status, 41, Status::Ok);
    let mut both = unrelated;
    both.extend_from_slice(&expected);
    let mut io = FakeIo { reads: vec![both], index: 0, written: vec![] };
    assert_eq!(send_and_receive_io(&mut io, b"request", Opcode::Status, 41, Duration::from_millis(20)).unwrap(), expected);
}

#[test]
fn serial_transport_rejects_non_ok_status() {
    let mut io = FakeIo { reads: vec![response(Opcode::Status, 41, Status::InternalError)], index: 0, written: vec![] };
    let error = send_and_receive_io(&mut io, b"request", Opcode::Status, 41, Duration::from_millis(20)).unwrap_err();
    assert!(error.contains("InternalError"));
}

#[test]
fn serial_transport_times_out_without_matching_response() {
    let mut io = FakeIo { reads: vec![response(Opcode::Status, 40, Status::Ok)], index: 0, written: vec![] };
    let error = send_and_receive_io(&mut io, b"request", Opcode::Status, 41, Duration::from_millis(2)).unwrap_err();
    assert!(error.contains("timeout"));
}
