use binbook_diagnostic_protocol::{
    decode_frame, decode_hello_response, decode_log_get_payload, decode_log_record,
    decode_status_payload, decode_store_abort_request, decode_store_delete_request,
    decode_store_list_request, decode_store_read_request, decode_store_read_response,
    decode_store_upload_begin_request, decode_store_upload_begin_response,
    decode_store_upload_commit_request, decode_store_upload_write_request,
    decode_store_upload_write_response, encode_frame, encode_hello_response,
    encode_log_get_payload, encode_log_record, encode_page_payload, encode_status_payload,
    encode_store_abort_request, encode_store_delete_request, encode_store_upload_begin_request,
    encode_store_upload_begin_response, encode_store_upload_commit_request,
    encode_store_upload_write_request, encode_store_upload_write_response, FrameHeader, FrameKind,
    HelloResponse, KeyAction, KeyCode, LogGetPayload, LogRecordPayload, Opcode, PageAction,
    PanelModeCode, ProbeCode, Status, StatusPayload, StorageBackend, EVT_DISPLAY_RECOVERY,
    EVT_INPUT_DECISION, EVT_INPUT_TRANSITION, EVT_REFRESH_PHASE, EVT_RESEED_COMPLETE,
    EVT_RESEED_START, EVT_TURN_BOUNDARY_NOOP, EVT_TURN_DEQUEUED, EVT_TURN_DROPPED, EVT_TURN_QUEUED,
    EVT_TURN_STARTED, FRAME_DELIMITER, MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES, PROTOCOL_VERSION,
    STORE_LIST_ENTRY_HEADER_BYTES,
};

#[test]
fn log_response_header_preserves_count_and_counters() {
    let value = binbook_diagnostic_protocol::LogResponseHeader {
        next_cursor: 70_001,
        dropped_log_count: 90_003,
        record_count: 2,
    };
    let mut bytes = [0u8; binbook_diagnostic_protocol::LOG_RESPONSE_HEADER_BYTES];
    let len = binbook_diagnostic_protocol::encode_log_response_header(value, &mut bytes).unwrap();
    assert_eq!(len, 10);
    assert_eq!(
        binbook_diagnostic_protocol::decode_log_response_header(&bytes).unwrap(),
        value
    );
}

#[test]
fn crash_response_requires_exact_present_layout() {
    let summary = [0xA5; binbook_diagnostic_protocol::CRASH_SUMMARY_BYTES];
    let mut bytes = [0u8; 1 + binbook_diagnostic_protocol::CRASH_SUMMARY_BYTES];
    let len =
        binbook_diagnostic_protocol::encode_crash_response(Some(&summary), &mut bytes).unwrap();
    assert_eq!(len, bytes.len());
    assert_eq!(
        binbook_diagnostic_protocol::decode_crash_response(&bytes).unwrap(),
        Some(&summary[..])
    );
    assert_eq!(
        binbook_diagnostic_protocol::decode_crash_response(&[0]).unwrap(),
        None
    );
    assert!(binbook_diagnostic_protocol::decode_crash_response(&[1]).is_err());
    assert!(binbook_diagnostic_protocol::decode_crash_response(&[0, 0]).is_err());
}

#[test]
fn protocol_version_is_two() {
    assert_eq!(PROTOCOL_VERSION, 2);
}

#[test]
fn deferred_gray_event_codes_are_stable_and_nonzero() {
    let codes = [
        EVT_REFRESH_PHASE,
        EVT_TURN_QUEUED,
        EVT_TURN_DEQUEUED,
        EVT_TURN_DROPPED,
        EVT_RESEED_START,
        EVT_RESEED_COMPLETE,
        EVT_DISPLAY_RECOVERY,
        EVT_INPUT_TRANSITION,
        EVT_INPUT_DECISION,
        EVT_TURN_STARTED,
        EVT_TURN_BOUNDARY_NOOP,
    ];

    assert!(codes.iter().all(|code| *code != 0));
    for (index, code) in codes.iter().enumerate() {
        assert!(!codes[..index].contains(code));
    }
}

