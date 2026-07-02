//! Diagnostic projection of application-local runtime events.

use binbook_diagnostic_protocol::{PanelModeCode, StatusPayload};

use crate::error::FirmwareError;
use crate::{
    diag::{DiagnosticSnapshot, PendingCommand},
    diag_log::{
        DiagEvent, DiagLog, EVT_BUSY_WAIT_END, EVT_BUSY_WAIT_START, EVT_BW_BASE_SYNC_CANCELLED,
        EVT_BW_BASE_SYNC_COMPLETE, EVT_BW_BASE_SYNC_START, EVT_CONTROLLER_RAM_STATE,
        EVT_DISPLAY_ERROR, EVT_DISPLAY_RECOVERY, EVT_DISPLAY_REQUEST_END,
        EVT_DISPLAY_REQUEST_START, EVT_GRAY_DELAY_CANCELLED, EVT_GRAY_OVERLAY_ACTIVATE,
        EVT_GRAY_OVERLAY_CANCELLED, EVT_GRAY_OVERLAY_COMPLETE, EVT_GRAY_OVERLAY_START,
        EVT_INPUT_DECISION, EVT_INPUT_TRANSITION, EVT_PAGE_METADATA_READ, EVT_PANEL_MODE,
        EVT_PLANE_ROW_FILL_SUMMARY, EVT_PLANE_SPI_WRITE_SUMMARY, EVT_PLANE_WRITE_END,
        EVT_PLANE_WRITE_START, EVT_REFRESH_PHASE, EVT_REFRESH_TRIGGER, EVT_RENDER_FAILURE,
        EVT_RENDER_START, EVT_RENDER_SUCCESS, EVT_REQUEST_ENQUEUE, EVT_REQUEST_RECEIVE,
        EVT_TURN_BOUNDARY_NOOP, EVT_TURN_DEQUEUED, EVT_TURN_DROPPED, EVT_TURN_QUEUED,
        EVT_TURN_STARTED, EVT_WAVEFORM_SELECTED, LEVEL_DEBUG, LEVEL_ERROR, LEVEL_INFO, SUB_DISPLAY,
        SUB_INPUT, SUB_NAV, SUB_SERIAL, SUB_SYSTEM,
    },
    runtime_engine::{
        RuntimeCompletion, RuntimeCompletionStatus, RuntimeEvent, RuntimeEventKind,
        RuntimePanelMode,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReserveError {
    DuplicateSequence,
    Full,
    EnqueueFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommittedCompletion {
    pub pending: PendingCommand,
    pub completion: RuntimeCompletion,
    pub snapshot: DiagnosticSnapshot,
    pub log_sequence: Option<u32>,
}

pub struct RuntimeAggregator<const PENDING: usize, const LOG: usize> {
    snapshot: DiagnosticSnapshot,
    pending: [Option<PendingCommand>; PENDING],
    pending_len: usize,
    log: DiagLog<LOG>,
}

impl<const PENDING: usize, const LOG: usize> RuntimeAggregator<PENDING, LOG> {
    pub const fn new(snapshot: DiagnosticSnapshot) -> Self {
        Self {
            snapshot,
            pending: [None; PENDING],
            pending_len: 0,
            log: DiagLog::new(),
        }
    }

    pub fn reserve(&mut self, pending: PendingCommand) -> Result<(), ReserveError> {
        let sequence = pending.header.sequence;
        if self
            .pending
            .iter()
            .flatten()
            .any(|item| item.header.sequence == sequence)
        {
            return Err(ReserveError::DuplicateSequence);
        }
        let Some(slot) = self.pending.iter_mut().find(|slot| slot.is_none()) else {
            return Err(ReserveError::Full);
        };
        *slot = Some(pending);
        self.pending_len += 1;
        Ok(())
    }

    pub fn cancel(&mut self, sequence: u16) -> Option<PendingCommand> {
        let slot = self
            .pending
            .iter_mut()
            .find(|slot| slot.map(|item| item.header.sequence) == Some(sequence))?;
        let pending = slot.take();
        if pending.is_some() {
            self.pending_len -= 1;
        }
        pending
    }

    pub fn reserve_and_enqueue(
        &mut self,
        pending: PendingCommand,
        enqueue: impl FnOnce() -> bool,
    ) -> Result<(), ReserveError> {
        let sequence = pending.header.sequence;
        self.reserve(pending)?;
        if enqueue() {
            Ok(())
        } else {
            self.cancel(sequence);
            Err(ReserveError::EnqueueFailed)
        }
    }

    pub fn pending_len(&self) -> usize {
        self.pending_len
    }

    pub fn snapshot(&self) -> DiagnosticSnapshot {
        self.snapshot
    }

    pub fn set_protocol_error_count(&mut self, count: u32) {
        self.snapshot.protocol_error_count = count;
    }

    pub fn status_payload(&self) -> StatusPayload {
        self.snapshot.status_payload()
    }

    pub fn log(&self) -> &DiagLog<LOG> {
        &self.log
    }

    pub fn clear_log(&mut self) -> u32 {
        let next = self.log.next_sequence();
        self.log.clear();
        self.snapshot.dropped_log_count = 0;
        next
    }

    pub fn commit(&mut self, event: RuntimeEvent) -> Option<CommittedCompletion> {
        let tick_ms = event.timestamp_ms.min(u32::MAX as u64) as u32;
        let mut completion = None;
        match event.kind {
            RuntimeEventKind::FirmwareStarted { page_count } => {
                self.push_with_subsystem(
                    tick_ms,
                    DiagEvent {
                        level: LEVEL_INFO,
                        subsystem: SUB_SYSTEM,
                        event: crate::diag_log::EVT_FIRMWARE_STARTED,
                        arg0: page_count as i32,
                        arg1: 0,
                        arg2: 0,
                    },
                );
            }
            RuntimeEventKind::ProtocolCommand { opcode, sequence } => {
                self.push_with_subsystem(
                    tick_ms,
                    DiagEvent {
                        level: LEVEL_INFO,
                        subsystem: SUB_SERIAL,
                        event: crate::diag_log::EVT_CMD_RECEIPT,
                        arg0: opcode as i32,
                        arg1: sequence as i32,
                        arg2: 0,
                    },
                );
            }
            RuntimeEventKind::PhaseChanged(phase) => {
                self.push(tick_ms, LEVEL_INFO, EVT_REFRESH_PHASE, phase as i32, 0, 0);
            }
            RuntimeEventKind::GrayDelayCancelled { page } => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_GRAY_DELAY_CANCELLED,
                    page as i32,
                    0,
                    0,
                );
            }
            RuntimeEventKind::PanelModeChanged(mode) => {
                self.snapshot.panel_mode = panel_mode(mode);
                self.push(tick_ms, LEVEL_INFO, EVT_PANEL_MODE, mode as i32, 0, 0);
            }
            RuntimeEventKind::ControllerRamStateChanged(state) => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_CONTROLLER_RAM_STATE,
                    state as i32,
                    0,
                    0,
                );
            }
            RuntimeEventKind::TurnQueued { sequence, turn } => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_TURN_QUEUED,
                    sequence.map(i32::from).unwrap_or(-1),
                    turn as i32,
                    0,
                );
            }
            RuntimeEventKind::RequestEnqueue {
                kind,
                sequence,
                status,
            } => {
                self.push_with_subsystem(
                    tick_ms,
                    DiagEvent {
                        level: if status == crate::runtime_engine::RequestEnqueueStatus::Ok {
                            LEVEL_INFO
                        } else {
                            LEVEL_ERROR
                        },
                        subsystem: SUB_INPUT,
                        event: EVT_REQUEST_ENQUEUE,
                        arg0: kind.log_code(),
                        arg1: sequence.map(i32::from).unwrap_or(-1),
                        arg2: status.log_code(),
                    },
                );
            }
            RuntimeEventKind::RequestReceive {
                kind,
                sequence,
                queue_age_ms,
            } => {
                self.push_with_subsystem(
                    tick_ms,
                    DiagEvent {
                        level: LEVEL_INFO,
                        subsystem: SUB_NAV,
                        event: EVT_REQUEST_RECEIVE,
                        arg0: kind.log_code(),
                        arg1: sequence.map(i32::from).unwrap_or(-1),
                        arg2: queue_age_ms
                            .map(|value| value.min(i32::MAX as u32) as i32)
                            .unwrap_or(-1),
                    },
                );
            }
            RuntimeEventKind::DisplayRequestStart {
                kind,
                current_page,
                target_page,
            } => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_DISPLAY_REQUEST_START,
                    kind.log_code(),
                    current_page as i32,
                    target_page.map(|page| page as i32).unwrap_or(-1),
                );
            }
            RuntimeEventKind::DisplayRequestEnd {
                kind,
                duration_ms,
                status,
            } => {
                self.push(
                    tick_ms,
                    if status == RuntimeCompletionStatus::Ok {
                        LEVEL_INFO
                    } else {
                        LEVEL_ERROR
                    },
                    EVT_DISPLAY_REQUEST_END,
                    kind.log_code(),
                    duration_ms.min(i32::MAX as u32) as i32,
                    if status == RuntimeCompletionStatus::Ok {
                        0
                    } else {
                        1
                    },
                );
            }
            RuntimeEventKind::BusyWaitStart {
                site,
                timeout_ms,
                busy_state,
            } => {
                self.push(
                    tick_ms,
                    LEVEL_DEBUG,
                    EVT_BUSY_WAIT_START,
                    site.log_code(),
                    timeout_ms.min(i32::MAX as u32) as i32,
                    busy_state.map(i32::from).unwrap_or(-1),
                );
            }
            RuntimeEventKind::BusyWaitEnd {
                site,
                elapsed_ms,
                status,
            } => {
                self.push(
                    tick_ms,
                    if status == crate::runtime_engine::BusyWaitStatus::Ready {
                        LEVEL_DEBUG
                    } else {
                        LEVEL_ERROR
                    },
                    EVT_BUSY_WAIT_END,
                    site.log_code(),
                    elapsed_ms.min(i32::MAX as u32) as i32,
                    status.log_code(),
                );
            }
            RuntimeEventKind::PageMetadataRead {
                from,
                target,
                duration_ms,
            } => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_PAGE_METADATA_READ,
                    from as i32,
                    target as i32,
                    duration_ms.min(i32::MAX as u32) as i32,
                );
            }
            RuntimeEventKind::PlaneWriteStart {
                role,
                ram_target,
                plane_bytes,
            } => {
                self.push(
                    tick_ms,
                    LEVEL_DEBUG,
                    EVT_PLANE_WRITE_START,
                    role,
                    ram_target,
                    plane_bytes.min(i32::MAX as u32) as i32,
                );
            }
            RuntimeEventKind::PlaneRowFillSummary {
                role,
                duration_ms,
                row_count,
            } => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_PLANE_ROW_FILL_SUMMARY,
                    role,
                    duration_ms.min(i32::MAX as u32) as i32,
                    row_count.min(i32::MAX as u32) as i32,
                );
            }
            RuntimeEventKind::PlaneSpiWriteSummary {
                role,
                duration_ms,
                bytes_written,
            } => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_PLANE_SPI_WRITE_SUMMARY,
                    role,
                    duration_ms.min(i32::MAX as u32) as i32,
                    bytes_written.min(i32::MAX as u32) as i32,
                );
            }
            RuntimeEventKind::PlaneWriteEnd {
                role,
                duration_ms,
                status,
            } => {
                self.push(
                    tick_ms,
                    if status == 0 { LEVEL_INFO } else { LEVEL_ERROR },
                    EVT_PLANE_WRITE_END,
                    role,
                    duration_ms.min(i32::MAX as u32) as i32,
                    status,
                );
            }
            RuntimeEventKind::RefreshTrigger {
                mode,
                duration_ms,
                status,
            } => {
                self.push(
                    tick_ms,
                    if status == 0 { LEVEL_INFO } else { LEVEL_ERROR },
                    EVT_REFRESH_TRIGGER,
                    mode,
                    duration_ms.min(i32::MAX as u32) as i32,
                    status,
                );
            }
            RuntimeEventKind::InputTransition { ch1, ch2, observed } => {
                self.push_with_subsystem(
                    tick_ms,
                    DiagEvent {
                        level: LEVEL_INFO,
                        subsystem: SUB_INPUT,
                        event: EVT_INPUT_TRANSITION,
                        arg0: i32::from(ch1),
                        arg1: i32::from(ch2),
                        arg2: observed.map(|button| button as i32).unwrap_or(-1),
                    },
                );
            }
            RuntimeEventKind::InputDecision {
                observed,
                decision,
                elapsed_ms,
            } => {
                let decision_code = match decision {
                    crate::input::InputDecision::Press(_) => 0,
                    crate::input::InputDecision::Released => 1,
                    crate::input::InputDecision::SuppressedByCooldown { .. } => 2,
                    crate::input::InputDecision::Unchanged => -1,
                };
                self.push_with_subsystem(
                    tick_ms,
                    DiagEvent {
                        level: LEVEL_INFO,
                        subsystem: SUB_INPUT,
                        event: EVT_INPUT_DECISION,
                        arg0: observed.map(|button| button as i32).unwrap_or(-1),
                        arg1: decision_code,
                        arg2: elapsed_ms.min(i32::MAX as u32) as i32,
                    },
                );
            }
            RuntimeEventKind::TurnStarted {
                sequence,
                from,
                target,
            } => {
                self.push_with_subsystem(
                    tick_ms,
                    DiagEvent {
                        level: LEVEL_INFO,
                        subsystem: SUB_NAV,
                        event: EVT_TURN_STARTED,
                        arg0: sequence.map(i32::from).unwrap_or(-1),
                        arg1: from as i32,
                        arg2: target as i32,
                    },
                );
            }
            RuntimeEventKind::TurnBoundaryNoop {
                sequence,
                page,
                turn,
            } => {
                self.push_with_subsystem(
                    tick_ms,
                    DiagEvent {
                        level: LEVEL_INFO,
                        subsystem: SUB_NAV,
                        event: EVT_TURN_BOUNDARY_NOOP,
                        arg0: sequence.map(i32::from).unwrap_or(-1),
                        arg1: page as i32,
                        arg2: turn as i32,
                    },
                );
            }
            RuntimeEventKind::PageDisplayed { from, page } => {
                self.snapshot.current_page = page;
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    crate::diag_log::EVT_PAGE_TURN,
                    from as i32,
                    page as i32,
                    0,
                );
            }
            RuntimeEventKind::GrayStarted { page } => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_GRAY_OVERLAY_START,
                    page as i32,
                    0,
                    0,
                );
            }
            RuntimeEventKind::GrayCancelled { page } => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_GRAY_OVERLAY_CANCELLED,
                    page as i32,
                    0,
                    0,
                );
            }
            RuntimeEventKind::GrayActivated { page } => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_GRAY_OVERLAY_ACTIVATE,
                    page as i32,
                    0,
                    0,
                );
            }
            RuntimeEventKind::WaveformSelected {
                waveform_hint,
                lut_revision,
            } => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_WAVEFORM_SELECTED,
                    waveform_hint as i32,
                    lut_revision as i32,
                    0,
                );
            }
            RuntimeEventKind::GrayCompleted { page } => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_GRAY_OVERLAY_COMPLETE,
                    page as i32,
                    0,
                    0,
                );
            }
            RuntimeEventKind::BaseSyncStarted { page } => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_BW_BASE_SYNC_START,
                    page as i32,
                    0,
                    0,
                );
            }
            RuntimeEventKind::BaseSyncCancelled { page } => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_BW_BASE_SYNC_CANCELLED,
                    page as i32,
                    0,
                    -1,
                );
            }
            RuntimeEventKind::BaseSyncCompleted { page } => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_BW_BASE_SYNC_COMPLETE,
                    page as i32,
                    0,
                    0,
                );
            }
            RuntimeEventKind::RecoveryStarted { page } => {
                self.push(tick_ms, LEVEL_INFO, EVT_DISPLAY_RECOVERY, page as i32, 0, 0);
            }
            RuntimeEventKind::RecoveryCompleted { page } => {
                self.push(tick_ms, LEVEL_INFO, EVT_DISPLAY_RECOVERY, page as i32, 1, 0);
            }
            RuntimeEventKind::ProbeStarted { sequence, kind } => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_RENDER_START,
                    sequence as i32,
                    kind as i32,
                    0,
                );
            }
            RuntimeEventKind::ProbeCompleted { sequence, kind } => {
                self.push(
                    tick_ms,
                    LEVEL_INFO,
                    EVT_RENDER_SUCCESS,
                    sequence as i32,
                    kind as i32,
                    0,
                );
            }
            RuntimeEventKind::TurnDropped { turn } => {
                self.push(tick_ms, LEVEL_ERROR, EVT_TURN_DROPPED, turn as i32, 0, 0);
            }
            RuntimeEventKind::DisplayFailure { error, page } => {
                let code = hal_error_code(error);
                self.snapshot.last_error = code;
                self.push(
                    tick_ms,
                    LEVEL_ERROR,
                    EVT_DISPLAY_ERROR,
                    code,
                    page as i32,
                    0,
                );
            }
            RuntimeEventKind::Completion(value) => {
                self.snapshot.current_page = value.page;
                if let Some(error) = value.error {
                    self.snapshot.last_error = hal_error_code(error);
                }
                let event_code = if value.status == RuntimeCompletionStatus::Ok {
                    EVT_TURN_DEQUEUED
                } else {
                    EVT_RENDER_FAILURE
                };
                let log_sequence = self.push(
                    tick_ms,
                    if value.status == RuntimeCompletionStatus::Ok {
                        LEVEL_INFO
                    } else {
                        LEVEL_ERROR
                    },
                    event_code,
                    value.sequence.map(i32::from).unwrap_or(-1),
                    value.page as i32,
                    0,
                );
                if let Some(sequence) = value.sequence {
                    if let Some(pending) = self.cancel(sequence) {
                        completion = Some(CommittedCompletion {
                            pending,
                            completion: value,
                            snapshot: self.snapshot,
                            log_sequence: Some(log_sequence),
                        });
                    }
                }
            }
            RuntimeEventKind::MenuTransition => {}
            RuntimeEventKind::ModeChange { .. } => {}
            RuntimeEventKind::BookOpen { .. } => {}
        }
        self.snapshot.dropped_log_count = self.log.dropped_records();
        completion
    }

    fn push(
        &mut self,
        tick_ms: u32,
        level: u8,
        event: u16,
        arg0: i32,
        arg1: i32,
        arg2: i32,
    ) -> u32 {
        let subsystem = match event {
            EVT_TURN_QUEUED | EVT_TURN_DEQUEUED | EVT_TURN_DROPPED => SUB_INPUT,
            crate::diag_log::EVT_PAGE_TURN => SUB_NAV,
            _ => SUB_DISPLAY,
        };
        self.push_with_subsystem(
            tick_ms,
            DiagEvent {
                level,
                subsystem,
                event,
                arg0,
                arg1,
                arg2,
            },
        )
    }

    fn push_with_subsystem(&mut self, tick_ms: u32, event: DiagEvent) -> u32 {
        self.log.push(tick_ms, event)
    }
}

fn panel_mode(mode: RuntimePanelMode) -> PanelModeCode {
    match mode {
        RuntimePanelMode::Unknown => PanelModeCode::Unknown,
        RuntimePanelMode::Grayscale => PanelModeCode::Grayscale,
        RuntimePanelMode::Bw => PanelModeCode::Bw,
    }
}

fn hal_error_code(error: FirmwareError) -> i32 {
    match error {
        FirmwareError::Spi => -1,
        FirmwareError::Gpio => -2,
        FirmwareError::Storage => -3,
        FirmwareError::Timeout => -4,
        FirmwareError::InvalidParameter => -5,
    }
}
