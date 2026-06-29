use std::{
    boxed::Box,
    future::Future,
    pin::Pin,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
    vec::Vec,
};

use binbook_fw::{
    async_refresh::{DisplayProbeKind, DisplayRequest, RefreshPhase},
    display::{BaseSyncOutcome, GrayRenderOutcome},
    input::PageTurn,
    runtime_engine::{
        ControllerRamState, DisplayBackend, DisplayEngine, EventSink, RuntimeCompletionStatus,
        RuntimeEvent, RuntimeEventKind,
    },
};
use xteink_hal::{HalError, HalResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    InitBw,
    Bw(u32, u32),
    Gray(u32, u32),
    Sync(u32, u32),
    Recovery(u32),
    Probe(DisplayProbeKind, u32),
}

struct Backend {
    operations: Vec<Operation>,
    failures: Vec<Operation>,
    epoch: u32,
    timestamp_ms: u64,
    gray_outcome: GrayRenderOutcome,
    epoch_after_gray: Option<u32>,
    sync_outcome: BaseSyncOutcome,
}

impl Default for Backend {
    fn default() -> Self {
        Self {
            operations: Vec::new(),
            failures: Vec::new(),
            epoch: 0,
            timestamp_ms: 0,
            gray_outcome: GrayRenderOutcome::Completed,
            epoch_after_gray: None,
            sync_outcome: BaseSyncOutcome::Completed,
        }
    }
}

impl Backend {
    fn fail(&mut self, operation: Operation) {
        self.failures.push(operation);
    }

    fn record(&mut self, operation: Operation) -> HalResult<()> {
        self.operations.push(operation);
        if let Some(index) = self.failures.iter().position(|item| *item == operation) {
            self.failures.remove(index);
            Err(HalError::Spi)
        } else {
            Ok(())
        }
    }
}

impl DisplayBackend for Backend {
    fn timestamp_ms(&self) -> Option<u64> {
        Some(self.timestamp_ms)
    }

    fn request_epoch(&self) -> u32 {
        self.epoch
    }

    async fn render_grayscale(
        &mut self,
        page: u32,
        expected_epoch: u32,
    ) -> HalResult<GrayRenderOutcome> {
        self.record(Operation::Gray(page, expected_epoch))?;
        if let Some(epoch) = self.epoch_after_gray {
            self.epoch = epoch;
        }
        Ok(self.gray_outcome)
    }

    async fn init_bw(&mut self) -> HalResult<()> {
        self.record(Operation::InitBw)
    }

    async fn render_bw(&mut self, from: u32, target: u32) -> HalResult<()> {
        self.record(Operation::Bw(from, target))
    }

    async fn sync_bw_base(&mut self, page: u32, expected_epoch: u32) -> HalResult<BaseSyncOutcome> {
        self.record(Operation::Sync(page, expected_epoch))?;
        Ok(self.sync_outcome)
    }

    async fn recover_bw(&mut self, page: u32) -> HalResult<()> {
        self.record(Operation::Recovery(page))
    }

    async fn run_probe(&mut self, kind: DisplayProbeKind, page: u32) -> HalResult<()> {
        self.record(Operation::Probe(kind, page))
    }
}

#[derive(Default)]
struct Events(Vec<RuntimeEvent>);

impl EventSink for Events {
    fn emit(&mut self, event: RuntimeEvent) {
        self.0.push(event);
    }
}

fn noop_raw_waker() -> RawWaker {
    unsafe fn clone(_: *const ()) -> RawWaker {
        noop_raw_waker()
    }
    unsafe fn wake(_: *const ()) {}
    unsafe fn wake_by_ref(_: *const ()) {}
    unsafe fn drop(_: *const ()) {}
    RawWaker::new(
        core::ptr::null(),
        &RawWakerVTable::new(clone, wake, wake_by_ref, drop),
    )
}

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(output) = Future::poll(Pin::as_mut(&mut future), &mut context) {
            return output;
        }
    }
}

fn cold_seeded_engine() -> (DisplayEngine, Backend, Events) {
    let mut engine = DisplayEngine::new(4);
    let mut backend = Backend::default();
    backend.timestamp_ms = 1_000;
    let mut events = Events::default();
    block_on(engine.initialize(&mut backend, &mut events, 0));
    (engine, backend, events)
}

#[test]
fn cold_seed_schedules_overlay_at_350_ms_from_completion() {
    let (mut engine, mut backend, mut events) = cold_seeded_engine();

    assert_eq!(
        backend.operations,
        [Operation::InitBw, Operation::Recovery(0)]
    );
    assert_eq!(engine.phase(), RefreshPhase::GrayDelay);
    assert!(block_on(engine.advance(&mut backend, &mut events, 1_349)).is_none());
    assert!(!backend
        .operations
        .iter()
        .any(|op| matches!(op, Operation::Gray(..))));
    block_on(engine.advance(&mut backend, &mut events, 1_350));
    assert!(backend.operations.contains(&Operation::Gray(0, 0)));
}

