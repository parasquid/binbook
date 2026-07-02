#![cfg(feature = "diagnostic-console")]

#[test]
fn diag_key_right_matches_physical_button_target() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Key,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 10,
        payload_len: 2,
    };
    let mut payload = [0u8; 2];
    payload[0] = binbook_diagnostic_protocol::KeyCode::Right as u8;
    payload[1] = binbook_diagnostic_protocol::KeyAction::Press as u8;

    let mut ctx = binbook_fw::diag::CommandContext::new(3, 8, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf, &mut storage);
    match result {
        binbook_fw::diag::DispatchResult::RenderTurn { turn } => {
            assert_eq!(turn, binbook_fw::input::PageTurn::Next);
        }
        other => panic!("expected RenderTurn, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_key_left_matches_physical_button_target() {
    let current_page = 3u32;
    let page_count = 8u32;

    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Key,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 11,
        payload_len: 2,
    };
    let mut payload = [0u8; 2];
    payload[0] = binbook_diagnostic_protocol::KeyCode::Left as u8;
    payload[1] = binbook_diagnostic_protocol::KeyAction::Press as u8;

    let mut ctx = binbook_fw::diag::CommandContext::new(current_page, page_count, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf, &mut storage);
    match result {
        binbook_fw::diag::DispatchResult::RenderTurn { turn } => {
            assert_eq!(turn, binbook_fw::input::PageTurn::Previous);
        }
        other => panic!("expected RenderTurn, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_all_key_codes_match_physical_mapping() {
    use binbook_fw::input::{target_page_for_button, Button};

    let cases: &[(binbook_diagnostic_protocol::KeyCode, Button)] = &[
        (binbook_diagnostic_protocol::KeyCode::Right, Button::Right),
        (binbook_diagnostic_protocol::KeyCode::Left, Button::Left),
        (binbook_diagnostic_protocol::KeyCode::Up, Button::Up),
        (binbook_diagnostic_protocol::KeyCode::Down, Button::Down),
    ];

    for (code, button) in cases {
        let header = binbook_diagnostic_protocol::FrameHeader {
            kind: binbook_diagnostic_protocol::FrameKind::Request,
            opcode: binbook_diagnostic_protocol::Opcode::Key,
            status: binbook_diagnostic_protocol::Status::Ok,
            sequence: 20,
            payload_len: 2,
        };
        let mut payload = [0u8; 2];
        payload[0] = *code as u8;
        payload[1] = binbook_diagnostic_protocol::KeyAction::Press as u8;

        let mut ctx = binbook_fw::diag::CommandContext::new(5, 10, 0, 0);
        let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
        let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf, &mut storage);

        match result {
            binbook_fw::diag::DispatchResult::RenderTurn { turn } => {
                assert_eq!(
                    turn,
                    target_page_for_button(*button),
                    "key {:?} should match button {:?}",
                    code,
                    button
                );
            }
            other => panic!("key {:?}: expected RenderTurn, got {:?}", code, other),
        }
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_page_goto_zero_from_nonzero_targets_zero() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 30,
        payload_len: 5,
    };
    let mut payload = [0u8; 5];
    payload[0] = binbook_diagnostic_protocol::PageAction::Goto as u8;
    payload[1..5].copy_from_slice(&0u32.to_le_bytes());

    let mut ctx = binbook_fw::diag::CommandContext::new(3, 8, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf, &mut storage);
    match result {
        binbook_fw::diag::DispatchResult::RenderPage { target_page } => {
            assert_eq!(target_page, 0);
        }
        other => panic!("expected RenderPage for goto 0, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_page_goto_nonadjacent_targets_exact_page() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 31,
        payload_len: 5,
    };
    let mut payload = [0u8; 5];
    payload[0] = binbook_diagnostic_protocol::PageAction::Goto as u8;
    payload[1..5].copy_from_slice(&6u32.to_le_bytes());

    let mut ctx = binbook_fw::diag::CommandContext::new(2, 8, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf, &mut storage);
    match result {
        binbook_fw::diag::DispatchResult::RenderPage { target_page } => {
            assert_eq!(target_page, 6);
        }
        other => panic!("expected RenderPage for goto 6, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_page_goto_current_is_no_action() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 32,
        payload_len: 5,
    };
    let mut payload = [0u8; 5];
    payload[0] = binbook_diagnostic_protocol::PageAction::Goto as u8;
    payload[1..5].copy_from_slice(&3u32.to_le_bytes());

    let mut ctx = binbook_fw::diag::CommandContext::new(3, 8, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf, &mut storage);
    assert_eq!(result, binbook_fw::diag::DispatchResult::NoAction);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_page_next_and_previous_clamp_at_edges() {
    let header_next = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 33,
        payload_len: 1,
    };
    let payload_next = [binbook_diagnostic_protocol::PageAction::Next as u8];

    let mut ctx = binbook_fw::diag::CommandContext::new(7, 8, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result_next =
        binbook_fw::diag::dispatch_command(header_next, &payload_next, &mut ctx, &mut resp_buf, &mut storage);
    assert_eq!(
        result_next,
        binbook_fw::diag::DispatchResult::NoAction,
        "next from last page should be NoAction"
    );

    let header_prev = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 34,
        payload_len: 1,
    };
    let payload_prev = [binbook_diagnostic_protocol::PageAction::Previous as u8];

    let mut ctx = binbook_fw::diag::CommandContext::new(0, 8, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result_prev =
        binbook_fw::diag::dispatch_command(header_prev, &payload_prev, &mut ctx, &mut resp_buf, &mut storage);
    assert_eq!(
        result_prev,
        binbook_fw::diag::DispatchResult::NoAction,
        "previous from page 0 should be NoAction"
    );
}
