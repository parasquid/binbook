#![cfg(feature = "serial-device")]

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use binbook::{
    bulk_serial::BulkSerial, bulk_upload::BulkUploadTransport, diag_protocol,
    serial_transport::SerialSession,
};
use binbook_diagnostic_protocol::{
    decode_frame, encode_frame, encode_store_bulk_data, FrameHeader, FrameKind, Opcode, Status,
    StorageBackend, StoreBulkData, StoreBulkOpenRequest, StoreBulkState, StoreUploadCommitResult,
    TransferDirection, FRAME_DELIMITER, MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES,
};

const TIMEOUT: Duration = Duration::from_secs(30);

fn crc32(bytes: &[u8]) -> u32 {
    let mut state = u32::MAX;
    for byte in bytes {
        state ^= u32::from(*byte);
        for _ in 0..8 {
            state = if state & 1 == 0 {
                state >> 1
            } else {
                (state >> 1) ^ 0xedb8_8320
            };
        }
    }
    !state
}

fn open_upload(
    serial: &mut SerialSession,
    name: &str,
    total_size: u32,
    expected_crc32: u32,
) -> binbook_diagnostic_protocol::StoreBulkOpenResponse {
    let mut bulk = BulkSerial::new(serial, TIMEOUT);
    let opened = bulk
        .open(StoreBulkOpenRequest {
            direction: TransferDirection::Upload,
            backend: StorageBackend::Sd,
            requested_window: 1,
            requested_chunk: 512,
            name,
            total_size,
            expected_crc32,
        })
        .unwrap();
    assert_eq!(opened.direction, TransferDirection::Upload);
    assert_eq!(opened.selected_chunk, 512);
    assert_eq!(opened.initial_credits, 1);
    assert_eq!(opened.durable_offset, 0);
    opened
}

