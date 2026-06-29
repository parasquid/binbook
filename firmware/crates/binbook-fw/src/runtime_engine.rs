//! Allocation-free, host-testable staged-grayscale display engine.

use crate::{
    async_refresh::{
        DisplayProbeKind, DisplayRequest, RefreshAction, RefreshCoordinator, RefreshPhase,
    },
    display::{BaseSyncOutcome, GrayRenderOutcome},
    input::{apply_page_turn, PageTurn},
};
use xteink_hal::{HalError, HalResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePanelMode {
    Unknown,
    Grayscale,
    Bw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerRamState {
    BwBaseReady,
    GrayOverlayResident,
    BaseSyncInProgress,
    NeedsFullBwInputs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCompletionStatus {
    Ok,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeCompletion {
    pub sequence: Option<u16>,
    pub status: RuntimeCompletionStatus,
    pub page: u32,
    pub error: Option<HalError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeEventKind {
    FirmwareStarted {
        page_count: u32,
    },
    ProtocolCommand {
        opcode: u8,
        sequence: u16,
    },
    PhaseChanged(RefreshPhase),
    GrayDelayCancelled {
        page: u32,
    },
    PanelModeChanged(RuntimePanelMode),
    ControllerRamStateChanged(ControllerRamState),
    TurnQueued {
        sequence: Option<u16>,
        turn: PageTurn,
    },
    PageDisplayed {
        from: u32,
        page: u32,
    },
    GrayStarted {
        page: u32,
    },
    GrayCancelled {
        page: u32,
    },
    GrayActivated {
        page: u32,
    },
    WaveformSelected {
        waveform_hint: u16,
        lut_revision: u16,
    },
    GrayCompleted {
        page: u32,
    },
    BaseSyncStarted {
        page: u32,
    },
    BaseSyncCancelled {
        page: u32,
    },
    BaseSyncCompleted {
        page: u32,
    },
    RecoveryStarted {
        page: u32,
    },
    RecoveryCompleted {
        page: u32,
    },
    ProbeStarted {
        sequence: u16,
        kind: DisplayProbeKind,
    },
    ProbeCompleted {
        sequence: u16,
        kind: DisplayProbeKind,
    },
    TurnDropped {
        turn: PageTurn,
    },
    DisplayFailure {
        error: HalError,
        page: u32,
    },
    Completion(RuntimeCompletion),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeEvent {
    pub timestamp_ms: u64,
    pub kind: RuntimeEventKind,
}

pub trait EventSink {
    fn emit(&mut self, event: RuntimeEvent);
}

#[allow(async_fn_in_trait)]
pub trait DisplayBackend {
    fn timestamp_ms(&self) -> Option<u64> {
        None
    }
    fn request_epoch(&self) -> u32 {
        0
    }
    async fn init_grayscale(&mut self) -> HalResult<()> {
        Ok(())
    }
    async fn render_grayscale(
        &mut self,
        page: u32,
        expected_epoch: u32,
    ) -> HalResult<GrayRenderOutcome>;
    async fn init_bw(&mut self) -> HalResult<()>;
    async fn render_bw(&mut self, from: u32, target: u32) -> HalResult<()>;
    async fn sync_bw_base(&mut self, page: u32, expected_epoch: u32) -> HalResult<BaseSyncOutcome>;
    async fn recover_bw(&mut self, page: u32) -> HalResult<()>;
    async fn run_probe(&mut self, kind: DisplayProbeKind, page: u32) -> HalResult<()>;
}

pub struct DisplayEngine {
    page_count: u32,
    coordinator: RefreshCoordinator,
    panel_mode: RuntimePanelMode,
    controller_state: ControllerRamState,
}

impl DisplayEngine {
    pub fn new(page_count: u32) -> Self {
        Self {
            page_count,
            coordinator: RefreshCoordinator::new(page_count),
            panel_mode: RuntimePanelMode::Unknown,
            controller_state: ControllerRamState::NeedsFullBwInputs,
        }
    }

    pub fn current_page(&self) -> u32 {
        self.coordinator.displayed_page()
    }
    pub fn phase(&self) -> RefreshPhase {
        self.coordinator.phase()
    }
    pub fn panel_mode(&self) -> RuntimePanelMode {
        self.panel_mode
    }
    pub fn controller_state(&self) -> ControllerRamState {
        self.controller_state
    }
    pub fn differential_ready(&self) -> bool {
        self.controller_state == ControllerRamState::BwBaseReady
    }

    pub async fn initialize<B: DisplayBackend, S: EventSink>(
        &mut self,
        backend: &mut B,
        events: &mut S,
        now_ms: u64,
    ) -> Option<RuntimeCompletion> {
        self.phase_event(events, now_ms);
        emit(
            events,
            now_ms,
            RuntimeEventKind::RecoveryStarted { page: 0 },
        );
        let result = async {
            backend.init_bw().await?;
            self.set_panel_mode(RuntimePanelMode::Bw, events, now_ms);
            backend.recover_bw(0).await
        }
        .await;
        let completed_ms = backend.timestamp_ms().unwrap_or(now_ms);
        match result {
            Ok(()) => {
                self.coordinator.record_seed_complete(0, completed_ms);
                self.set_controller_state(ControllerRamState::BwBaseReady, events, completed_ms);
                emit(
                    events,
                    completed_ms,
                    RuntimeEventKind::RecoveryCompleted { page: 0 },
                );
                self.phase_event(events, completed_ms);
                None
            }
            Err(error) => {
                emit_failure(events, completed_ms, error, 0);
                self.coordinator.record_failure();
                self.phase_event(events, completed_ms);
                Some(self.complete(
                    None,
                    RuntimeCompletionStatus::Error,
                    Some(error),
                    events,
                    completed_ms,
                ))
            }
        }
    }

    pub async fn request<B: DisplayBackend, S: EventSink>(
        &mut self,
        request: DisplayRequest,
        backend: &mut B,
        events: &mut S,
        now_ms: u64,
    ) -> Option<RuntimeCompletion> {
        if self.phase() == RefreshPhase::Fault {
            return Some(self.complete(
                request_sequence(request),
                RuntimeCompletionStatus::Error,
                Some(HalError::InvalidParam),
                events,
                now_ms,
            ));
        }
        match request {
            DisplayRequest::Probe {
                kind,
                completion_sequence,
            } => {
                self.run_probe(kind, completion_sequence, backend, events, now_ms)
                    .await
            }
            DisplayRequest::Turn {
                turn,
                completion_sequence,
            } => {
                emit(
                    events,
                    now_ms,
                    RuntimeEventKind::TurnQueued {
                        sequence: completion_sequence,
                        turn,
                    },
                );
                let target = apply_page_turn(self.current_page(), self.page_count, turn);
                Some(
                    self.navigate(target, completion_sequence, backend, events, now_ms)
                        .await,
                )
            }
            DisplayRequest::Goto {
                page,
                completion_sequence,
            } => Some(
                self.navigate(page, Some(completion_sequence), backend, events, now_ms)
                    .await,
            ),
        }
    }

    pub async fn advance<B: DisplayBackend, S: EventSink>(
        &mut self,
        backend: &mut B,
        events: &mut S,
        now_ms: u64,
    ) -> Option<RuntimeCompletion> {
        if let RefreshAction::WaitUntil { deadline_ms } = self.coordinator.next_action() {
            if now_ms < deadline_ms {
                return None;
            }
            self.coordinator.gray_deadline_elapsed(now_ms);
        }
        if matches!(
            self.coordinator.next_action(),
            RefreshAction::RenderGray { .. }
        ) {
            return self.run_staged_gray(backend, events, now_ms).await;
        }
        None
    }

    async fn navigate<B: DisplayBackend, S: EventSink>(
        &mut self,
        target: u32,
        sequence: Option<u16>,
        backend: &mut B,
        events: &mut S,
        now_ms: u64,
    ) -> RuntimeCompletion {
        let from = self.current_page();
        if target >= self.page_count {
            return self.complete(
                sequence,
                RuntimeCompletionStatus::Error,
                Some(HalError::InvalidParam),
                events,
                now_ms,
            );
        }
        let cancelled_delay = self.phase() == RefreshPhase::GrayDelay;
        self.coordinator.request_arrived();
        if cancelled_delay {
            emit(
                events,
                now_ms,
                RuntimeEventKind::GrayDelayCancelled { page: from },
            );
        }
        if target == from {
            return self.complete(sequence, RuntimeCompletionStatus::Ok, None, events, now_ms);
        }
        self.coordinator.start_bw(target);
        self.phase_event(events, now_ms);
        let result = backend.render_bw(from, target).await;
        let completed_ms = backend.timestamp_ms().unwrap_or(now_ms);
        match result {
            Ok(()) => {
                self.set_panel_mode(RuntimePanelMode::Bw, events, completed_ms);
                self.set_controller_state(ControllerRamState::BwBaseReady, events, completed_ms);
                self.coordinator.record_bw_complete(target, completed_ms);
                emit(
                    events,
                    completed_ms,
                    RuntimeEventKind::PageDisplayed { from, page: target },
                );
                self.phase_event(events, completed_ms);
                self.complete(
                    sequence,
                    RuntimeCompletionStatus::Ok,
                    None,
                    events,
                    completed_ms,
                )
            }
            Err(error) => {
                emit_failure(events, completed_ms, error, target);
                self.coordinator.record_failure();
                self.recover(target, sequence, backend, events, completed_ms)
                    .await
            }
        }
    }

    async fn run_staged_gray<B: DisplayBackend, S: EventSink>(
        &mut self,
        backend: &mut B,
        events: &mut S,
        now_ms: u64,
    ) -> Option<RuntimeCompletion> {
        let page = match self.coordinator.next_action() {
            RefreshAction::RenderGray { page } => page,
            _ => return None,
        };
        let expected_epoch = backend.request_epoch();
        self.phase_event(events, now_ms);
        emit(events, now_ms, RuntimeEventKind::GrayStarted { page });
        let result = backend.render_grayscale(page, expected_epoch).await;
        let completed_ms = backend.timestamp_ms().unwrap_or(now_ms);
        match result {
            Ok(GrayRenderOutcome::Cancelled) => {
                self.coordinator.record_gray_cancelled();
                self.set_controller_state(
                    ControllerRamState::NeedsFullBwInputs,
                    events,
                    completed_ms,
                );
                emit(
                    events,
                    completed_ms,
                    RuntimeEventKind::GrayCancelled { page },
                );
                self.phase_event(events, completed_ms);
                None
            }
            Ok(GrayRenderOutcome::Completed) => {
                self.set_panel_mode(RuntimePanelMode::Grayscale, events, completed_ms);
                self.set_controller_state(
                    ControllerRamState::GrayOverlayResident,
                    events,
                    completed_ms,
                );
                emit(
                    events,
                    completed_ms,
                    RuntimeEventKind::GrayCompleted { page },
                );
                self.coordinator.record_gray_complete();
                self.phase_event(events, completed_ms);
                if backend.request_epoch() != expected_epoch {
                    self.coordinator.skip_base_sync();
                    self.set_controller_state(
                        ControllerRamState::NeedsFullBwInputs,
                        events,
                        completed_ms,
                    );
                    self.phase_event(events, completed_ms);
                    return None;
                }
                emit(
                    events,
                    completed_ms,
                    RuntimeEventKind::BaseSyncStarted { page },
                );
                self.set_controller_state(
                    ControllerRamState::BaseSyncInProgress,
                    events,
                    completed_ms,
                );
                match backend.sync_bw_base(page, expected_epoch).await {
                    Ok(BaseSyncOutcome::Completed) => {
                        self.coordinator.record_base_sync_complete();
                        self.set_controller_state(
                            ControllerRamState::BwBaseReady,
                            events,
                            completed_ms,
                        );
                        emit(
                            events,
                            completed_ms,
                            RuntimeEventKind::BaseSyncCompleted { page },
                        );
                        self.phase_event(events, completed_ms);
                        None
                    }
                    Ok(BaseSyncOutcome::Cancelled) => {
                        self.coordinator.record_base_sync_complete();
                        self.set_controller_state(
                            ControllerRamState::NeedsFullBwInputs,
                            events,
                            completed_ms,
                        );
                        emit(
                            events,
                            completed_ms,
                            RuntimeEventKind::BaseSyncCancelled { page },
                        );
                        self.phase_event(events, completed_ms);
                        None
                    }
                    Err(error) => {
                        emit_failure(events, completed_ms, error, page);
                        self.coordinator.record_failure();
                        Some(
                            self.recover(page, None, backend, events, completed_ms)
                                .await,
                        )
                    }
                }
            }
            Err(error) => {
                emit_failure(events, completed_ms, error, page);
                self.coordinator.record_failure();
                Some(
                    self.recover(page, None, backend, events, completed_ms)
                        .await,
                )
            }
        }
    }

    async fn recover<B: DisplayBackend, S: EventSink>(
        &mut self,
        page: u32,
        sequence: Option<u16>,
        backend: &mut B,
        events: &mut S,
        now_ms: u64,
    ) -> RuntimeCompletion {
        self.phase_event(events, now_ms);
        emit(events, now_ms, RuntimeEventKind::RecoveryStarted { page });
        let result = async {
            backend.init_bw().await?;
            self.set_panel_mode(RuntimePanelMode::Bw, events, now_ms);
            backend.recover_bw(page).await
        }
        .await;
        match result {
            Ok(()) => {
                let from = self.current_page();
                self.coordinator.record_recovery_complete(page, now_ms);
                self.set_controller_state(ControllerRamState::BwBaseReady, events, now_ms);
                emit(events, now_ms, RuntimeEventKind::RecoveryCompleted { page });
                if from != page {
                    emit(
                        events,
                        now_ms,
                        RuntimeEventKind::PageDisplayed { from, page },
                    );
                }
                self.phase_event(events, now_ms);
                self.complete(sequence, RuntimeCompletionStatus::Ok, None, events, now_ms)
            }
            Err(error) => {
                emit_failure(events, now_ms, error, page);
                self.coordinator.record_failure();
                self.set_controller_state(ControllerRamState::NeedsFullBwInputs, events, now_ms);
                self.phase_event(events, now_ms);
                self.complete(
                    sequence,
                    RuntimeCompletionStatus::Error,
                    Some(error),
                    events,
                    now_ms,
                )
            }
        }
    }

    async fn run_probe<B: DisplayBackend, S: EventSink>(
        &mut self,
        kind: DisplayProbeKind,
        sequence: u16,
        backend: &mut B,
        events: &mut S,
        now_ms: u64,
    ) -> Option<RuntimeCompletion> {
        let page = self.current_page();
        emit(
            events,
            now_ms,
            RuntimeEventKind::ProbeStarted { sequence, kind },
        );
        let result = async {
            if kind == DisplayProbeKind::FullRefreshCurrent {
                backend.init_grayscale().await?;
            }
            backend.run_probe(kind, page).await
        }
        .await;
        self.set_controller_state(ControllerRamState::NeedsFullBwInputs, events, now_ms);
        Some(match result {
            Ok(()) => {
                emit(
                    events,
                    now_ms,
                    RuntimeEventKind::ProbeCompleted { sequence, kind },
                );
                self.complete(
                    Some(sequence),
                    RuntimeCompletionStatus::Ok,
                    None,
                    events,
                    now_ms,
                )
            }
            Err(error) => {
                emit_failure(events, now_ms, error, page);
                self.complete(
                    Some(sequence),
                    RuntimeCompletionStatus::Error,
                    Some(error),
                    events,
                    now_ms,
                )
            }
        })
    }

    fn complete<S: EventSink>(
        &self,
        sequence: Option<u16>,
        status: RuntimeCompletionStatus,
        error: Option<HalError>,
        events: &mut S,
        now_ms: u64,
    ) -> RuntimeCompletion {
        let completion = RuntimeCompletion {
            sequence,
            status,
            page: self.current_page(),
            error,
        };
        emit(events, now_ms, RuntimeEventKind::Completion(completion));
        completion
    }
    fn phase_event<S: EventSink>(&self, events: &mut S, now_ms: u64) {
        emit(
            events,
            now_ms,
            RuntimeEventKind::PhaseChanged(self.coordinator.phase()),
        );
    }
    fn set_panel_mode<S: EventSink>(
        &mut self,
        mode: RuntimePanelMode,
        events: &mut S,
        now_ms: u64,
    ) {
        self.panel_mode = mode;
        emit(events, now_ms, RuntimeEventKind::PanelModeChanged(mode));
    }
    fn set_controller_state<S: EventSink>(
        &mut self,
        state: ControllerRamState,
        events: &mut S,
        now_ms: u64,
    ) {
        self.controller_state = state;
        emit(
            events,
            now_ms,
            RuntimeEventKind::ControllerRamStateChanged(state),
        );
    }
}

fn request_sequence(request: DisplayRequest) -> Option<u16> {
    match request {
        DisplayRequest::Turn {
            completion_sequence,
            ..
        } => completion_sequence,
        DisplayRequest::Goto {
            completion_sequence,
            ..
        }
        | DisplayRequest::Probe {
            completion_sequence,
            ..
        } => Some(completion_sequence),
    }
}
fn emit<S: EventSink>(events: &mut S, timestamp_ms: u64, kind: RuntimeEventKind) {
    events.emit(RuntimeEvent { timestamp_ms, kind });
}
fn emit_failure<S: EventSink>(events: &mut S, timestamp_ms: u64, error: HalError, page: u32) {
    emit(
        events,
        timestamp_ms,
        RuntimeEventKind::DisplayFailure { error, page },
    );
}
