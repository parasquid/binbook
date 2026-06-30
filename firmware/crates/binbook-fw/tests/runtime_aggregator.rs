#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::{FrameHeader, FrameKind, Opcode, PanelModeCode, Status};
use binbook_fw::{
    diag::{DiagnosticSnapshot, PendingAction, PendingCommand},
    input::PageTurn,
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