#[test]
fn timing_event_codes_are_stable_and_nonzero() {
    use binbook_diagnostic_protocol::{
        EVT_BUSY_WAIT_END, EVT_BUSY_WAIT_START, EVT_DISPLAY_REQUEST_END, EVT_DISPLAY_REQUEST_START,
        EVT_REQUEST_ENQUEUE, EVT_REQUEST_RECEIVE,
    };

    let expected = [
        (EVT_REQUEST_ENQUEUE, 0x0207),
        (EVT_REQUEST_RECEIVE, 0x0208),
        (EVT_DISPLAY_REQUEST_START, 0x030D),
        (EVT_DISPLAY_REQUEST_END, 0x030E),
        (EVT_BUSY_WAIT_START, 0x0404),
        (EVT_BUSY_WAIT_END, 0x0405),
    ];
    for (actual, expected) in expected {
        assert_eq!(actual, expected);
        assert_ne!(actual, 0);
    }
}

#[test]
fn max_frame_bytes_is_4126() {
    assert_eq!(MAX_FRAME_BYTES, 4126);
}

#[test]
fn storage_backend_and_unsupported_status_and_cap_storage() {
    use binbook_diagnostic_protocol::{Status, StorageBackend};
    assert_eq!(StorageBackend::from_u8(0), Some(StorageBackend::Sd));
    assert_eq!(StorageBackend::from_u8(1), Some(StorageBackend::Flash));
    assert_eq!(StorageBackend::from_u8(2), None);
    assert_eq!(Status::Unsupported, Status::from_u8(5).unwrap());
    assert_eq!(binbook_diagnostic_protocol::CAP_STORAGE, 1 << 6);
}

#[test]
fn storage_opcodes_are_defined() {
    use binbook_diagnostic_protocol::Opcode;
    assert_eq!(Opcode::from_u8(0x0A), Some(Opcode::StoreList));
    assert_eq!(Opcode::from_u8(0x0B), Some(Opcode::StoreUploadBegin));
    assert_eq!(Opcode::from_u8(0x0C), Some(Opcode::StoreUploadWrite));
    assert_eq!(Opcode::from_u8(0x0D), Some(Opcode::StoreUploadCommit));
    assert_eq!(Opcode::from_u8(0x0E), Some(Opcode::StoreAbort));
    assert_eq!(Opcode::from_u8(0x0F), Some(Opcode::StoreDelete));
    assert_eq!(Opcode::from_u8(0x10), Some(Opcode::StoreRead));
    assert_eq!(Opcode::from_u8(0x11), None);
}

#[test]
fn frame_delimiter_is_zero() {
    assert_eq!(FRAME_DELIMITER, 0x00);
}

#[test]
fn encode_hello_request_produces_cobs_delimited_frame() {
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Hello,
        status: Status::Ok,
        sequence: 7,
        payload_len: 0,
    };

    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();

    assert_eq!(buf[len - 1], FRAME_DELIMITER);
    assert!(len <= MAX_FRAME_BYTES);
}

#[test]
fn decode_hello_request_roundtrips() {
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Hello,
        status: Status::Ok,
        sequence: 7,
        payload_len: 0,
    };

    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();

    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    let (decoded, _) = decode_frame(&buf[..len], &mut payload).unwrap();
    assert_eq!(decoded.kind, FrameKind::Request);
    assert_eq!(decoded.opcode, Opcode::Hello);
    assert_eq!(decoded.sequence, 7);
    assert_eq!(decoded.payload_len, 0);
}

