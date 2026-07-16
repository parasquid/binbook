use binbook_diagnostic_protocol::*;

#[test]
fn bulk_open_request_has_exact_layout_and_validation() {
    let upload = StoreBulkOpenRequest {
        direction: TransferDirection::Upload,
        backend: StorageBackend::Sd,
        requested_window: 4,
        requested_chunk: 3584,
        name: "book.binbook",
        total_size: 0x4433_2211,
        expected_crc32: 0x8877_6655,
    };
    let mut out = [0xa5; 64];
    let len = encode_store_bulk_open_request(&upload, &mut out).unwrap();
    assert_eq!(len, 28);
    assert_eq!(
        &out[..16],
        &[0, 0, 4, 0, 0, 14, 12, 0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]
    );
    assert_eq!(&out[16..len], b"book.binbook");
    assert_eq!(decode_store_bulk_open_request(&out[..len]), Ok(upload));

    for (offset, invalid) in [(0, 2), (1, 2), (2, 3), (3, 1), (6, 0), (7, 1)] {
        let mut malformed = out;
        malformed[offset] = invalid;
        assert!(
            decode_store_bulk_open_request(&malformed[..len]).is_err(),
            "offset {offset}"
        );
    }
    let readback = StoreBulkOpenRequest {
        direction: TransferDirection::Readback,
        backend: StorageBackend::Sd,
        requested_window: 0,
        requested_chunk: 0,
        name: "book.binbook",
        total_size: 0,
        expected_crc32: 0,
    };
    assert!(encode_store_bulk_open_request(&readback, &mut out).is_ok());
    assert_eq!(
        encode_store_bulk_open_request(
            &StoreBulkOpenRequest {
                total_size: 1,
                ..readback
            },
            &mut out
        ),
        Err(ProtocolError::InvalidValue)
    );
}

#[test]
fn bulk_open_response_has_exact_layout_and_validation() {
    let value = StoreBulkOpenResponse {
        stream_id: 0x4433_2211,
        selected_chunk: 3584,
        initial_credits: 4,
        direction: TransferDirection::Upload,
        durable_offset: 1024,
        total_size: 8192,
    };
    let expected = [0x11, 0x22, 0x33, 0x44, 0, 14, 4, 0, 0, 4, 0, 0, 0, 32, 0, 0];
    let mut out = [0; STORE_BULK_OPEN_RESPONSE_BYTES];
    assert_eq!(encode_store_bulk_open_response(&value, &mut out), Ok(16));
    assert_eq!(out, expected);
    assert_eq!(decode_store_bulk_open_response(&out), Ok(value));
    for (offset, invalid) in [(0, 0), (4, 1), (6, 0), (7, 2)] {
        let mut malformed = out;
        malformed[offset] = invalid;
        if offset == 0 {
            malformed[1..4].fill(0);
        }
        assert!(
            decode_store_bulk_open_response(&malformed).is_err(),
            "offset {offset}"
        );
    }
}

#[test]
fn bulk_data_has_exact_layout_and_borrows_payload() {
    let value = StoreBulkData {
        stream_id: 0x4433_2211,
        offset: 0x8877_6655,
        end_of_data: true,
        data: b"payload",
    };
    let mut out = [0; 32];
    let len = encode_store_bulk_data(&value, &mut out).unwrap();
    assert_eq!(len, STORE_BULK_DATA_HEADER_BYTES + 7);
    assert_eq!(
        &out[..12],
        &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 7, 0, 1, 0]
    );
    assert_eq!(&out[12..len], b"payload");
    assert_eq!(decode_store_bulk_data(&out[..len]), Ok(value));

    for (offset, invalid) in [(10, 2), (11, 1)] {
        let mut malformed = out;
        malformed[offset] = invalid;
        assert!(decode_store_bulk_data(&malformed[..len]).is_err());
    }
    assert_eq!(
        decode_store_bulk_data(&[0; STORE_BULK_DATA_HEADER_BYTES]),
        Err(ProtocolError::InvalidValue)
    );
}

