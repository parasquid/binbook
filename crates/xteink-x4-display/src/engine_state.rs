use crate::{
    engine::{emit, ControllerRamState, DisplayEngine},
    events::{
        CompletionStatus, DisplayCompletion, DisplayEventKind, DisplayPhase, EventSink, PanelMode,
    },
    DisplayError,
};

impl DisplayEngine {
    pub(crate) fn complete<S: EventSink>(
        &self,
        sequence: Option<u16>,
        status: CompletionStatus,
        error: Option<DisplayError>,
        events: &mut S,
        now: u64,
    ) -> DisplayCompletion {
        let completion = DisplayCompletion {
            sequence,
            status,
            page: self.current_page,
            error,
        };
        emit(events, now, DisplayEventKind::Completion(completion));
        completion
    }

    pub(crate) fn fail<S: EventSink>(
        &mut self,
        sequence: Option<u16>,
        error: DisplayError,
        page: u32,
        events: &mut S,
        now: u64,
    ) -> DisplayCompletion {
        emit(events, now, DisplayEventKind::Failure { page, error });
        self.recovery_attempted = true;
        self.set_controller(ControllerRamState::NeedsFullBwInputs, events, now);
        self.set_phase(DisplayPhase::Fault, events, now);
        self.complete(sequence, CompletionStatus::Error, Some(error), events, now)
    }

    pub(crate) fn set_phase<S: EventSink>(
        &mut self,
        phase: DisplayPhase,
        events: &mut S,
        now: u64,
    ) {
        self.phase = phase;
        emit(events, now, DisplayEventKind::PhaseChanged(phase));
    }

    pub(crate) fn set_mode<S: EventSink>(&mut self, mode: PanelMode, events: &mut S, now: u64) {
        if self.panel_mode != mode {
            self.panel_mode = mode;
            emit(events, now, DisplayEventKind::PanelModeChanged(mode));
        }
    }

    pub(crate) fn set_controller<S: EventSink>(
        &mut self,
        state: ControllerRamState,
        events: &mut S,
        now: u64,
    ) {
        if self.controller_state != state {
            self.controller_state = state;
            emit(events, now, DisplayEventKind::ControllerStateChanged(state));
        }
    }
}