#[test]
fn request_before_deadline_cancels_delay_and_starts_bw_immediately() {
    let (mut engine, mut backend, mut events) = cold_seeded_engine();
    backend.epoch = 1;

    let completion = block_on(engine.request(
        DisplayRequest::Goto {
            page: 1,
            completion_sequence: 9,
        },
        &mut backend,
        &mut events,
        1_349,
    ))
    .unwrap();

    assert_eq!(completion.status, RuntimeCompletionStatus::Ok);
    assert!(backend.operations.contains(&Operation::Bw(0, 1)));
    assert!(!backend
        .operations
        .iter()
        .any(|op| matches!(op, Operation::Gray(..))));
}

#[test]
fn successful_enqueue_during_overlay_cancels_without_failure() {
    let (mut engine, mut backend, mut events) = cold_seeded_engine();
    backend.gray_outcome = GrayRenderOutcome::Cancelled;
    backend.epoch = 1;

    block_on(engine.advance(&mut backend, &mut events, 1_350));

    assert_eq!(
        engine.controller_state(),
        ControllerRamState::NeedsFullBwInputs
    );
    assert!(!events
        .0
        .iter()
        .any(|event| matches!(event.kind, RuntimeEventKind::DisplayFailure { .. })));
}

#[test]
fn rejected_request_epoch_does_not_cancel_overlay() {
    let (mut engine, mut backend, mut events) = cold_seeded_engine();

    block_on(engine.advance(&mut backend, &mut events, 1_350));

    assert_eq!(engine.controller_state(), ControllerRamState::BwBaseReady);
    assert!(backend.operations.contains(&Operation::Sync(0, 0)));
}

#[test]
fn request_arriving_after_activation_skips_sync_and_queues_for_next_turn() {
    let (mut engine, mut backend, mut events) = cold_seeded_engine();
    backend.epoch_after_gray = Some(1);
    backend.gray_outcome = GrayRenderOutcome::Completed;

    block_on(engine.advance(&mut backend, &mut events, 1_350));

    assert_eq!(
        engine.controller_state(),
        ControllerRamState::NeedsFullBwInputs
    );
    assert!(!backend
        .operations
        .iter()
        .any(|op| matches!(op, Operation::Sync(..))));
    let completion = block_on(engine.request(
        DisplayRequest::Turn {
            turn: PageTurn::Next,
            completion_sequence: Some(10),
        },
        &mut backend,
        &mut events,
        1_500,
    ))
    .unwrap();
    assert_eq!(completion.page, 1);
    assert!(backend.operations.contains(&Operation::Bw(0, 1)));
}

#[test]
fn cancelled_base_sync_enters_needs_full_inputs_without_recovery() {
    let (mut engine, mut backend, mut events) = cold_seeded_engine();
    backend.sync_outcome = BaseSyncOutcome::Cancelled;

    block_on(engine.advance(&mut backend, &mut events, 1_350));

    assert_eq!(
        engine.controller_state(),
        ControllerRamState::NeedsFullBwInputs
    );
    assert!(!backend
        .operations
        .iter()
        .any(|op| matches!(op, Operation::Recovery(_)) && *op != Operation::Recovery(0)));
}

#[test]
fn completed_base_sync_restores_bw_ready() {
    let (mut engine, mut backend, mut events) = cold_seeded_engine();

    block_on(engine.advance(&mut backend, &mut events, 1_350));

    assert_eq!(engine.controller_state(), ControllerRamState::BwBaseReady);
}

#[test]
fn overlay_or_sync_failure_recovers_once_and_second_failure_faults() {
    for failed in [Operation::Gray(0, 0), Operation::Sync(0, 0)] {
        let (mut engine, mut backend, mut events) = cold_seeded_engine();
        backend.fail(failed);
        block_on(engine.advance(&mut backend, &mut events, 1_350));
        assert_eq!(engine.controller_state(), ControllerRamState::BwBaseReady);
        assert_eq!(
            backend
                .operations
                .iter()
                .filter(|op| **op == Operation::Recovery(0))
                .count(),
            2
        );
    }

    let (mut engine, mut backend, mut events) = cold_seeded_engine();
    backend.fail(Operation::Gray(0, 0));
    backend.fail(Operation::Recovery(0));
    let completion = block_on(engine.advance(&mut backend, &mut events, 1_350)).unwrap();
    assert_eq!(completion.status, RuntimeCompletionStatus::Error);
    assert_eq!(engine.phase(), RefreshPhase::Fault);
}

#[test]
fn probes_execute_and_force_full_bw_inputs() {
    for (sequence, kind) in [
        (1, DisplayProbeKind::FullRefreshCurrent),
        (2, DisplayProbeKind::ClearWhite),
        (3, DisplayProbeKind::WindowCorners),
    ] {
        let (mut engine, mut backend, mut events) = cold_seeded_engine();
        let completion = block_on(engine.request(
            DisplayRequest::Probe {
                kind,
                completion_sequence: sequence,
            },
            &mut backend,
            &mut events,
            1_100,
        ))
        .unwrap();
        assert_eq!(completion.status, RuntimeCompletionStatus::Ok);
        assert_eq!(
            engine.controller_state(),
            ControllerRamState::NeedsFullBwInputs
        );
    }
}
