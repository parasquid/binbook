#![cfg(feature = "serial-device")]

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::time::Duration;

use binbook::bulk_readback_stream::BulkReadbackTransport;
use binbook::bulk_serial::BulkSerial;
use binbook::bulk_upload::{BulkUploadTransport, CreditPollError};
use binbook_diagnostic_protocol::{
    decode_frame, decode_store_bulk_data, encode_frame, encode_store_bulk_credit,
    encode_store_bulk_finish_response, encode_store_bulk_open_response,
    encode_store_bulk_status_response, FrameHeader, FrameKind, Opcode, Status, StorageBackend,
    StoreBulkCredit, StoreBulkData, StoreBulkFinishResponse, StoreBulkOpenRequest,
    StoreBulkOpenResponse, StoreBulkState, StoreBulkStatusResponse, StoreUploadCommitResult,
    TransferDirection, MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES,
};

#[derive(Default)]
struct ScriptedIo {
    reads: VecDeque<u8>,
    writes: Vec<u8>,
}

impl Read for ScriptedIo {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let count = out.len().min(self.reads.len()).min(37);
        for item in out.iter_mut().take(count) {
            *item = self.reads.pop_front().unwrap();
        }
        Ok(count)
    }
}

impl Write for ScriptedIo {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.writes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn frame(header: FrameHeader, payload: &[u8]) -> Vec<u8> {
    let mut out = [0; MAX_FRAME_BYTES];
    let len = encode_frame(&header, payload, &mut out).unwrap();
    out[..len].to_vec()
}

fn response(opcode: Opcode, sequence: u16, payload: &[u8]) -> Vec<u8> {
    response_with_status(opcode, sequence, Status::Ok, payload)
}

fn response_with_status(opcode: Opcode, sequence: u16, status: Status, payload: &[u8]) -> Vec<u8> {
    frame(
        FrameHeader {
            kind: FrameKind::Response,
            opcode,
            status,
            sequence,
            payload_len: payload.len() as u16,
        },
        payload,
    )
}

#[test]
fn library_wait_retries_transient_list_error_after_commit() {
    let mut list_payload = [0u8; MAX_PAYLOAD_BYTES];
    list_payload[..2].copy_from_slice(&1u16.to_le_bytes());
    let entry_len = binbook_diagnostic_protocol::encode_store_entry(
        b"book.binbook",
        1_024,
        3,
        &mut list_payload[2..],
    )
    .unwrap();
    let mut reads = response_with_status(Opcode::StoreList, 1, Status::Error, &[]);
    reads.extend(response(
        Opcode::StoreList,
        2,
        &list_payload[..2 + entry_len],
    ));
    let mut io = ScriptedIo {
        reads: reads.into(),
        writes: Vec::new(),
    };
    let mut serial = BulkSerial::new(&mut io, Duration::from_millis(50));

    assert!(BulkUploadTransport::wait_library(&mut serial, "book.binbook", 1_024).unwrap());
}

#[test]
fn bulk_serial_routes_fragmented_credit_around_exact_open_status_and_finish_responses() {
    let mut payload = [0; MAX_PAYLOAD_BYTES];
    let open_len = encode_store_bulk_open_response(
        &StoreBulkOpenResponse {
            stream_id: 7,
            selected_chunk: 512,
            initial_credits: 2,
            direction: TransferDirection::Upload,
            durable_offset: 0,
            total_size: 1_024,
        },
        &mut payload,
    )
    .unwrap();
    let credit_len = encode_store_bulk_credit(
        &StoreBulkCredit {
            stream_id: 7,
            durable_offset: 512,
            credits: 1,
            status: Status::Ok,
        },
        &mut payload[open_len..],
    )
    .unwrap();
    let credit = frame(
        FrameHeader {
            kind: FrameKind::Event,
            opcode: Opcode::StoreBulkCredit,
            status: Status::Ok,
            sequence: 0,
            payload_len: credit_len as u16,
        },
        &payload[open_len..open_len + credit_len],
    );
    let status_len = encode_store_bulk_status_response(
        &StoreBulkStatusResponse {
            stream_id: 7,
            direction: TransferDirection::Upload,
            state: StoreBulkState::Receiving,
            available_credits: 2,
            durable_offset: 512,
            total_size: 1_024,
            running_crc32: 1,
        },
        &mut payload[open_len..],
    )
    .unwrap();
    let status_payload = payload[open_len..open_len + status_len].to_vec();
    let finish_len = encode_store_bulk_finish_response(
        &StoreBulkFinishResponse {
            stream_id: 7,
            result: StoreUploadCommitResult::Ok,
            total_bytes: 1_024,
            computed_crc32: 0x1234,
        },
        &mut payload[open_len..],
    )
    .unwrap();
    let finish_payload = payload[open_len..open_len + finish_len].to_vec();
    let mut reads = response(Opcode::StoreBulkOpen, 1, &payload[..open_len]);
    reads.extend(credit);
    reads.extend(response(Opcode::StoreBulkStatus, 2, &status_payload));
    reads.extend(response(Opcode::StoreBulkFinish, 3, &finish_payload));
    let mut io = ScriptedIo {
        reads: reads.into(),
        writes: Vec::new(),
    };
    let mut serial = BulkSerial::new(&mut io, Duration::from_millis(50));

    let opened = BulkUploadTransport::open(
        &mut serial,
        StoreBulkOpenRequest {
            direction: TransferDirection::Upload,
            backend: StorageBackend::Sd,
            requested_window: 2,
            requested_chunk: 512,
            name: "book.binbook",
            total_size: 1_024,
            expected_crc32: 0x1234,
        },
    )
    .unwrap();
    assert_eq!(opened.stream_id, 7);
    assert_eq!(serial.poll_credit().unwrap().durable_offset, 512);
    assert_eq!(
        BulkUploadTransport::status(&mut serial, 7)
            .unwrap()
            .durable_offset,
        512
    );
    assert_eq!(
        BulkUploadTransport::finish(&mut serial, 7)
            .unwrap()
            .computed_crc32,
        0x1234
    );
    assert_eq!(serial.recovery_metrics(), (1, 1));
}

#[test]
fn bulk_serial_emits_data_as_strict_zero_sequence_event() {
    let mut io = ScriptedIo::default();
    let mut serial = BulkSerial::new(&mut io, Duration::from_millis(1));
    serial
        .send_data(StoreBulkData {
            stream_id: 7,
            offset: 512,
            end_of_data: true,
            data: b"last",
        })
        .unwrap();
    drop(serial);

    let mut payload = [0; MAX_PAYLOAD_BYTES];
    let (header, len) = decode_frame(&io.writes, &mut payload).unwrap();
    assert_eq!(header.kind, FrameKind::Event);
    assert_eq!(header.opcode, Opcode::StoreBulkData);
    assert_eq!(header.sequence, 0);
    assert_eq!(decode_store_bulk_data(&payload[..len]).unwrap().offset, 512);
}

#[test]
fn bulk_serial_credit_poll_times_out_without_manufacturing_progress() {
    let mut io = ScriptedIo::default();
    let mut serial = BulkSerial::new(&mut io, Duration::from_millis(1));
    assert!(matches!(
        serial.poll_credit(),
        Err(CreditPollError::Timeout)
    ));
}

#[test]
fn bulk_serial_routes_readback_data_event_and_emits_durable_credit_event() {
    let mut payload = [0; MAX_PAYLOAD_BYTES];
    let data_len = binbook_diagnostic_protocol::encode_store_bulk_data(
        &StoreBulkData {
            stream_id: 9,
            offset: 0,
            end_of_data: true,
            data: b"book",
        },
        &mut payload,
    )
    .unwrap();
    let reads = frame(
        FrameHeader {
            kind: FrameKind::Event,
            opcode: Opcode::StoreBulkData,
            status: Status::Ok,
            sequence: 0,
            payload_len: data_len as u16,
        },
        &payload[..data_len],
    );
    let mut io = ScriptedIo {
        reads: reads.into(),
        writes: Vec::new(),
    };
    let mut serial = BulkSerial::new(&mut io, Duration::from_millis(50));
    let data = BulkReadbackTransport::poll_data(&mut serial).unwrap();
    assert_eq!(data.data, b"book");
    BulkReadbackTransport::send_credit(
        &mut serial,
        StoreBulkCredit {
            stream_id: 9,
            durable_offset: 4,
            credits: 1,
            status: Status::Ok,
        },
    )
    .unwrap();
    drop(serial);

    let mut decoded = [0; MAX_PAYLOAD_BYTES];
    let (header, len) = decode_frame(&io.writes, &mut decoded).unwrap();
    assert_eq!(header.kind, FrameKind::Event);
    assert_eq!(header.opcode, Opcode::StoreBulkCredit);
    assert_eq!(header.sequence, 0);
    assert_eq!(
        binbook_diagnostic_protocol::decode_store_bulk_credit(&decoded[..len])
            .unwrap()
            .durable_offset,
        4
    );
}