#[test]
fn decode_rejects_bad_magic() {
    let mut buf = [0u8; MAX_FRAME_BYTES];
    buf[0] = b'X';
    buf[1] = b'Y';
    buf[2] = PROTOCOL_VERSION;
    buf[3] = 1;
    buf[4] = 0;
    buf[5] = 0;
    buf[6..8].copy_from_slice(&7u16.to_le_bytes());
    buf[8..10].copy_from_slice(&0u16.to_le_bytes());
    buf[10..12].copy_from_slice(&0x1234u16.to_le_bytes());
    buf[12] = FRAME_DELIMITER;

    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    assert!(decode_frame(&buf[..13], &mut payload).is_err());
}

#[test]
fn decode_rejects_frame_too_large() {
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Hello,
        status: Status::Ok,
        sequence: 1,
        payload_len: 0,
    };

    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();

    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    assert!(decode_frame(&buf[..len + 1], &mut payload).is_err());
}

#[test]
fn key_right_press_roundtrips() {
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Key,
        status: Status::Ok,
        sequence: 42,
        payload_len: 2,
    };
    let payload = [KeyCode::Right as u8, KeyAction::Press as u8];

    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &payload, &mut buf).unwrap();

    let mut payload_out = [0u8; MAX_PAYLOAD_BYTES];
    let (decoded, payload_len) = decode_frame(&buf[..len], &mut payload_out).unwrap();
    assert_eq!(decoded.opcode, Opcode::Key);
    assert_eq!(decoded.sequence, 42);
    assert_eq!(payload_len, 2);
    assert_eq!(payload_out[0], KeyCode::Right as u8);
    assert_eq!(payload_out[1], KeyAction::Press as u8);
}

#[test]
fn page_goto_payload_roundtrips() {
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Page,
        status: Status::Ok,
        sequence: 99,
        payload_len: 5,
    };
    let mut payload = [0u8; 5];
    payload[0] = PageAction::Goto as u8;
    payload[1..5].copy_from_slice(&3u32.to_le_bytes());

    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &payload, &mut buf).unwrap();

    let mut payload_out = [0u8; MAX_PAYLOAD_BYTES];
    let (decoded, payload_len) = decode_frame(&buf[..len], &mut payload_out).unwrap();
    assert_eq!(decoded.opcode, Opcode::Page);
    assert_eq!(decoded.sequence, 99);
    assert_eq!(payload_len, 5);
    assert_eq!(payload_out[0], PageAction::Goto as u8);
    assert_eq!(
        u32::from_le_bytes([
            payload_out[1],
            payload_out[2],
            payload_out[3],
            payload_out[4]
        ]),
        3
    );
}

#[test]
fn status_response_payload_roundtrips() {
    let header = FrameHeader {
        kind: FrameKind::Response,
        opcode: Opcode::Status,
        status: Status::Ok,
        sequence: 10,
        payload_len: 11,
    };
    let mut payload = [0u8; 11];
    payload[0..4].copy_from_slice(&5u32.to_le_bytes());
    payload[4..8].copy_from_slice(&20u32.to_le_bytes());
    payload[8] = PanelModeCode::Bw as u8;
    payload[9] = 3;
    payload[10] = 0;

    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &payload, &mut buf).unwrap();

    let mut payload_out = [0u8; MAX_PAYLOAD_BYTES];
    let (decoded, payload_len) = decode_frame(&buf[..len], &mut payload_out).unwrap();
    assert_eq!(decoded.opcode, Opcode::Status);
    assert_eq!(payload_len, 11);
    assert_eq!(u32::from_le_bytes(payload_out[0..4].try_into().unwrap()), 5);
    assert_eq!(
        u32::from_le_bytes(payload_out[4..8].try_into().unwrap()),
        20
    );
    assert_eq!(payload_out[8], PanelModeCode::Bw as u8);
    assert_eq!(payload_out[9], 3);
    assert_eq!(payload_out[10], 0);
}

