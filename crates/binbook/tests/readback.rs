#![cfg(feature = "serial-device")]

use binbook::storage_readback::{read_stream, ReadTransport, ReadbackError};
use binbook_diagnostic_protocol::{
    decode_frame, decode_store_read_request, encode_frame, encode_store_read_response, FrameHeader,
    FrameKind, Opcode, Status, MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES,
};

struct FakeReadPeer {
    data: Vec<u8>,
    window: usize,
    batch_sizes: Vec<usize>,
}

impl ReadTransport for FakeReadPeer {
    fn read_window(&mut self) -> Result<usize, ReadbackError> {
        Ok(self.window)
    }

    fn exchange_batch(
        &mut self,
        requests: &[Vec<u8>],
        sequences: &[u16],
    ) -> Result<Vec<Vec<u8>>, ReadbackError> {
        self.batch_sizes.push(requests.len());
        requests
            .iter()
            .zip(sequences)
            .map(|(frame, sequence)| self.response(frame, *sequence))
            .collect()
    }
}

impl FakeReadPeer {
    fn response(&self, frame: &[u8], sequence: u16) -> Result<Vec<u8>, ReadbackError> {
        let mut request_payload = [0u8; MAX_PAYLOAD_BYTES];
        let (_, payload_len) = decode_frame(frame, &mut request_payload).unwrap();
        let request = decode_store_read_request(&request_payload[..payload_len]).unwrap();
        let offset = request.offset as usize;
        if offset > self.data.len() {
            return Ok(response_frame(sequence, Status::BadRequest, &[]));
        }
        let end = (offset + request.max_bytes as usize).min(self.data.len());
        let mut response_payload = [0u8; MAX_PAYLOAD_BYTES];
        let response_len = encode_store_read_response(
            &self.data[offset..end],
            end == self.data.len(),
            &mut response_payload,
        )
        .unwrap();
        Ok(response_frame(
            sequence,
            Status::Ok,
            &response_payload[..response_len],
        ))
    }
}

fn response_frame(sequence: u16, status: Status, payload: &[u8]) -> Vec<u8> {
    let header = FrameHeader {
        kind: FrameKind::Response,
        opcode: Opcode::StoreRead,
        status,
        sequence,
        payload_len: payload.len() as u16,
    };
    let mut frame = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, payload, &mut frame).unwrap();
    frame[..len].to_vec()
}

#[test]
fn negotiated_window_streams_two_reads_per_batch_without_whole_file_buffer() {
    let expected = (0..10_000).map(|value| value as u8).collect::<Vec<_>>();
    let mut peer = FakeReadPeer {
        data: expected.clone(),
        window: 2,
        batch_sizes: Vec::new(),
    };
    let mut output = Vec::new();

    let bytes = read_stream("book.binbook", &mut peer, &mut output).unwrap();

    assert_eq!(bytes, 10_000);
    assert_eq!(output, expected);
    assert_eq!(peer.batch_sizes, [2, 2]);
}

#[test]
fn legacy_peer_reads_one_frame_at_a_time() {
    let expected = vec![0x5a; 10_000];
    let mut peer = FakeReadPeer {
        data: expected.clone(),
        window: 1,
        batch_sizes: Vec::new(),
    };
    let mut output = Vec::new();

    read_stream("book.binbook", &mut peer, &mut output).unwrap();

    assert_eq!(output, expected);
    assert_eq!(peer.batch_sizes, [1, 1, 1]);
}

#[test]
fn eof_in_first_response_ignores_speculative_second_range_error() {
    let expected = vec![0x33; 100];
    let mut peer = FakeReadPeer {
        data: expected.clone(),
        window: 2,
        batch_sizes: Vec::new(),
    };
    let mut output = Vec::new();

    read_stream("book.binbook", &mut peer, &mut output).unwrap();

    assert_eq!(output, expected);
    assert_eq!(peer.batch_sizes, [2]);
}