fn send_fault_data_and_expect_bad_request(serial: &mut SerialSession, data: StoreBulkData<'_>) {
    let mut payload = [0; MAX_PAYLOAD_BYTES];
    let payload_len = encode_store_bulk_data(&data, &mut payload).unwrap();
    let mut encoded = [0; MAX_FRAME_BYTES];
    let encoded_len = encode_frame(
        &FrameHeader {
            kind: FrameKind::Event,
            opcode: Opcode::StoreBulkData,
            status: Status::Ok,
            sequence: 0,
            payload_len: payload_len as u16,
        },
        &payload[..payload_len],
        &mut encoded,
    )
    .unwrap();
    serial.write_all(&encoded[..encoded_len]).unwrap();

    let deadline = Instant::now() + TIMEOUT;
    let mut buffered = Vec::new();
    let mut read = [0; 256];
    while Instant::now() < deadline {
        match serial.read(&mut read) {
            Ok(0) => {}
            Ok(len) => buffered.extend_from_slice(&read[..len]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => panic!("bulk fault response read failed: {error}"),
        }
        while let Some(end) = buffered.iter().position(|byte| *byte == FRAME_DELIMITER) {
            let frame: Vec<_> = buffered.drain(..=end).collect();
            let mut decoded = [0; MAX_PAYLOAD_BYTES];
            let Ok((header, payload_len)) = decode_frame(&frame, &mut decoded) else {
                continue;
            };
            if header.opcode == Opcode::StoreBulkData
                && header.kind == FrameKind::Response
                && header.sequence == 0
            {
                assert_eq!(header.status, Status::BadRequest);
                assert_eq!(payload_len, 0);
                return;
            }
        }
    }
    panic!("timed out waiting for explicit StoreBulkData BadRequest");
}

fn assert_not_found(serial: &mut SerialSession, sequence: u16, name: &str) {
    let error = serial
        .send_and_receive(
            &diag_protocol::store_read_request_range(sequence, name, 0, 1),
            Opcode::StoreRead,
            sequence,
            TIMEOUT,
        )
        .unwrap_err();
    assert!(
        error.contains("NotFound"),
        "unexpected read result: {error}"
    );
}

#[test]
#[ignore = "requires exclusive X4 serial access and bulk diagnostic firmware"]
fn hardware_bulk_upload_fault_matrix_recovers_without_publishing() {
    let port = std::env::var("BINBOOK_TEST_PORT").unwrap_or_else(|_| "/dev/ttyACM0".into());
    let mut serial = SerialSession::open(&port).unwrap();
    let block = [0x5a; 512];

    let wrong_stream = open_upload(&mut serial, "bulk-wrong-stream.binbook", 512, crc32(&block));
    send_fault_data_and_expect_bad_request(
        &mut serial,
        StoreBulkData {
            stream_id: wrong_stream.stream_id.wrapping_add(1),
            offset: 0,
            end_of_data: true,
            data: &block,
        },
    );
    let mut bulk = BulkSerial::new(&mut serial, TIMEOUT);
    let status = bulk.status(wrong_stream.stream_id).unwrap();
    assert_eq!(status.state, StoreBulkState::Receiving);
    assert_eq!(status.durable_offset, 0);
    assert!(bulk.abort(wrong_stream.stream_id));
    drop(bulk);
    assert_not_found(&mut serial, 101, "bulk-wrong-stream.binbook");

    let wrong_offset = open_upload(&mut serial, "bulk-wrong-offset.binbook", 512, crc32(&block));
    send_fault_data_and_expect_bad_request(
        &mut serial,
        StoreBulkData {
            stream_id: wrong_offset.stream_id,
            offset: 512,
            end_of_data: true,
            data: &block,
        },
    );
    let mut bulk = BulkSerial::new(&mut serial, TIMEOUT);
    assert_eq!(
        bulk.status(wrong_offset.stream_id).unwrap().durable_offset,
        0
    );
    assert!(bulk.abort(wrong_offset.stream_id));
    drop(bulk);
    assert_not_found(&mut serial, 102, "bulk-wrong-offset.binbook");

    let duplicate = open_upload(
        &mut serial,
        "bulk-duplicate.binbook",
        1024,
        crc32(&[block, block].concat()),
    );
    let mut bulk = BulkSerial::new(&mut serial, TIMEOUT);
    bulk.send_data(StoreBulkData {
        stream_id: duplicate.stream_id,
        offset: 0,
        end_of_data: false,
        data: &block,
    })
    .unwrap();
    let credit = bulk.poll_credit().unwrap();
    assert_eq!(credit.durable_offset, 512);
    drop(bulk);
    send_fault_data_and_expect_bad_request(
        &mut serial,
        StoreBulkData {
            stream_id: duplicate.stream_id,
            offset: 0,
            end_of_data: false,
            data: &block,
        },
    );
    let mut bulk = BulkSerial::new(&mut serial, TIMEOUT);
    assert_eq!(
        bulk.status(duplicate.stream_id).unwrap().durable_offset,
        512
    );
    assert!(bulk.abort(duplicate.stream_id));
    drop(bulk);
    assert_not_found(&mut serial, 103, "bulk-duplicate.binbook");

    let dropped_credit = open_upload(
        &mut serial,
        "bulk-dropped-credit.binbook",
        1024,
        crc32(&[block, block].concat()),
    );
    let mut bulk = BulkSerial::new(&mut serial, TIMEOUT);
    bulk.send_data(StoreBulkData {
        stream_id: dropped_credit.stream_id,
        offset: 0,
        end_of_data: false,
        data: &block,
    })
    .unwrap();
    let status = bulk.status(dropped_credit.stream_id).unwrap();
    assert_eq!(status.durable_offset, 512);
    assert_eq!(status.available_credits, 1);
    assert!(bulk.abort(dropped_credit.stream_id));
    drop(bulk);
    assert_not_found(&mut serial, 104, "bulk-dropped-credit.binbook");

    let premature = open_upload(&mut serial, "bulk-premature.binbook", 512, crc32(&block));
    let mut bulk = BulkSerial::new(&mut serial, TIMEOUT);
    assert!(bulk.finish(premature.stream_id).is_err());
    assert!(bulk.abort(premature.stream_id));
    drop(bulk);
    assert_not_found(&mut serial, 105, "bulk-premature.binbook");

    let bad_crc = open_upload(&mut serial, "bulk-bad-crc.binbook", 512, 0);
    let mut bulk = BulkSerial::new(&mut serial, TIMEOUT);
    bulk.send_data(StoreBulkData {
        stream_id: bad_crc.stream_id,
        offset: 0,
        end_of_data: true,
        data: &block,
    })
    .unwrap();
    assert_eq!(bulk.poll_credit().unwrap().durable_offset, 512);
    let finish = bulk.finish(bad_crc.stream_id).unwrap();
    assert_eq!(finish.result, StoreUploadCommitResult::CrcMismatch);
    assert_eq!(finish.total_bytes, 512);
    assert_eq!(finish.computed_crc32, crc32(&block));
    drop(bulk);
    assert_not_found(&mut serial, 106, "bulk-bad-crc.binbook");
}

#[test]
#[ignore = "requires exclusive X4 serial access, bulk firmware, and 65 seconds"]
fn hardware_bulk_idle_timeout_cleans_unpublished_upload() {
    let port = std::env::var("BINBOOK_TEST_PORT").unwrap_or_else(|_| "/dev/ttyACM0".into());
    let mut serial = SerialSession::open(&port).unwrap();
    let opened = open_upload(&mut serial, "bulk-idle-timeout.binbook", 512, 1);
    std::thread::sleep(Duration::from_secs(65));

    let mut bulk = BulkSerial::new(&mut serial, TIMEOUT);
    assert!(bulk.status(opened.stream_id).is_err());
    drop(bulk);
    assert_not_found(&mut serial, 107, "bulk-idle-timeout.binbook");
}
