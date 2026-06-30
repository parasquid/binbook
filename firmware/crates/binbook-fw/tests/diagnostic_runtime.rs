#![cfg(feature = "diagnostic-console")]

#[test]
fn diagnostic_snapshot_builds_status_payload_from_committed_state() {
    use binbook_fw::diag::DiagnosticSnapshot;

    let snapshot = DiagnosticSnapshot {
        current_page: 7,
        page_count: 30,
        panel_mode: binbook_diagnostic_protocol::PanelModeCode::Bw,
        dropped_log_count: 4,
        protocol_error_count: 2,
        last_error: -12,
    };

    let status = snapshot.status_payload();

    assert_eq!(status.current_page, 7);
    assert_eq!(status.page_count, 30);
    assert_eq!(
        status.panel_mode,
        binbook_diagnostic_protocol::PanelModeCode::Bw
    );
    assert_eq!(status.dropped_log_count, 4);
    assert_eq!(status.protocol_error_count, 2);
    assert_eq!(status.last_error, -12);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diagnostic_loop_services_status_and_log_while_render_is_pending() {
    use binbook_fw::diag::{
        DiagnosticLoopState, DiagnosticSnapshot, PendingAction, PendingCommand,
    };
    use binbook_fw::diag_log::{DiagEvent, DiagLog};

    let snapshot = DiagnosticSnapshot {
        current_page: 7,
        page_count: 30,
        panel_mode: binbook_diagnostic_protocol::PanelModeCode::Bw,
        dropped_log_count: 4,
        protocol_error_count: 2,
        last_error: -12,
    };
    let mut log = DiagLog::<8>::new();
    log.push(
        1000,
        DiagEvent {
            level: binbook_fw::diag_log::LEVEL_INFO,
            subsystem: binbook_fw::diag_log::SUB_SERIAL,
            event: binbook_fw::diag_log::EVT_CMD_RECEIPT,
            arg0: 1,
            arg1: 2,
            arg2: 3,
        },
    );
    let mut loop_state = DiagnosticLoopState::<16, 8>::new(snapshot, log);

    let pending = PendingCommand {
        header: binbook_diagnostic_protocol::FrameHeader {
            kind: binbook_diagnostic_protocol::FrameKind::Request,
            opcode: binbook_diagnostic_protocol::Opcode::Page,
            status: binbook_diagnostic_protocol::Status::Ok,
            sequence: 99,
            payload_len: 1,
        },
        action: PendingAction::RenderPage { target_page: 8 },
    };
    loop_state.enqueue_pending(pending).unwrap();

    let status = loop_state.status_payload();
    let mut log_buf = [0u8; 128];
    let log_len = loop_state.resolve_log_get(0, 128, &mut log_buf);

    assert_eq!(status.current_page, 7);
    assert_eq!(status.page_count, 30);
    assert_eq!(status.dropped_log_count, 4);
    assert_eq!(status.protocol_error_count, 2);
    assert_eq!(loop_state.pending_len(), 1);
    assert!(log_len > 0);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diagnostic_loop_reports_error_when_key_queue_is_full_without_evicting_old_requests() {
    use binbook_fw::diag::{
        DiagnosticLoopState, DiagnosticSnapshot, PendingAction, PendingCommand,
    };

    let snapshot = DiagnosticSnapshot {
        current_page: 7,
        page_count: 30,
        panel_mode: binbook_diagnostic_protocol::PanelModeCode::Bw,
        dropped_log_count: 4,
        protocol_error_count: 2,
        last_error: -12,
    };
    let log = binbook_fw::diag_log::DiagLog::<8>::new();
    let mut loop_state = DiagnosticLoopState::<16, 8>::new(snapshot, log);

    for sequence in 0..16u16 {
        loop_state
            .enqueue_pending(PendingCommand {
                header: binbook_diagnostic_protocol::FrameHeader {
                    kind: binbook_diagnostic_protocol::FrameKind::Request,
                    opcode: binbook_diagnostic_protocol::Opcode::Key,
                    status: binbook_diagnostic_protocol::Status::Ok,
                    sequence,
                    payload_len: 0,
                },
                action: PendingAction::RenderPage {
                    target_page: sequence as u32,
                },
            })
            .unwrap();
    }

    let result = loop_state.enqueue_pending_with_status(PendingCommand {
        header: binbook_diagnostic_protocol::FrameHeader {
            kind: binbook_diagnostic_protocol::FrameKind::Request,
            opcode: binbook_diagnostic_protocol::Opcode::Key,
            status: binbook_diagnostic_protocol::Status::Ok,
            sequence: 16,
            payload_len: 0,
        },
        action: PendingAction::RenderPage { target_page: 16 },
    });

    assert_eq!(result, binbook_diagnostic_protocol::Status::Error);
    assert_eq!(loop_state.pending_len(), 16);
    assert_eq!(loop_state.complete_pending().unwrap().header.sequence, 0);
}