#[test]
fn log_get_request_payload_roundtrips() {
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::LogGet,
        status: Status::Ok,
        sequence: 55,
        payload_len: 6,
    };
    let mut payload = [0u8; 6];
    payload[0..4].copy_from_slice(&100u32.to_le_bytes());
    payload[4..6].copy_from_slice(&512u16.to_le_bytes());

    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &payload, &mut buf).unwrap();

    let mut payload_out = [0u8; MAX_PAYLOAD_BYTES];
    let (decoded, payload_len) = decode_frame(&buf[..len], &mut payload_out).unwrap();
    assert_eq!(decoded.opcode, Opcode::LogGet);
    assert_eq!(decoded.sequence, 55);
    assert_eq!(payload_len, 6);
    assert_eq!(
        u32::from_le_bytes(payload_out[0..4].try_into().unwrap()),
        100
    );
    assert_eq!(u16::from_le_bytes([payload_out[4], payload_out[5]]), 512);
}

#[test]
fn malformed_crc_is_rejected() {
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Hello,
        status: Status::Ok,
        sequence: 1,
        payload_len: 0,
    };

    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();

    let mut bad_buf = buf;
    bad_buf[len - 3] ^= 0xFF;

    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    assert!(decode_frame(&bad_buf[..len], &mut payload).is_err());
}

#[test]
fn display_probe_payload_roundtrips() {
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::DisplayProbe,
        status: Status::Ok,
        sequence: 200,
        payload_len: 1,
    };
    let payload = [ProbeCode::WindowCorners as u8];

    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &payload, &mut buf).unwrap();

    let mut payload_out = [0u8; MAX_PAYLOAD_BYTES];
    let (decoded, payload_len) = decode_frame(&buf[..len], &mut payload_out).unwrap();
    assert_eq!(decoded.opcode, Opcode::DisplayProbe);
    assert_eq!(payload_len, 1);
    assert_eq!(payload_out[0], ProbeCode::WindowCorners as u8);
}

#[test]
fn page_goto_uses_action_plus_full_u32_le() {
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_page_payload(PageAction::Goto, Some(0x0102_0304), &mut buf).unwrap();
    assert_eq!(len, 5);
    assert_eq!(
        &buf[..len],
        &[PageAction::Goto as u8, 0x04, 0x03, 0x02, 0x01]
    );
}

#[test]
fn status_preserves_u32_fields_and_signed_error() {
    let value = StatusPayload {
        current_page: 70_001,
        page_count: 80_002,
        panel_mode: PanelModeCode::Grayscale,
        dropped_log_count: 90_003,
        protocol_error_count: 100_004,
        last_error: -12,
    };
    let mut buf = [0u8; MAX_PAYLOAD_BYTES];
    let len = encode_status_payload(value, &mut buf).unwrap();
    let decoded = decode_status_payload(&buf[..len]).unwrap();
    assert_eq!(decoded.current_page, 70_001);
    assert_eq!(decoded.page_count, 80_002);
    assert_eq!(decoded.panel_mode, PanelModeCode::Grayscale);
    assert_eq!(decoded.dropped_log_count, 90_003);
    assert_eq!(decoded.protocol_error_count, 100_004);
    assert_eq!(decoded.last_error, -12);
}

#[test]
fn hello_contains_identity_version_frame_limit_and_capabilities() {
    let value = HelloResponse {
        protocol_version: PROTOCOL_VERSION,
        max_frame_bytes: MAX_FRAME_BYTES as u16,
        capabilities: 0x3F,
        firmware_name: "binbook-fw",
        target: "xteink-x4",
    };
    let mut buf = [0u8; MAX_PAYLOAD_BYTES];
    let len = encode_hello_response(&value, &mut buf).unwrap();
    let decoded = decode_hello_response(&buf[..len]).unwrap();
    assert_eq!(decoded.protocol_version, PROTOCOL_VERSION);
    assert_eq!(decoded.max_frame_bytes, MAX_FRAME_BYTES as u16);
    assert_eq!(decoded.capabilities, 0x3F);
    assert_eq!(decoded.firmware_name, b"binbook-fw");
    assert_eq!(decoded.target, b"xteink-x4");
}

