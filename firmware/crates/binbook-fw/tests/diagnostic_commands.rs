#![cfg(feature = "diagnostic-console")]

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_hello_response_has_all_required_fields() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Hello,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 40,
        payload_len: 0,
    };

    let mut ctx = binbook_fw::diag::CommandContext::new(0, 0, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result =
        binbook_fw::diag::dispatch_command(header, &[], &mut ctx, &mut resp_buf, &mut storage);
    match result {
        binbook_fw::diag::DispatchResult::Response { payload_len, .. } => {
            assert!(
                payload_len >= 8,
                "HELLO response must be at least 8 bytes, got {}",
                payload_len
            );
        }
        other => panic!("expected Response for HELLO, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_status_response_uses_live_state_without_truncation() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Status,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 41,
        payload_len: 0,
    };

    let mut ctx = binbook_fw::diag::CommandContext::new(70_001, 80_002, -12i32, 0);
    ctx.protocol_errors = 100_004;
    ctx.dropped_records = 90_003;
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result =
        binbook_fw::diag::dispatch_command(header, &[], &mut ctx, &mut resp_buf, &mut storage);
    match result {
        binbook_fw::diag::DispatchResult::Response { payload_len, .. } => {
            assert_eq!(payload_len, 21, "STATUS payload must be 21 bytes");
        }
        other => panic!("expected Response for STATUS, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_invalid_page_payload_returns_bad_request() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 50,
        payload_len: 5,
    };
    let payload = [
        binbook_diagnostic_protocol::PageAction::Goto as u8,
        0xFF,
        0xFF,
        0xFF,
        0xFF,
    ];

    let mut ctx = binbook_fw::diag::CommandContext::new(0, 8, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result =
        binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf, &mut storage);
    match result {
        binbook_fw::diag::DispatchResult::Response { status, .. } => {
            assert_eq!(status, binbook_diagnostic_protocol::Status::BadRequest);
        }
        other => panic!(
            "expected BadRequest response for invalid goto, got {:?}",
            other
        ),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_refresh_mode_get_returns_active_mode() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::RefreshMode,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 60,
        payload_len: 2,
    };
    let payload = [
        binbook_diagnostic_protocol::RefreshModeAction::Get as u8,
        binbook_diagnostic_protocol::RefreshModeCode::Balanced as u8,
    ];

    let mut ctx = binbook_fw::diag::CommandContext::new(0, 8, 0, 0);
    ctx.refresh_mode = binbook_diagnostic_protocol::RefreshModeCode::Fast;
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result =
        binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf, &mut storage);

    match result {
        binbook_fw::diag::DispatchResult::Response {
            status,
            payload_len,
        } => {
            assert_eq!(status, binbook_diagnostic_protocol::Status::Ok);
            let decoded =
                binbook_diagnostic_protocol::decode_refresh_mode_payload(&resp_buf[..payload_len])
                    .unwrap();
            assert_eq!(
                decoded.mode,
                binbook_diagnostic_protocol::RefreshModeCode::Fast
            );
        }
        other => panic!("expected immediate refresh mode response, got {other:?}"),
    }
}

#[cfg(all(feature = "diagnostic-console", feature = "fast-refresh-mode"))]
#[test]
fn diag_refresh_mode_set_fast_enqueues_mode_change() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::RefreshMode,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 61,
        payload_len: 2,
    };
    let payload = [
        binbook_diagnostic_protocol::RefreshModeAction::Set as u8,
        binbook_diagnostic_protocol::RefreshModeCode::Fast as u8,
    ];

    let mut ctx = binbook_fw::diag::CommandContext::new(0, 8, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result =
        binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf, &mut storage);

    match result {
        binbook_fw::diag::DispatchResult::SetRefreshMode { mode } => {
            assert_eq!(mode, binbook_diagnostic_protocol::RefreshModeCode::Fast);
        }
        other => panic!("expected SetRefreshMode dispatch, got {other:?}"),
    }
}

#[cfg(all(feature = "diagnostic-console", not(feature = "fast-refresh-mode")))]
#[test]
fn diag_refresh_mode_set_fast_without_feature_is_unsupported() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::RefreshMode,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 62,
        payload_len: 2,
    };
    let payload = [
        binbook_diagnostic_protocol::RefreshModeAction::Set as u8,
        binbook_diagnostic_protocol::RefreshModeCode::Fast as u8,
    ];

    let mut ctx = binbook_fw::diag::CommandContext::new(0, 8, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result =
        binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf, &mut storage);

    match result {
        binbook_fw::diag::DispatchResult::Response { status, .. } => {
            assert_eq!(status, binbook_diagnostic_protocol::Status::Unsupported);
        }
        other => panic!("expected Unsupported response, got {other:?}"),
    }
}
