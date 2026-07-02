#![cfg(feature = "diagnostic-console")]

use binbook_fw::diag_log::{DiagEvent, DiagLog};
#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_get_returns_known_command_and_render_records() {
    let mut log = DiagLog::<64>::new();
    let event_receipt = DiagEvent {
        level: 1,
        subsystem: 0,
        event: 0x0001,
        arg0: 0,
        arg1: 0,
        arg2: 0,
    };
    let event_render = DiagEvent {
        level: 1,
        subsystem: 1,
        event: 0x0100,
        arg0: 5,
        arg1: 0,
        arg2: 0,
    };
    log.push(100, event_receipt);
    log.push(200, event_render);

    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::LogGet,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 1,
        payload_len: 6,
    };
    let mut payload = [0u8; 6];
    payload[0..4].copy_from_slice(&0u32.to_le_bytes());
    payload[4..6].copy_from_slice(&512u16.to_le_bytes());

    let mut ctx = binbook_fw::diag::CommandContext::new(0, 10, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf, &mut storage);
    match result {
        binbook_fw::diag::DispatchResult::LogGet { cursor, max_bytes } => {
            assert_eq!(cursor, 0);
            assert_eq!(max_bytes, 512);
        }
        other => panic!("expected LogGet, got {:?}", other),
    }

    let mut log_resp = [0u8; 496];
    let written = binbook_fw::diag::resolve_log_get(&log, 0, 512, &mut log_resp);
    assert!(written > binbook_diagnostic_protocol::LOG_RESPONSE_HEADER_BYTES);

    let next_cursor = u32::from_le_bytes([log_resp[0], log_resp[1], log_resp[2], log_resp[3]]);
    let dropped = u32::from_le_bytes([log_resp[4], log_resp[5], log_resp[6], log_resp[7]]);
    assert_eq!(next_cursor, 2);
    assert_eq!(dropped, 0);

    let rec1 = binbook_diagnostic_protocol::decode_log_record(&log_resp[10..34]).unwrap();
    assert_eq!(rec1.sequence, 0);
    assert_eq!(rec1.event, 0x0001);
    assert_eq!(rec1.tick_ms, 100);

    let rec2 = binbook_diagnostic_protocol::decode_log_record(&log_resp[34..58]).unwrap();
    assert_eq!(rec2.sequence, 1);
    assert_eq!(rec2.event, 0x0100);
    assert_eq!(rec2.arg0, 5);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_get_honors_sequence_cursor_after_overwrite() {
    let mut log = DiagLog::<4>::new();
    let event = DiagEvent {
        level: 1,
        subsystem: 0,
        event: 0x0001,
        arg0: 0,
        arg1: 0,
        arg2: 0,
    };
    for i in 0..8u32 {
        log.push(i * 10, event);
    }

    let mut log_resp = [0u8; 496];
    let written = binbook_fw::diag::resolve_log_get(&log, 4, 512, &mut log_resp);
    let next_cursor = u32::from_le_bytes([log_resp[0], log_resp[1], log_resp[2], log_resp[3]]);
    let dropped = u32::from_le_bytes([log_resp[4], log_resp[5], log_resp[6], log_resp[7]]);
    assert_eq!(dropped, 4);
    assert_eq!(next_cursor, 8);

    let mut pos = binbook_diagnostic_protocol::LOG_RESPONSE_HEADER_BYTES;
    let mut count = 0;
    while pos + binbook_diagnostic_protocol::LOG_RECORD_BYTES <= written {
        let rec = binbook_diagnostic_protocol::decode_log_record(
            &log_resp[pos..pos + binbook_diagnostic_protocol::LOG_RECORD_BYTES],
        )
        .unwrap();
        assert!(rec.sequence >= 4);
        count += 1;
        pos += binbook_diagnostic_protocol::LOG_RECORD_BYTES;
    }
    assert_eq!(count, 4);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_get_honors_byte_budget_on_record_boundaries() {
    let mut log = DiagLog::<64>::new();
    let event = DiagEvent {
        level: 1,
        subsystem: 0,
        event: 0x0001,
        arg0: 0,
        arg1: 0,
        arg2: 0,
    };
    for i in 0..10u32 {
        log.push(i * 10, event);
    }

    let budget = binbook_diagnostic_protocol::LOG_RESPONSE_HEADER_BYTES
        + 2 * binbook_diagnostic_protocol::LOG_RECORD_BYTES;
    let mut log_resp = [0u8; 496];
    let written = binbook_fw::diag::resolve_log_get(&log, 0, budget as u16, &mut log_resp);
    assert_eq!(written, budget);

    let mut pos = binbook_diagnostic_protocol::LOG_RESPONSE_HEADER_BYTES;
    let mut record_count = 0;
    while pos + binbook_diagnostic_protocol::LOG_RECORD_BYTES <= written {
        let _rec = binbook_diagnostic_protocol::decode_log_record(
            &log_resp[pos..pos + binbook_diagnostic_protocol::LOG_RECORD_BYTES],
        )
        .unwrap();
        record_count += 1;
        pos += binbook_diagnostic_protocol::LOG_RECORD_BYTES;
    }
    assert_eq!(record_count, 2);

    let next_cursor = u32::from_le_bytes([log_resp[0], log_resp[1], log_resp[2], log_resp[3]]);
    assert_eq!(next_cursor, 2);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_clear_clears_nonempty_ring_and_dropped_count() {
    let mut log = DiagLog::<4>::new();
    let event = DiagEvent {
        level: 1,
        subsystem: 0,
        event: 0x0001,
        arg0: 0,
        arg1: 0,
        arg2: 0,
    };
    for i in 0..6u32 {
        log.push(i * 10, event);
    }
    assert_eq!(log.record_count(), 4);
    assert_eq!(log.dropped_records(), 2);

    let mut log_resp = [0u8; 496];
    let written = binbook_fw::diag::resolve_log_get(&log, 0, 512, &mut log_resp);
    assert!(written > binbook_diagnostic_protocol::LOG_RESPONSE_HEADER_BYTES);

    let (next_cursor, dropped) = binbook_fw::diag::resolve_log_clear(&mut log);
    assert_eq!(next_cursor, 6);
    assert_eq!(dropped, 0);
    assert_eq!(log.record_count(), 0);

    let written2 = binbook_fw::diag::resolve_log_get(&log, 0, 512, &mut log_resp);
    let next_cursor2 = u32::from_le_bytes([log_resp[0], log_resp[1], log_resp[2], log_resp[3]]);
    let record_count_after = (written2 - binbook_diagnostic_protocol::LOG_RESPONSE_HEADER_BYTES)
        / binbook_diagnostic_protocol::LOG_RECORD_BYTES;
    assert_eq!(record_count_after, 0);
    assert_eq!(next_cursor2, 6);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_clear_dispatch_returns_log_clear_variant() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::LogClear,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 1,
        payload_len: 0,
    };
    let mut ctx = binbook_fw::diag::CommandContext::new(0, 10, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result = binbook_fw::diag::dispatch_command(header, &[], &mut ctx, &mut resp_buf, &mut storage);
    assert_eq!(result, binbook_fw::diag::DispatchResult::LogClear);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_get_dispatch_returns_log_get_variant() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::LogGet,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 1,
        payload_len: 6,
    };
    let mut payload = [0u8; 6];
    payload[0..4].copy_from_slice(&5u32.to_le_bytes());
    payload[4..6].copy_from_slice(&256u16.to_le_bytes());

    let mut ctx = binbook_fw::diag::CommandContext::new(0, 10, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf, &mut storage);
    match result {
        binbook_fw::diag::DispatchResult::LogGet { cursor, max_bytes } => {
            assert_eq!(cursor, 5);
            assert_eq!(max_bytes, 256);
        }
        other => panic!("expected LogGet, got {:?}", other),
    }
}
