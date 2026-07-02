#![cfg(feature = "diagnostic-console")]

use binbook_fw::input::PageTurn;
#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_dispatch_key_right_press_matches_button_right() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    use binbook_fw::diag_storage::UnavailableStorage;
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Key,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 1,
        payload_len: 2,
    };
    let mut ctx = CommandContext::new(5, 20, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = UnavailableStorage;
    let result = dispatch_command(header, &[0x02, 0x01], &mut ctx, &mut resp_buf, &mut storage);
    match result {
        DispatchResult::RenderTurn { turn } => {
            assert_eq!(turn, PageTurn::Next);
        }
        other => panic!("expected RenderTurn, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_dispatch_key_left_press_matches_button_left() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    use binbook_fw::diag_storage::UnavailableStorage;
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Key,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 2,
        payload_len: 2,
    };
    let mut ctx = CommandContext::new(5, 20, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = UnavailableStorage;
    let result = dispatch_command(header, &[0x01, 0x01], &mut ctx, &mut resp_buf, &mut storage);
    match result {
        DispatchResult::RenderTurn { turn } => {
            assert_eq!(turn, PageTurn::Previous);
        }
        other => panic!("expected RenderTurn, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_dispatch_page_next_clamps_at_end() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    use binbook_fw::diag_storage::UnavailableStorage;
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 3,
        payload_len: 1,
    };
    let mut ctx = CommandContext::new(19, 20, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = UnavailableStorage;
    let result = dispatch_command(header, &[0x01], &mut ctx, &mut resp_buf, &mut storage);
    assert_eq!(result, DispatchResult::NoAction);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_dispatch_page_goto_clamps_at_edges() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    use binbook_fw::diag_storage::UnavailableStorage;
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 4,
        payload_len: 5,
    };
    let mut ctx = CommandContext::new(0, 20, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = UnavailableStorage;
    let result = dispatch_command(
        header,
        &[0x05, 0xFF, 0xFF, 0xFF, 0xFF],
        &mut ctx,
        &mut resp_buf, &mut storage,
    );
    match result {
        DispatchResult::Response { status, .. } => {
            assert_eq!(status, binbook_diagnostic_protocol::Status::BadRequest);
        }
        other => panic!("expected BadRequest for out-of-range goto, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_dispatch_page_goto_valid_targets_exact_page() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    use binbook_fw::diag_storage::UnavailableStorage;
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 4,
        payload_len: 5,
    };
    let mut ctx = CommandContext::new(0, 20, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = UnavailableStorage;
    let result = dispatch_command(
        header,
        &[0x05, 0x0A, 0x00, 0x00, 0x00],
        &mut ctx,
        &mut resp_buf, &mut storage,
    );
    match result {
        DispatchResult::RenderPage { target_page } => {
            assert_eq!(target_page, 10);
        }
        other => panic!("expected RenderPage for valid goto, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_dispatch_status_includes_current_state() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    use binbook_fw::diag_storage::UnavailableStorage;
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Status,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 5,
        payload_len: 0,
    };
    let mut ctx = CommandContext::new(7, 30, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = UnavailableStorage;
    let result = dispatch_command(header, &[], &mut ctx, &mut resp_buf, &mut storage);
    match result {
        DispatchResult::Response {
            status,
            payload_len,
        } => {
            assert_eq!(status, binbook_diagnostic_protocol::Status::Ok);
            assert_eq!(payload_len, 21);
        }
        other => panic!("expected Response, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_probe_window_corners_maps_to_render_request() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    use binbook_fw::diag_storage::UnavailableStorage;
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::DisplayProbe,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 6,
        payload_len: 1,
    };
    let mut ctx = CommandContext::new(0, 10, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = UnavailableStorage;
    let result = dispatch_command(header, &[0x03], &mut ctx, &mut resp_buf, &mut storage);
    assert_eq!(
        result,
        DispatchResult::DisplayProbe(binbook_fw::diag::DisplayProbeKind::WindowCorners)
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_probe_clear_white_maps_to_render_request() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    use binbook_fw::diag_storage::UnavailableStorage;
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::DisplayProbe,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 7,
        payload_len: 1,
    };
    let mut ctx = CommandContext::new(0, 10, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = UnavailableStorage;
    let result = dispatch_command(header, &[0x02], &mut ctx, &mut resp_buf, &mut storage);
    assert_eq!(
        result,
        DispatchResult::DisplayProbe(binbook_fw::diag::DisplayProbeKind::ClearWhite)
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_probe_unknown_code_returns_error() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    use binbook_fw::diag_storage::UnavailableStorage;
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::DisplayProbe,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 8,
        payload_len: 1,
    };
    let mut ctx = CommandContext::new(0, 10, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = UnavailableStorage;
    let result = dispatch_command(header, &[0xFF], &mut ctx, &mut resp_buf, &mut storage);
    match result {
        DispatchResult::Response { status, .. } => {
            assert_eq!(status, binbook_diagnostic_protocol::Status::BadRequest);
        }
        other => panic!(
            "expected BadRequest for unknown probe code, got {:?}",
            other
        ),
    }
}
