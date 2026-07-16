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

#[test]
fn diag_transfer_limits_report_real_legacy_rx_capacity_without_bulk_advertisement() {
    use binbook_diagnostic_protocol::{
        decode_transfer_limits, FrameHeader, FrameKind, Opcode, Status, CAP_STORAGE_BULK_V1,
        CAP_STORAGE_TRANSFER_LIMITS, CAP_STORAGE_TRANSFER_METRICS, MAX_FRAME_BYTES,
    };

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::StoreTransferLimits,
        status: Status::Ok,
        sequence: 42,
        payload_len: 0,
    };
    let mut ctx = binbook_fw::diag::CommandContext::new(0, 0, 0, 0);
    let mut response = [0; 64];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result =
        binbook_fw::diag::dispatch_command(header, &[], &mut ctx, &mut response, &mut storage);
    let binbook_fw::diag::DispatchResult::Response {
        status: Status::Ok,
        payload_len,
    } = result
    else {
        panic!("unexpected limits result {result:?}");
    };
    let limits = decode_transfer_limits(&response[..payload_len]).unwrap();
    assert_eq!(
        limits.flags,
        if cfg!(feature = "bulk-transfer-runtime") {
            binbook_diagnostic_protocol::TRANSFER_LIMIT_FLAGS
        } else {
            0
        }
    );
    assert_eq!(
        limits.bulk_slots,
        if cfg!(feature = "bulk-transfer-runtime") {
            2
        } else {
            0
        }
    );
    assert_eq!(limits.rx_capacity_bytes, (MAX_FRAME_BYTES * 2) as u16);
    assert_ne!(
        binbook_diagnostic_protocol::ALL_CAPABILITIES & CAP_STORAGE_TRANSFER_LIMITS,
        0
    );
    assert_ne!(
        binbook_diagnostic_protocol::ALL_CAPABILITIES & CAP_STORAGE_TRANSFER_METRICS,
        0
    );
    assert_eq!(
        binbook_diagnostic_protocol::ALL_CAPABILITIES & CAP_STORAGE_BULK_V1,
        0
    );

    let invalid = binbook_fw::diag::dispatch_command(
        FrameHeader {
            payload_len: 1,
            ..header
        },
        &[0],
        &mut ctx,
        &mut response,
        &mut storage,
    );
    assert_eq!(
        invalid,
        binbook_fw::diag::DispatchResult::Response {
            status: Status::BadRequest,
            payload_len: 0
        }
    );
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
