use crate::error::FirmwareError;
use crate::{
    async_refresh::{DisplayProbeKind, RefreshPhase},
    input::{Button, InputDecision, PageTurn},
};

pub use xteink_x4_display::engine::ControllerRamState;
pub type RuntimePanelMode = xteink_x4_display::events::PanelMode;

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
    pub error: Option<FirmwareError>,
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
    InputTransition {
        ch1: u16,
        ch2: u16,
        observed: Option<Button>,
    },
    InputDecision {
        observed: Option<Button>,
        decision: InputDecision,
        elapsed_ms: u32,
    },
    TurnStarted {
        sequence: Option<u16>,
        from: u32,
        target: u32,
    },
    TurnBoundaryNoop {
        sequence: Option<u16>,
        page: u32,
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
        error: FirmwareError,
        page: u32,
    },
    Completion(RuntimeCompletion),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeEvent {
    pub timestamp_ms: u64,
    pub kind: RuntimeEventKind,
}

#[must_use]
pub fn map_display_event(event: xteink_x4_display::events::DisplayEvent) -> RuntimeEvent {
    use xteink_x4_display::events::DisplayEventKind as Source;
    let kind = match event.kind {
        Source::PhaseChanged(value) => RuntimeEventKind::PhaseChanged(map_phase(value)),
        Source::PanelModeChanged(value) => RuntimeEventKind::PanelModeChanged(value),
        Source::ControllerStateChanged(value) => RuntimeEventKind::ControllerRamStateChanged(value),
        Source::TurnStarted {
            sequence,
            from,
            target,
        } => RuntimeEventKind::TurnStarted {
            sequence,
            from,
            target,
        },
        Source::TurnBoundaryNoop {
            sequence,
            page,
            turn,
        } => RuntimeEventKind::TurnBoundaryNoop {
            sequence,
            page,
            turn: map_turn(turn),
        },
        Source::GrayDelayCancelled { page } => RuntimeEventKind::GrayDelayCancelled { page },
        Source::PageDisplayed { from, page } => RuntimeEventKind::PageDisplayed { from, page },
        Source::GrayStarted { page } => RuntimeEventKind::GrayStarted { page },
        Source::GrayCancelled { page } => RuntimeEventKind::GrayCancelled { page },
        Source::GrayCompleted { page } => RuntimeEventKind::GrayCompleted { page },
        Source::BaseSyncStarted { page } => RuntimeEventKind::BaseSyncStarted { page },
        Source::BaseSyncCancelled { page } => RuntimeEventKind::BaseSyncCancelled { page },
        Source::BaseSyncCompleted { page } => RuntimeEventKind::BaseSyncCompleted { page },
        Source::RecoveryStarted { page } => RuntimeEventKind::RecoveryStarted { page },
        Source::RecoveryCompleted { page } => RuntimeEventKind::RecoveryCompleted { page },
        Source::ProbeStarted { sequence, kind } => RuntimeEventKind::ProbeStarted {
            sequence,
            kind: map_probe(kind),
        },
        Source::ProbeCompleted { sequence, kind } => RuntimeEventKind::ProbeCompleted {
            sequence,
            kind: map_probe(kind),
        },
        Source::Failure { page, error } => RuntimeEventKind::DisplayFailure {
            error: map_error(error),
            page,
        },
        Source::Completion(value) => RuntimeEventKind::Completion(RuntimeCompletion {
            sequence: value.sequence,
            status: match value.status {
                xteink_x4_display::events::CompletionStatus::Ok => RuntimeCompletionStatus::Ok,
                xteink_x4_display::events::CompletionStatus::Error => {
                    RuntimeCompletionStatus::Error
                }
            },
            page: value.page,
            error: value.error.map(map_error),
        }),
    };
    RuntimeEvent {
        timestamp_ms: event.timestamp_ms,
        kind,
    }
}

pub const fn map_turn(turn: xteink_x4_display::events::PageTurn) -> PageTurn {
    match turn {
        xteink_x4_display::events::PageTurn::Previous => PageTurn::Previous,
        xteink_x4_display::events::PageTurn::Next => PageTurn::Next,
        xteink_x4_display::events::PageTurn::First => PageTurn::First,
        xteink_x4_display::events::PageTurn::Last => PageTurn::Last,
    }
}

pub const fn to_display_turn(turn: PageTurn) -> xteink_x4_display::events::PageTurn {
    match turn {
        PageTurn::Previous => xteink_x4_display::events::PageTurn::Previous,
        PageTurn::Next => xteink_x4_display::events::PageTurn::Next,
        PageTurn::First => xteink_x4_display::events::PageTurn::First,
        PageTurn::Last => xteink_x4_display::events::PageTurn::Last,
    }
}

fn map_phase(phase: xteink_x4_display::events::DisplayPhase) -> RefreshPhase {
    match phase {
        xteink_x4_display::events::DisplayPhase::BwReady => RefreshPhase::BwReady,
        xteink_x4_display::events::DisplayPhase::BwRefreshing => RefreshPhase::BwRefreshing,
        xteink_x4_display::events::DisplayPhase::GrayDelay => RefreshPhase::GrayDelay,
        xteink_x4_display::events::DisplayPhase::GrayRefreshing => RefreshPhase::GrayRefreshing,
        xteink_x4_display::events::DisplayPhase::BaseSync => RefreshPhase::BaseSync,
        xteink_x4_display::events::DisplayPhase::Recovering => RefreshPhase::Recovering,
        xteink_x4_display::events::DisplayPhase::Fault => RefreshPhase::Fault,
    }
}

fn map_probe(kind: xteink_x4_display::probes::ProbeKind) -> DisplayProbeKind {
    match kind {
        xteink_x4_display::probes::ProbeKind::FullRefreshCurrent => {
            DisplayProbeKind::FullRefreshCurrent
        }
        xteink_x4_display::probes::ProbeKind::ClearWhite => DisplayProbeKind::ClearWhite,
        xteink_x4_display::probes::ProbeKind::WindowCorners => DisplayProbeKind::WindowCorners,
    }
}

fn map_error(error: xteink_x4_display::DisplayError) -> FirmwareError {
    match error {
        xteink_x4_display::DisplayError::Source => FirmwareError::Storage,
        xteink_x4_display::DisplayError::Controller => FirmwareError::Spi,
        _ => FirmwareError::InvalidParameter,
    }
}
