#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::{FrameHeader, FrameKind, Opcode, PanelModeCode, Status};
use binbook_fw::{
    async_refresh::RefreshPhase,
    diag::{DiagnosticSnapshot, PendingAction, PendingCommand},
    diag_log::{
        DiagLogRecord, EVT_INPUT_DECISION, EVT_INPUT_TRANSITION, EVT_REFRESH_PHASE,
        EVT_TURN_BOUNDARY_NOOP, EVT_TURN_DROPPED, EVT_TURN_STARTED,
    },
    input::{Button, InputDecision, PageTurn},
    runtime_aggregator::{ReserveError, RuntimeAggregator},
    runtime_engine::{
        RuntimeCompletion, RuntimeCompletionStatus, RuntimeEvent, RuntimeEventKind,
        RuntimePanelMode,
    },
};

fn pending(sequence: u16) -> PendingCommand {
    PendingCommand {
        header: FrameHeader {
            kind: FrameKind::Request,
            opcode: Opcode::Key,
            status: Status::Ok,
            sequence,
            payload_len: 0,
        },
        action: PendingAction::RenderTurn {
            turn: PageTurn::Next,
        },
    }
}

fn event(timestamp_ms: u64, kind: RuntimeEventKind) -> RuntimeEvent {
    RuntimeEvent { timestamp_ms, kind }
}

fn aggregator<const N: usize>() -> RuntimeAggregator<N, 32> {
    RuntimeAggregator::new(DiagnosticSnapshot {
        current_page: 0,
        page_count: 4,
        panel_mode: PanelModeCode::Unknown,
        dropped_log_count: 0,
        protocol_error_count: 0,
        last_error: 0,
    })
}

#[test]
fn reservation_rejects_duplicates_and_capacity_before_hardware_enqueue() {
    let mut aggregator = aggregator::<2>();
    assert_eq!(aggregator.reserve(pending(10)), Ok(()));
    assert_eq!(
        aggregator.reserve(pending(10)),
        Err(ReserveError::DuplicateSequence)
    );
    assert_eq!(aggregator.reserve(pending(11)), Ok(()));
    assert_eq!(aggregator.reserve(pending(12)), Err(ReserveError::Full));
    assert_eq!(aggregator.pending_len(), 2);
}

#[test]
fn capacity_failure_never_enqueues_hardware_and_enqueue_failure_rolls_back() {
    let mut aggregator = aggregator::<1>();
    aggregator.reserve(pending(1)).unwrap();
    let mut enqueue_called = false;
    assert_eq!(
        aggregator.reserve_and_enqueue(pending(2), || {
            enqueue_called = true;
            true
        }),
        Err(ReserveError::Full),
    );
    assert!(!enqueue_called);

    aggregator.cancel(1).unwrap();
    assert_eq!(
        aggregator.reserve_and_enqueue(pending(3), || false),
        Err(ReserveError::EnqueueFailed),
    );
    assert_eq!(aggregator.pending_len(), 0);
}

#[test]
fn request_epoch_changes_only_after_successful_reserve_and_enqueue() {
    let mut aggregator = aggregator::<1>();
    let mut epoch = 0u32;

    assert_eq!(
        aggregator.reserve_and_enqueue(pending(1), || {
            epoch += 1;
            true
        }),
        Ok(())
    );
    assert_eq!(epoch, 1);
    assert_eq!(
        aggregator.reserve_and_enqueue(pending(1), || {
            epoch += 1;
            true
        }),
        Err(ReserveError::DuplicateSequence)
    );
    assert_eq!(epoch, 1);
    assert_eq!(
        aggregator.reserve_and_enqueue(pending(2), || {
            epoch += 1;
            true
        }),
        Err(ReserveError::Full)
    );
    assert_eq!(epoch, 1);

    aggregator.cancel(1).unwrap();
    assert_eq!(
        aggregator.reserve_and_enqueue(pending(3), || false),
        Err(ReserveError::EnqueueFailed)
    );
    assert_eq!(epoch, 1);
}

#[test]
fn completions_match_pending_commands_by_sequence_not_fifo_position() {
    let mut aggregator = aggregator::<4>();
    aggregator.reserve(pending(20)).unwrap();
    aggregator.reserve(pending(21)).unwrap();

    let second = aggregator
        .commit(event(
            50,
            RuntimeEventKind::Completion(RuntimeCompletion {
                sequence: Some(21),
                status: RuntimeCompletionStatus::Ok,
                page: 2,
                error: None,
            }),
        ))
        .unwrap();
    assert_eq!(second.pending.header.sequence, 21);
    assert_eq!(aggregator.pending_len(), 1);

    let first = aggregator
        .commit(event(
            60,
            RuntimeEventKind::Completion(RuntimeCompletion {
                sequence: Some(20),
                status: RuntimeCompletionStatus::Ok,
                page: 1,
                error: None,
            }),
        ))
        .unwrap();
    assert_eq!(first.pending.header.sequence, 20);
}

