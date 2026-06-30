#![cfg(feature = "diagnostic-console")]

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_command_error_is_logged_immediately() {
    use binbook_diagnostic_protocol::{
        encode_frame, FrameHeader, FrameKind, Opcode, Status, LOG_RECORD_BYTES,
        LOG_RESPONSE_HEADER_BYTES, MAX_FRAME_BYTES,
    };
    use binbook_fw::diag::{poll_pending_command, SerialState};
    use binbook_fw::diag_log::{DiagLog, EVT_CMD_ERROR};

    let mut state = SerialState::new();
    let mut log = DiagLog::<64>::new();

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Key,
        status: Status::Ok,
        sequence: 7,
        payload_len: 1,
    };
    let mut req_buf = [0u8; MAX_FRAME_BYTES];
    let req_len = encode_frame(&header, &[0xFF], &mut req_buf).unwrap();
    state.feed_rx(&req_buf[..req_len]);

    let _action = poll_pending_command(&mut state, 0, 10, 0, 0, &mut log, 500);

    let mut resp_buf = [0u8; 496];
    let written = binbook_fw::diag::resolve_log_get(&log, 0, 496, &mut resp_buf);
    let mut pos = LOG_RESPONSE_HEADER_BYTES;
    let mut found_error = false;
    while pos + LOG_RECORD_BYTES <= written {
        let rec =
            binbook_diagnostic_protocol::decode_log_record(&resp_buf[pos..pos + LOG_RECORD_BYTES])
                .unwrap();
        if rec.event == EVT_CMD_ERROR {
            found_error = true;
            assert_eq!(rec.arg0, Opcode::Key as i32);
            assert_eq!(rec.arg1, Status::BadRequest as i32);
            assert_eq!(rec.tick_ms, 500);
        }
        pos += LOG_RECORD_BYTES;
    }
    assert!(
        found_error,
        "EVT_CMD_ERROR must appear in log after invalid command"
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_refresh_panel_and_display_error_events_are_emitted() {
    use binbook_diagnostic_protocol::{
        encode_frame, FrameHeader, FrameKind, KeyAction, KeyCode, Opcode, Status, LOG_RECORD_BYTES,
        LOG_RESPONSE_HEADER_BYTES, MAX_FRAME_BYTES,
    };
    use binbook_fw::diag::{poll_pending_command, SerialState};
    use binbook_fw::diag_log::{DiagLog, EVT_CMD_RECEIPT};

    let mut state = SerialState::new();
    let mut log = DiagLog::<64>::new();

    let key_payload = [KeyCode::Right as u8, KeyAction::Press as u8];
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Key,
        status: Status::Ok,
        sequence: 3,
        payload_len: 2,
    };
    let mut req_buf = [0u8; MAX_FRAME_BYTES];
    let req_len = encode_frame(&header, &key_payload, &mut req_buf).unwrap();
    state.feed_rx(&req_buf[..req_len]);

    let _action = poll_pending_command(&mut state, 0, 10, 0, 0, &mut log, 1000);

    let mut resp_buf = [0u8; 496];
    let written = binbook_fw::diag::resolve_log_get(&log, 0, 496, &mut resp_buf);
    let mut pos = LOG_RESPONSE_HEADER_BYTES;
    let mut found_receipt = false;
    while pos + LOG_RECORD_BYTES <= written {
        let rec =
            binbook_diagnostic_protocol::decode_log_record(&resp_buf[pos..pos + LOG_RECORD_BYTES])
                .unwrap();
        if rec.event == EVT_CMD_RECEIPT {
            found_receipt = true;
            assert_eq!(rec.arg0, Opcode::Key as i32);
            assert_eq!(rec.arg1, 3);
            assert_eq!(rec.tick_ms, 1000);
        }
        pos += LOG_RECORD_BYTES;
    }
    assert!(
        found_receipt,
        "EVT_CMD_RECEIPT must appear in log after valid command"
    );
}