#[test]
fn log_get_preserves_cursor_and_budget() {
    let value = LogGetPayload {
        cursor_sequence: 0x1234_5678,
        max_bytes: 512,
    };
    let mut buf = [0u8; MAX_PAYLOAD_BYTES];
    let len = encode_log_get_payload(value, &mut buf).unwrap();
    let decoded = decode_log_get_payload(&buf[..len]).unwrap();
    assert_eq!(decoded.cursor_sequence, 0x1234_5678);
    assert_eq!(decoded.max_bytes, 512);
}

#[test]
fn log_record_is_exactly_24_bytes() {
    let value = LogRecordPayload {
        sequence: 1,
        tick_ms: 1000,
        level: 1,
        subsystem: 2,
        event: 0x0010,
        arg0: -5,
        arg1: 100,
        arg2: 0,
    };
    let mut buf = [0u8; 32];
    let len = encode_log_record(value, &mut buf).unwrap();
    assert_eq!(len, 24);
    let decoded = decode_log_record(&buf[..len]).unwrap();
    assert_eq!(decoded.sequence, 1);
    assert_eq!(decoded.tick_ms, 1000);
    assert_eq!(decoded.level, 1);
    assert_eq!(decoded.subsystem, 2);
    assert_eq!(decoded.event, 0x0010);
    assert_eq!(decoded.arg0, -5);
    assert_eq!(decoded.arg1, 100);
    assert_eq!(decoded.arg2, 0);
}

#[test]
fn crash_response_distinguishes_empty_from_present() {
    let header = FrameHeader {
        kind: FrameKind::Response,
        opcode: Opcode::CrashGet,
        status: Status::Ok,
        sequence: 1,
        payload_len: 1,
    };
    let mut frame_buf = [0u8; MAX_FRAME_BYTES];
    let frame_len = encode_frame(&header, &[0x00], &mut frame_buf).unwrap();
    let mut payload_out = [0u8; MAX_PAYLOAD_BYTES];
    let (decoded, _) = decode_frame(&frame_buf[..frame_len], &mut payload_out).unwrap();
    assert_eq!(decoded.opcode, Opcode::CrashGet);
    assert_eq!(payload_out[0], 0x00);

    let header2 = FrameHeader {
        kind: FrameKind::Response,
        opcode: Opcode::CrashGet,
        status: Status::Ok,
        sequence: 2,
        payload_len: 1 + 128,
    };
    let mut present_payload = [0u8; 129];
    present_payload[0] = 0x01;
    let frame_len2 = encode_frame(&header2, &present_payload, &mut frame_buf).unwrap();
    let (decoded2, payload_len2) =
        decode_frame(&frame_buf[..frame_len2], &mut payload_out).unwrap();
    assert_eq!(decoded2.opcode, Opcode::CrashGet);
    assert_eq!(payload_len2, 129);
    assert_eq!(payload_out[0], 0x01);
}

#[test]
fn encode_rejects_header_payload_length_mismatch() {
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Page,
        status: Status::Ok,
        sequence: 1,
        payload_len: 10,
    };
    let payload = [0u8; 5];
    let mut buf = [0u8; MAX_FRAME_BYTES];
    assert_eq!(
        encode_frame(&header, &payload, &mut buf),
        Err(binbook_diagnostic_protocol::ProtocolError::BadPayloadLength)
    );
}

#[test]
fn decode_rejects_trailing_raw_bytes_after_declared_payload() {
    let mut raw = [0u8; MAX_FRAME_BYTES];
    raw[0..2].copy_from_slice(&binbook_diagnostic_protocol::MAGIC);
    raw[2] = PROTOCOL_VERSION;
    raw[3] = FrameKind::Request as u8;
    raw[4] = Opcode::Hello as u8;
    raw[5] = Status::Ok as u8;
    raw[6..8].copy_from_slice(&1u16.to_le_bytes());
    raw[8..10].copy_from_slice(&0u16.to_le_bytes());
    let crc = binbook_diagnostic_protocol::crc16_ccitt_false(&raw[..10]);
    raw[10..12].copy_from_slice(&crc.to_le_bytes());
    raw[12] = 0x42;
    raw[13] = FRAME_DELIMITER;
    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    assert!(decode_frame(&raw[..14], &mut payload).is_err());
}