#[test]
fn completion_updates_snapshot_and_log_before_it_is_forwarded() {
    let mut aggregator = aggregator::<2>();
    aggregator.reserve(pending(7)).unwrap();
    aggregator.commit(event(
        10,
        RuntimeEventKind::PanelModeChanged(RuntimePanelMode::Bw),
    ));
    let committed = aggregator
        .commit(event(
            20,
            RuntimeEventKind::Completion(RuntimeCompletion {
                sequence: Some(7),
                status: RuntimeCompletionStatus::Ok,
                page: 3,
                error: None,
            }),
        ))
        .unwrap();

    assert_eq!(committed.snapshot.current_page, 3);
    assert_eq!(committed.snapshot.panel_mode, PanelModeCode::Bw);
    assert!(committed.log_sequence.is_some());
    assert_eq!(aggregator.snapshot().current_page, 3);
}

#[test]
fn physical_completion_updates_snapshot_without_protocol_forwarding() {
    let mut aggregator = aggregator::<2>();
    assert!(aggregator
        .commit(event(
            30,
            RuntimeEventKind::Completion(RuntimeCompletion {
                sequence: None,
                status: RuntimeCompletionStatus::Ok,
                page: 1,
                error: None,
            }),
        ))
        .is_none());
    assert_eq!(aggregator.snapshot().current_page, 1);
}

#[test]
fn queue_drops_and_real_phase_events_are_recorded_with_origin_timestamps() {
    let mut aggregator = aggregator::<2>();
    aggregator.commit(event(
        350,
        RuntimeEventKind::PhaseChanged(RefreshPhase::GrayRefreshing),
    ));
    aggregator.commit(event(
        400,
        RuntimeEventKind::TurnDropped {
            turn: PageTurn::Next,
        },
    ));

    let mut records = [DiagLogRecord::default(); 4];
    let result = aggregator.log().read_from_sequence(0, &mut records);
    assert_eq!(result.record_count, 2);
    assert_eq!(records[0].event, EVT_REFRESH_PHASE);
    assert_eq!(records[0].tick_ms, 350);
    assert_eq!(records[1].event, EVT_TURN_DROPPED);
    assert_eq!(records[1].tick_ms, 400);
}

#[test]
fn navigation_burst_events_keep_exact_log_argument_layouts() {
    let mut aggregator = aggregator::<2>();
    for kind in [
        RuntimeEventKind::InputTransition {
            ch1: 500,
            ch2: 4095,
            observed: Some(Button::Right),
        },
        RuntimeEventKind::InputDecision {
            observed: Some(Button::Right),
            decision: InputDecision::Press(Button::Right),
            elapsed_ms: 101,
        },
        RuntimeEventKind::TurnStarted {
            sequence: Some(44),
            from: 8,
            target: 9,
        },
        RuntimeEventKind::TurnBoundaryNoop {
            sequence: Some(45),
            page: 0,
            turn: PageTurn::Previous,
        },
    ] {
        aggregator.commit(event(123, kind));
    }

    let mut records = [DiagLogRecord::default(); 4];
    aggregator.log().read_from_sequence(0, &mut records);
    assert_eq!(
        (
            records[0].event,
            records[0].arg0,
            records[0].arg1,
            records[0].arg2
        ),
        (EVT_INPUT_TRANSITION, 500, 4095, Button::Right as i32)
    );
    assert_eq!(
        (
            records[1].event,
            records[1].arg0,
            records[1].arg1,
            records[1].arg2
        ),
        (EVT_INPUT_DECISION, Button::Right as i32, 0, 101)
    );
    assert_eq!(
        (
            records[2].event,
            records[2].arg0,
            records[2].arg1,
            records[2].arg2
        ),
        (EVT_TURN_STARTED, 44, 8, 9)
    );
    assert_eq!(
        (
            records[3].event,
            records[3].arg0,
            records[3].arg1,
            records[3].arg2
        ),
        (EVT_TURN_BOUNDARY_NOOP, 45, 0, PageTurn::Previous as i32)
    );
}

#[test]
fn log_clear_is_nonempty_and_status_and_logs_are_owned_by_aggregator() {
    let mut aggregator = aggregator::<2>();
    aggregator.commit(event(
        1,
        RuntimeEventKind::TurnDropped {
            turn: PageTurn::Previous,
        },
    ));
    assert_eq!(aggregator.log().record_count(), 1);
    let next = aggregator.clear_log();
    assert_eq!(aggregator.log().record_count(), 0);
    assert_eq!(next, 1);
    assert_eq!(aggregator.status_payload().page_count, 4);
}