#[test]
fn bulk_credit_has_exact_layout_and_validation() {
    let value = StoreBulkCredit {
        stream_id: 1,
        durable_offset: 3584,
        credits: 2,
        status: Status::Ok,
    };
    let expected = [1, 0, 0, 0, 0, 14, 0, 0, 2, 0, 0, 0];
    let mut out = [0; STORE_BULK_CREDIT_BYTES];
    assert_eq!(encode_store_bulk_credit(&value, &mut out), Ok(12));
    assert_eq!(out, expected);
    assert_eq!(decode_store_bulk_credit(&out), Ok(value));
    for (offset, invalid) in [(0, 0), (9, 9), (10, 1), (11, 1)] {
        let mut malformed = out;
        malformed[offset] = invalid;
        assert!(
            decode_store_bulk_credit(&malformed).is_err(),
            "offset {offset}"
        );
    }
}

#[test]
fn bulk_status_request_and_response_are_exact() {
    let mut request = [0; STORE_BULK_ID_BYTES];
    assert_eq!(encode_store_bulk_status_request(7, &mut request), Ok(4));
    assert_eq!(decode_store_bulk_status_request(&request), Ok(7));

    let value = StoreBulkStatusResponse {
        stream_id: 7,
        direction: TransferDirection::Readback,
        state: StoreBulkState::Finishing,
        available_credits: 4,
        durable_offset: 3584,
        total_size: 4096,
        running_crc32: 0x4433_2211,
    };
    let expected = [
        7, 0, 0, 0, 1, 1, 4, 0, 0, 14, 0, 0, 0, 16, 0, 0, 0x11, 0x22, 0x33, 0x44,
    ];
    let mut out = [0; STORE_BULK_STATUS_RESPONSE_BYTES];
    assert_eq!(encode_store_bulk_status_response(&value, &mut out), Ok(20));
    assert_eq!(out, expected);
    assert_eq!(decode_store_bulk_status_response(&out), Ok(value));
    for (offset, invalid) in [(0, 0), (4, 2), (5, 4), (6, 0), (7, 1)] {
        let mut malformed = out;
        malformed[offset] = invalid;
        assert!(
            decode_store_bulk_status_response(&malformed).is_err(),
            "offset {offset}"
        );
    }
}

#[test]
fn bulk_finish_and_abort_are_exact() {
    let mut id = [0; STORE_BULK_ID_BYTES];
    assert_eq!(encode_store_bulk_finish_request(9, &mut id), Ok(4));
    assert_eq!(decode_store_bulk_finish_request(&id), Ok(9));
    assert_eq!(encode_store_bulk_abort_request(9, &mut id), Ok(4));
    assert_eq!(decode_store_bulk_abort_request(&id), Ok(9));

    let value = StoreBulkFinishResponse {
        stream_id: 9,
        result: StoreUploadCommitResult::Ok,
        total_bytes: 3584,
        computed_crc32: 0x4433_2211,
    };
    let expected = [9, 0, 0, 0, 0, 0, 0, 0, 0, 14, 0, 0, 0x11, 0x22, 0x33, 0x44];
    let mut out = [0; STORE_BULK_FINISH_RESPONSE_BYTES];
    assert_eq!(encode_store_bulk_finish_response(&value, &mut out), Ok(16));
    assert_eq!(out, expected);
    assert_eq!(decode_store_bulk_finish_response(&out), Ok(value));
    for offset in [0, 5, 6, 7] {
        let mut malformed = out;
        malformed[offset] = if offset == 0 { 0 } else { 1 };
        if offset == 0 {
            malformed[1..4].fill(0);
        }
        assert!(
            decode_store_bulk_finish_response(&malformed).is_err(),
            "offset {offset}"
        );
    }
}

#[test]
fn every_bulk_codec_rejects_short_or_trailing_payloads() {
    assert_eq!(
        decode_store_bulk_status_request(&[1, 0, 0]),
        Err(ProtocolError::BadPayloadLength)
    );
    assert_eq!(
        decode_store_bulk_finish_request(&[1, 0, 0, 0, 0]),
        Err(ProtocolError::BadPayloadLength)
    );
    assert_eq!(
        decode_store_bulk_abort_request(&[]),
        Err(ProtocolError::BadPayloadLength)
    );
    assert_eq!(
        decode_store_bulk_credit(&[0; 11]),
        Err(ProtocolError::BadPayloadLength)
    );
    assert_eq!(
        decode_store_bulk_status_response(&[0; 21]),
        Err(ProtocolError::BadPayloadLength)
    );
    assert_eq!(
        decode_store_bulk_finish_response(&[0; 15]),
        Err(ProtocolError::BadPayloadLength)
    );
}