#[test]
fn decode_rejects_encoded_frame_larger_than_maximum() {
    let mut payload = [0u8; MAX_PAYLOAD_BYTES + 1];
    payload[0] = 0x01;
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Page,
        status: Status::Ok,
        sequence: 1,
        payload_len: MAX_PAYLOAD_BYTES as u16 + 1,
    };
    let mut buf = [0u8; MAX_FRAME_BYTES];
    assert!(encode_frame(&header, &payload, &mut buf).is_err());
}

#[test]
fn cobs_encode_returns_output_too_small_instead_of_panicking() {
    let input = [0x01, 0x02, 0x03];
    let mut output = [0u8; 2];
    assert_eq!(
        binbook_diagnostic_protocol::cobs_encode(&input, &mut output),
        Err(binbook_diagnostic_protocol::ProtocolError::OutputTooSmall)
    );
}

#[test]
fn decode_preserves_unknown_opcode_and_safe_sequence_for_error_response() {
    let mut raw = [0u8; MAX_FRAME_BYTES];
    raw[0..2].copy_from_slice(&binbook_diagnostic_protocol::MAGIC);
    raw[2] = PROTOCOL_VERSION;
    raw[3] = FrameKind::Response as u8;
    raw[4] = 0xFF;
    raw[5] = Status::BadRequest as u8;
    raw[6..8].copy_from_slice(&42u16.to_le_bytes());
    raw[8..10].copy_from_slice(&0u16.to_le_bytes());
    let crc = binbook_diagnostic_protocol::crc16_ccitt_false(&raw[..10]);
    raw[10..12].copy_from_slice(&crc.to_le_bytes());
    let mut encoded = [0u8; MAX_FRAME_BYTES];
    let encoded_len = binbook_diagnostic_protocol::cobs_encode(&raw[..12], &mut encoded).unwrap();
    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    let (header, payload_len) =
        binbook_diagnostic_protocol::decode_raw_frame(&encoded[..encoded_len], &mut payload)
            .unwrap();
    assert_eq!(header.opcode, 0xFF);
    assert_eq!(header.sequence, 42);
    assert_eq!(payload_len, 0);
    assert_eq!(
        decode_frame(&encoded[..encoded_len], &mut payload),
        Err(binbook_diagnostic_protocol::ProtocolError::UnknownOpcode)
    );
}

#[test]
fn store_list_request_roundtrips() {
    let mut buf = [0u8; MAX_PAYLOAD_BYTES];
    let req = binbook_diagnostic_protocol::StoreListRequest {
        backend: StorageBackend::Sd,
        path: "/books",
    };
    let len = binbook_diagnostic_protocol::encode_store_list_request(&req, &mut buf).unwrap();
    let decoded = decode_store_list_request(&buf[..len]).unwrap();
    assert_eq!(decoded.backend, StorageBackend::Sd);
    assert_eq!(decoded.path, "/books");
}

#[test]
fn store_list_request_rejects_bad_backend() {
    let mut buf = [0u8; 10];
    buf[0] = 0xFF;
    buf[1..3].copy_from_slice(&0u16.to_le_bytes());
    assert_eq!(
        decode_store_list_request(&buf[..3]),
        Err(binbook_diagnostic_protocol::ProtocolError::InvalidValue)
    );
}

#[test]
fn store_list_request_rejects_invalid_utf8() {
    let mut buf = [0u8; 10];
    buf[0] = StorageBackend::Sd as u8;
    buf[1..3].copy_from_slice(&2u16.to_le_bytes());
    buf[3] = 0xFF;
    buf[4] = 0xFE;
    assert_eq!(
        decode_store_list_request(&buf[..5]),
        Err(binbook_diagnostic_protocol::ProtocolError::InvalidValue)
    );
}

