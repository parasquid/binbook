use crate::{
    events::{
        CompletionStatus, DisplayCompletion, DisplayEvent, DisplayEventKind, DisplayPhase,
        DisplayRequest, EventSink, OperationOutcome, PageRefreshMode, PageTurn, PanelMode,
        GRAY_SETTLE_DELAY_MS,
    },
    probes::ProbeKind,
    DisplayError, DisplayResult,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerRamState {
    BwBaseReady,
    GrayOverlayResident,
    BaseSyncInProgress,
    NeedsFullBwInputs,
}

#[allow(async_fn_in_trait)]
pub trait DisplayBackend {
    fn timestamp_ms(&self) -> Option<u64> {
        None
    }
    fn request_epoch(&self) -> u32 {
        0
    }
    async fn init_grayscale(&mut self) -> DisplayResult<()> {
        Ok(())
    }
    async fn init_bw(&mut self) -> DisplayResult<()>;
    async fn render_bw(
        &mut self,
        sequence: Option<u16>,
        from: u32,
        target: u32,
    ) -> DisplayResult<()>;
    async fn render_preview(
        &mut self,
        sequence: Option<u16>,
        from: u32,
        target: u32,
    ) -> DisplayResult<()>;
    async fn cleanup_preview_bw(
        &mut self,
        sequence: Option<u16>,
        from: u32,
        target: u32,
    ) -> DisplayResult<()>;
    async fn render_grayscale(&mut self, page: u32, epoch: u32) -> DisplayResult<OperationOutcome>;
    async fn sync_bw_base(&mut self, page: u32, epoch: u32) -> DisplayResult<OperationOutcome>;
    async fn recover_bw(&mut self, page: u32) -> DisplayResult<()>;
    async fn run_probe(&mut self, kind: ProbeKind, page: u32) -> DisplayResult<()>;
}

pub struct DisplayEngine {
    pub(crate) page_count: u32,
    pub(crate) current_page: u32,
    pub(crate) phase: DisplayPhase,
    pub(crate) panel_mode: PanelMode,
    pub(crate) refresh_mode: PageRefreshMode,
    pub(crate) controller_state: ControllerRamState,
    pub(crate) gray_deadline: Option<u64>,
    pub(crate) preview_cleanup: Option<PreviewCleanup>,
    pub(crate) recovery_attempted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreviewCleanup {
    pub(crate) sequence: Option<u16>,
    pub(crate) from: u32,
    pub(crate) target: u32,
    pub(crate) deadline_ms: u64,
    pub(crate) epoch: u32,
}

impl DisplayEngine {
    #[must_use]
    pub const fn new(page_count: u32) -> Self {
        Self::new_with_refresh_mode(page_count, PageRefreshMode::Balanced)
    }

    #[must_use]
    pub const fn new_with_refresh_mode(page_count: u32, refresh_mode: PageRefreshMode) -> Self {
        Self {
            page_count,
            current_page: 0,
            phase: DisplayPhase::Recovering,
            panel_mode: PanelMode::Unknown,
            refresh_mode,
            controller_state: ControllerRamState::NeedsFullBwInputs,
            gray_deadline: None,
            preview_cleanup: None,
            recovery_attempted: false,
        }
    }

    pub const fn current_page(&self) -> u32 {
        self.current_page
    }
    pub const fn phase(&self) -> DisplayPhase {
        self.phase
    }
    pub const fn panel_mode(&self) -> PanelMode {
        self.panel_mode
    }
    pub const fn refresh_mode(&self) -> PageRefreshMode {
        self.refresh_mode
    }
    pub const fn controller_state(&self) -> ControllerRamState {
        self.controller_state
    }
    pub const fn faulted(&self) -> bool {
        matches!(self.phase, DisplayPhase::Fault)
    }
    pub const fn recovery_required(&self) -> bool {
        self.recovery_attempted && !self.faulted()
    }
    pub const fn target_for(&self, turn: PageTurn) -> u32 {
        apply_page_turn(self.current_page, self.page_count, turn)
    }

    pub fn commit_page(&mut self, page: u32) -> DisplayResult<()> {
        if page >= self.page_count {
            return Err(DisplayError::InvalidPage);
        }
        self.current_page = page;
        self.phase = DisplayPhase::BwReady;
        self.controller_state = ControllerRamState::BwBaseReady;
        self.gray_deadline = None;
        self.preview_cleanup = None;
        Ok(())
    }

    pub fn begin_overlay(&mut self) {
        self.preview_cleanup = None;
        self.phase = DisplayPhase::GrayRefreshing;
        self.controller_state = ControllerRamState::GrayOverlayResident;
    }
    pub fn cancel_overlay(&mut self) {
        self.gray_deadline = None;
        self.preview_cleanup = None;
        self.phase = DisplayPhase::BwReady;
        self.controller_state = ControllerRamState::NeedsFullBwInputs;
    }
    pub fn begin_base_sync(&mut self) {
        self.phase = DisplayPhase::BaseSync;
        self.controller_state = ControllerRamState::BaseSyncInProgress;
    }
    pub fn cancel_base_sync(&mut self) {
        self.phase = DisplayPhase::BwReady;
        self.controller_state = ControllerRamState::NeedsFullBwInputs;
    }
    pub fn record_failure(&mut self) {
        self.controller_state = ControllerRamState::NeedsFullBwInputs;
        if self.recovery_attempted {
            self.phase = DisplayPhase::Fault;
        } else {
            self.recovery_attempted = true;
            self.phase = DisplayPhase::Recovering;
        }
    }

    pub async fn initialize<B: DisplayBackend, S: EventSink>(
        &mut self,
        backend: &mut B,
        events: &mut S,
        now: u64,
    ) -> Option<DisplayCompletion> {
        self.set_phase(DisplayPhase::Recovering, events, now);
        emit(events, now, DisplayEventKind::RecoveryStarted { page: 0 });
        let result = async {
            backend.init_bw().await?;
            backend.recover_bw(0).await
        }
        .await;
        let completed = backend.timestamp_ms().unwrap_or(now);
        match result {
            Ok(()) => {
                self.current_page = 0;
                self.set_mode(PanelMode::Bw, events, completed);
                self.set_controller(ControllerRamState::BwBaseReady, events, completed);
                self.gray_deadline = Some(completed + GRAY_SETTLE_DELAY_MS);
                emit(
                    events,
                    completed,
                    DisplayEventKind::RecoveryCompleted { page: 0 },
                );
                self.set_phase(DisplayPhase::GrayDelay, events, completed);
                None
            }
            Err(error) => Some(self.fail(None, error, 0, events, completed)),
        }
    }

    pub async fn request<B: DisplayBackend, S: EventSink>(
        &mut self,
        request: DisplayRequest,
        backend: &mut B,
        events: &mut S,
        now: u64,
    ) -> DisplayCompletion {
        if self.faulted() {
            return self.complete(
                sequence(request),
                CompletionStatus::Error,
                Some(DisplayError::InvalidState),
                events,
                now,
            );
        }
        match request {
            DisplayRequest::Probe { kind, sequence } => {
                self.probe(kind, sequence, backend, events, now).await
            }
            DisplayRequest::Turn { turn, sequence } => {
                let target = self.target_for(turn);
                match self.refresh_mode {
                    PageRefreshMode::Balanced => {
                        self.navigate(target, sequence, Some(turn), backend, events, now)
                            .await
                    }
                    PageRefreshMode::Fast => {
                        self.preview_navigate(target, sequence, Some(turn), backend, events, now)
                            .await
                    }
                }
            }
            DisplayRequest::Goto { page, sequence } => match self.refresh_mode {
                PageRefreshMode::Balanced => {
                    self.navigate(page, Some(sequence), None, backend, events, now)
                        .await
                }
                PageRefreshMode::Fast => {
                    self.preview_navigate(page, Some(sequence), None, backend, events, now)
                        .await
                }
            },
            DisplayRequest::SetRefreshMode { mode, sequence } => {
                self.refresh_mode = mode;
                emit(events, now, DisplayEventKind::RefreshModeChanged(mode));
                self.complete(sequence, CompletionStatus::Ok, None, events, now)
            }
        }
    }

    pub async fn advance<B: DisplayBackend, S: EventSink>(
        &mut self,
        backend: &mut B,
        events: &mut S,
        now: u64,
    ) -> Option<DisplayCompletion> {
        if self.phase == DisplayPhase::PreviewDelay {
            if now
                < self
                    .preview_cleanup
                    .map(|cleanup| cleanup.deadline_ms)
                    .unwrap_or(u64::MAX)
            {
                return None;
            }
            return Some(self.cleanup_preview(backend, events, now).await).flatten();
        }
        if self.phase == DisplayPhase::GrayDelay && now >= self.gray_deadline.unwrap_or(u64::MAX) {
            return Some(self.overlay(backend, events, now).await).flatten();
        }
        None
    }
}

#[must_use]
pub const fn apply_page_turn(current: u32, count: u32, turn: PageTurn) -> u32 {
    if count == 0 {
        return 0;
    }
    match turn {
        PageTurn::Next if current < count - 1 => current + 1,
        PageTurn::Next => count - 1,
        PageTurn::Previous => current.saturating_sub(1),
        PageTurn::First => 0,
        PageTurn::Last => count - 1,
    }
}

fn sequence(request: DisplayRequest) -> Option<u16> {
    match request {
        DisplayRequest::Turn { sequence, .. } | DisplayRequest::SetRefreshMode { sequence, .. } => {
            sequence
        }
        DisplayRequest::Goto { sequence, .. } | DisplayRequest::Probe { sequence, .. } => {
            Some(sequence)
        }
    }
}

pub(crate) fn emit<S: EventSink>(events: &mut S, timestamp_ms: u64, kind: DisplayEventKind) {
    events.emit(DisplayEvent { timestamp_ms, kind });
}
