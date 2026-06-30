use crate::{
    engine::{emit, ControllerRamState, DisplayBackend, DisplayEngine},
    events::{
        CompletionStatus, DisplayCompletion, DisplayEventKind, DisplayPhase, EventSink,
        OperationOutcome, PageTurn, PanelMode, GRAY_SETTLE_DELAY_MS,
    },
    probes::ProbeKind,
    DisplayError,
};

impl DisplayEngine {
    pub(crate) async fn navigate<B: DisplayBackend, S: EventSink>(
        &mut self,
        target: u32,
        sequence: Option<u16>,
        boundary: Option<PageTurn>,
        backend: &mut B,
        events: &mut S,
        now: u64,
    ) -> DisplayCompletion {
        let from = self.current_page;
        if target >= self.page_count {
            return self.complete(
                sequence,
                CompletionStatus::Error,
                Some(DisplayError::InvalidPage),
                events,
                now,
            );
        }
        if self.phase == DisplayPhase::GrayDelay {
            self.gray_deadline = None;
            emit(
                events,
                now,
                DisplayEventKind::GrayDelayCancelled { page: from },
            );
        }
        if target == from {
            if let Some(turn) = boundary {
                emit(
                    events,
                    now,
                    DisplayEventKind::TurnBoundaryNoop {
                        sequence,
                        page: from,
                        turn,
                    },
                );
            }
            return self.complete(sequence, CompletionStatus::Ok, None, events, now);
        }
        emit(
            events,
            now,
            DisplayEventKind::TurnStarted {
                sequence,
                from,
                target,
            },
        );
        self.set_phase(DisplayPhase::BwRefreshing, events, now);
        match backend.render_bw(from, target).await {
            Ok(()) => {
                let completed = backend.timestamp_ms().unwrap_or(now);
                self.current_page = target;
                self.recovery_attempted = false;
                self.set_mode(PanelMode::Bw, events, completed);
                self.set_controller(ControllerRamState::BwBaseReady, events, completed);
                self.gray_deadline = Some(completed + GRAY_SETTLE_DELAY_MS);
                emit(
                    events,
                    completed,
                    DisplayEventKind::PageDisplayed { from, page: target },
                );
                self.set_phase(DisplayPhase::GrayDelay, events, completed);
                self.complete(sequence, CompletionStatus::Ok, None, events, completed)
            }
            Err(error) => {
                self.recover(target, sequence, error, backend, events, now)
                    .await
            }
        }
    }

    pub(crate) async fn overlay<B: DisplayBackend, S: EventSink>(
        &mut self,
        backend: &mut B,
        events: &mut S,
        now: u64,
    ) -> Option<DisplayCompletion> {
        let page = self.current_page;
        let epoch = backend.request_epoch();
        self.set_phase(DisplayPhase::GrayRefreshing, events, now);
        emit(events, now, DisplayEventKind::GrayStarted { page });
        match backend.render_grayscale(page, epoch).await {
            Ok(OperationOutcome::Cancelled) => {
                self.set_controller(ControllerRamState::NeedsFullBwInputs, events, now);
                emit(events, now, DisplayEventKind::GrayCancelled { page });
                self.set_phase(DisplayPhase::BwReady, events, now);
                None
            }
            Ok(OperationOutcome::Completed) => {
                self.base_sync(page, epoch, backend, events, now).await
            }
            Err(error) => Some(self.recover(page, None, error, backend, events, now).await),
        }
    }

    async fn base_sync<B: DisplayBackend, S: EventSink>(
        &mut self,
        page: u32,
        epoch: u32,
        backend: &mut B,
        events: &mut S,
        now: u64,
    ) -> Option<DisplayCompletion> {
        self.set_mode(PanelMode::Grayscale, events, now);
        self.set_controller(ControllerRamState::GrayOverlayResident, events, now);
        emit(events, now, DisplayEventKind::GrayCompleted { page });
        self.set_phase(DisplayPhase::BaseSync, events, now);
        if backend.request_epoch() != epoch {
            self.set_controller(ControllerRamState::NeedsFullBwInputs, events, now);
            self.set_phase(DisplayPhase::BwReady, events, now);
            return None;
        }
        emit(events, now, DisplayEventKind::BaseSyncStarted { page });
        self.set_controller(ControllerRamState::BaseSyncInProgress, events, now);
        match backend.sync_bw_base(page, epoch).await {
            Ok(OperationOutcome::Completed) => {
                self.set_controller(ControllerRamState::BwBaseReady, events, now);
                emit(events, now, DisplayEventKind::BaseSyncCompleted { page });
                self.set_phase(DisplayPhase::BwReady, events, now);
                None
            }
            Ok(OperationOutcome::Cancelled) => {
                self.set_controller(ControllerRamState::NeedsFullBwInputs, events, now);
                emit(events, now, DisplayEventKind::BaseSyncCancelled { page });
                self.set_phase(DisplayPhase::BwReady, events, now);
                None
            }
            Err(error) => Some(self.recover(page, None, error, backend, events, now).await),
        }
    }

    async fn recover<B: DisplayBackend, S: EventSink>(
        &mut self,
        page: u32,
        sequence: Option<u16>,
        error: DisplayError,
        backend: &mut B,
        events: &mut S,
        now: u64,
    ) -> DisplayCompletion {
        emit(events, now, DisplayEventKind::Failure { page, error });
        if self.recovery_attempted {
            self.set_phase(DisplayPhase::Fault, events, now);
            return self.complete(sequence, CompletionStatus::Error, Some(error), events, now);
        }
        self.recovery_attempted = true;
        self.set_phase(DisplayPhase::Recovering, events, now);
        emit(events, now, DisplayEventKind::RecoveryStarted { page });
        match async {
            backend.init_bw().await?;
            backend.recover_bw(page).await
        }
        .await
        {
            Ok(()) => {
                let from = self.current_page;
                self.current_page = page;
                self.set_mode(PanelMode::Bw, events, now);
                self.set_controller(ControllerRamState::BwBaseReady, events, now);
                emit(events, now, DisplayEventKind::RecoveryCompleted { page });
                if from != page {
                    emit(events, now, DisplayEventKind::PageDisplayed { from, page });
                }
                self.set_phase(DisplayPhase::BwReady, events, now);
                self.complete(sequence, CompletionStatus::Ok, None, events, now)
            }
            Err(recovery_error) => self.fail(sequence, recovery_error, page, events, now),
        }
    }

    pub(crate) async fn probe<B: DisplayBackend, S: EventSink>(
        &mut self,
        kind: ProbeKind,
        sequence: u16,
        backend: &mut B,
        events: &mut S,
        now: u64,
    ) -> DisplayCompletion {
        emit(
            events,
            now,
            DisplayEventKind::ProbeStarted { sequence, kind },
        );
        let result = async {
            if kind == ProbeKind::FullRefreshCurrent {
                backend.init_grayscale().await?;
            }
            backend.run_probe(kind, self.current_page).await
        }
        .await;
        self.set_controller(ControllerRamState::NeedsFullBwInputs, events, now);
        match result {
            Ok(()) => {
                emit(
                    events,
                    now,
                    DisplayEventKind::ProbeCompleted { sequence, kind },
                );
                self.complete(Some(sequence), CompletionStatus::Ok, None, events, now)
            }
            Err(error) => self.fail(Some(sequence), error, self.current_page, events, now),
        }
    }
}