#[test]
fn store_list_entry_encode_decode_roundtrips() {
    let mut buf = [0u8; MAX_PAYLOAD_BYTES];
    let name = b"book.binbook";
    let len = binbook_diagnostic_protocol::encode_store_entry(0, name, 123456, &mut buf).unwrap();
    let expected = STORE_LIST_ENTRY_HEADER_BYTES + name.len();
    assert_eq!(len, expected);
    let (entry_type, decoded_name, size) =
        binbook_diagnostic_protocol::decode_store_list_entry(&buf[..len]).unwrap();
    assert_eq!(entry_type, 0);
    assert_eq!(decoded_name, name);
    assert_eq!(size, 123456);
}

#[test]
fn store_list_entries_encoded_via_callback() {
    let mut buf = [0u8; MAX_PAYLOAD_BYTES];
    let mut call_count = 0;
    let total = binbook_diagnostic_protocol::encode_store_list_entries(
        &mut buf,
        |out| {
            call_count += 1;
            binbook_diagnostic_protocol::encode_store_entry(0, b"a.txt", 100, out)
        },
        1,
    )
    .unwrap();
    assert_eq!(call_count, 1);
    let count = binbook_diagnostic_protocol::decode_store_list_count(&buf[..total]).unwrap();
    assert_eq!(count, 1);
    let (entry_type, name, size) =
        binbook_diagnostic_protocol::decode_store_list_entry(&buf[2..total]).unwrap();
    assert_eq!(entry_type, 0);
    assert_eq!(name, b"a.txt");
    assert_eq!(size, 100);
}

#[test]
fn store_list_entry_rejects_oversized_name() {
    let large_name = [0u8; 200];
    let mut buf = [0u8; MAX_PAYLOAD_BYTES];
    let len = binbook_diagnostic_protocol::encode_store_entry(0, &large_name, 0, &mut buf).unwrap();
    assert_eq!(
        binbook_diagnostic_protocol::decode_store_list_entry(&buf[..len]),
        Err(binbook_diagnostic_protocol::ProtocolError::InvalidValue)
    );
}

#[test]
fn store_read_request_roundtrips() {
    let mut buf = [0u8; MAX_PAYLOAD_BYTES];
    let req = binbook_diagnostic_protocol::StoreReadRequest {
        backend: StorageBackend::Flash,
        path: "/config.json",
    };
    let len = binbook_diagnostic_protocol::encode_store_read_request(&req, &mut buf).unwrap();
    let decoded = decode_store_read_request(&buf[..len]).unwrap();
    assert_eq!(decoded.backend, StorageBackend::Flash);
    assert_eq!(decoded.path, "/config.json");
}

#[test]
fn store_read_response_roundtrips() {
    let data = b"hello, storage!";
    let mut buf = [0u8; MAX_PAYLOAD_BYTES];
    let len = binbook_diagnostic_protocol::encode_store_read_response(data, &mut buf).unwrap();
    assert_eq!(len, 4 + data.len());
    let decoded = decode_store_read_response(&buf[..len]).unwrap();
    assert_eq!(decoded, data);
}

#[test]
fn store_read_response_rejects_truncated() {
    assert_eq!(
        decode_store_read_response(&[0u8; 3]),
        Err(binbook_diagnostic_protocol::ProtocolError::BadPayloadLength)
    );
}

#[test]
fn store_read_response_rejects_length_mismatch() {
    let mut payload = [0u8; 10];
    payload[0..4].copy_from_slice(&5u32.to_le_bytes());
    assert_eq!(
        decode_store_read_response(&payload),
        Err(binbook_diagnostic_protocol::ProtocolError::BadPayloadLength)
    );
}

#[test]
fn store_upload_begin_request_roundtrips() {
    let mut buf = [0u8; MAX_PAYLOAD_BYTES];
    let req = binbook_diagnostic_protocol::StoreUploadBeginRequest {
        backend: StorageBackend::Sd,
        path: "/books/new.binbook",
        file_size: 1048576,
        expected_crc32: 0xDEADBEEF,
    };
    let len = encode_store_upload_begin_request(&req, &mut buf).unwrap();
    let decoded = decode_store_upload_begin_request(&buf[..len]).unwrap();
    assert_eq!(decoded.backend, StorageBackend::Sd);
    assert_eq!(decoded.path, "/books/new.binbook");
    assert_eq!(decoded.file_size, 1048576);
    assert_eq!(decoded.expected_crc32, 0xDEADBEEF);
}

#[test]
fn store_upload_begin_request_rejects_truncated() {
    assert_eq!(
        decode_store_upload_begin_request(&[0u8; 10]),
        Err(binbook_diagnostic_protocol::ProtocolError::BadPayloadLength)
    );
}

#[test]
fn store_upload_begin_response_roundtrips() {
    let mut buf = [0u8; 4];
    let len = encode_store_upload_begin_response(42, &mut buf).unwrap();
    assert_eq!(len, 4);
    assert_eq!(decode_store_upload_begin_response(&buf).unwrap(), 42);
}

#[test]
fn store_upload_write_request_roundtrips() {
    let data = b"hello, chunk!";
    let mut buf = [0u8; MAX_PAYLOAD_BYTES];
    let req = binbook_diagnostic_protocol::StoreUploadWriteRequest {
        upload_id: 7,
        offset: 4096,
        data,
    };
    let len = encode_store_upload_write_request(&req, &mut buf).unwrap();
    let decoded = decode_store_upload_write_request(&buf[..len]).unwrap();
    assert_eq!(decoded.upload_id, 7);
    assert_eq!(decoded.offset, 4096);
    assert_eq!(decoded.data, data);
}

#[test]
fn store_upload_write_request_rejects_truncated() {
    assert_eq!(
        decode_store_upload_write_request(&[0u8; 7]),
        Err(binbook_diagnostic_protocol::ProtocolError::BadPayloadLength)
    );
}

#[test]
fn store_upload_write_response_roundtrips() {
    let mut buf = [0u8; 4];
    let len = encode_store_upload_write_response(512, &mut buf).unwrap();
    assert_eq!(len, 4);
    assert_eq!(decode_store_upload_write_response(&buf).unwrap(), 512);
}

#[test]
fn store_upload_commit_request_roundtrips() {
    let mut buf = [0u8; 4];
    let len = encode_store_upload_commit_request(99, &mut buf).unwrap();
    assert_eq!(len, 4);
    assert_eq!(decode_store_upload_commit_request(&buf).unwrap(), 99);
}

#[test]
fn store_abort_request_roundtrips() {
    let mut buf = [0u8; 4];
    let len = encode_store_abort_request(5, &mut buf).unwrap();
    assert_eq!(len, 4);
    assert_eq!(decode_store_abort_request(&buf).unwrap(), 5);
}

#[test]
fn store_delete_request_roundtrips() {
    let mut buf = [0u8; MAX_PAYLOAD_BYTES];
    let req = binbook_diagnostic_protocol::StoreDeleteRequest {
        backend: StorageBackend::Flash,
        path: "/tmp/old.binbook",
    };
    let len = encode_store_delete_request(&req, &mut buf).unwrap();
    let decoded = decode_store_delete_request(&buf[..len]).unwrap();
    assert_eq!(decoded.backend, StorageBackend::Flash);
    assert_eq!(decoded.path, "/tmp/old.binbook");
}

#[test]
fn store_delete_request_rejects_truncated() {
    assert_eq!(
        decode_store_delete_request(&[0u8; 2]),
        Err(binbook_diagnostic_protocol::ProtocolError::BadPayloadLength)
    );
}
