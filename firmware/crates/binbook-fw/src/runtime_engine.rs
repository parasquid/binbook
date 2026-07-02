use crate::error::FirmwareError;
use crate::{
    async_refresh::{DisplayProbeKind, DisplayRequest, RefreshPhase},
    input::{Button, InputDecision, PageTurn},
    menu::Mode,
};

pub use xteink_x4_display::engine::ControllerRamState;
pub type RuntimePanelMode = xteink_x4_display::events::PanelMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCompletionStatus {
    Ok,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeRequestKind {
    Turn,
    Goto,
    Probe,
    MenuNext,
    MenuPrev,
    MenuSelect,
    MenuBack,
}

impl RuntimeRequestKind {
    pub const fn from_request(request: &DisplayRequest) -> Self {
        match request {
            DisplayRequest::Turn { .. } => Self::Turn,
            DisplayRequest::Goto { .. } => Self::Goto,
            DisplayRequest::Probe { .. } => Self::Probe,
            DisplayRequest::MenuNext => Self::MenuNext,
            DisplayRequest::MenuPrev => Self::MenuPrev,
            DisplayRequest::MenuSelect => Self::MenuSelect,
            DisplayRequest::MenuBack => Self::MenuBack,
        }
    }

    pub const fn log_code(self) -> i32 {
        match self {
            Self::Turn => 0,
            Self::Goto => 1,
            Self::Probe => 2,
            Self::MenuNext => 3,
            Self::MenuPrev => 4,
            Self::MenuSelect => 5,
            Self::MenuBack => 6,
        }
    }
}

#[must_use]
pub const fn request_sequence(request: &DisplayRequest) -> Option<u16> {
    match request {
        DisplayRequest::Turn {
            completion_sequence,
            ..
        } => *completion_sequence,
        DisplayRequest::Goto {
            completion_sequence,
            ..
        }
        | DisplayRequest::Probe {
            completion_sequence,
            ..
        } => Some(*completion_sequence),
        DisplayRequest::MenuNext
        | DisplayRequest::MenuPrev
        | DisplayRequest::MenuSelect
        | DisplayRequest::MenuBack => None,
    }
}

#[must_use]
pub const fn queue_age_ms(received_ms: u64, enqueued_ms: Option<u64>) -> Option<u32> {
    match enqueued_ms {
        Some(sent_ms) => {
            let elapsed = received_ms.saturating_sub(sent_ms);
            if elapsed > u32::MAX as u64 {
                Some(u32::MAX)
            } else {
                Some(elapsed as u32)
            }
        }
        None => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestEnqueueStatus {
    Ok,
    Full,
    Unmapped,
}

impl RequestEnqueueStatus {
    pub const fn log_code(self) -> i32 {
        match self {
            Self::Ok => 0,
            Self::Full => 1,
            Self::Unmapped => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusyWaitSite {
    GenericWaitReady,
    PanelInit,
    BwRefresh,
    GrayRefresh,
    Probe,
}

impl BusyWaitSite {
    pub const fn log_code(self) -> i32 {
        match self {
            Self::GenericWaitReady => 0,
            Self::PanelInit => 1,
            Self::BwRefresh => 2,
            Self::GrayRefresh => 3,
            Self::Probe => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusyWaitStatus {
    Ready,
    Timeout,
    PinError,
}

impl BusyWaitStatus {
    pub const fn log_code(self) -> i32 {
        match self {
            Self::Ready => 0,
            Self::Timeout => 1,
            Self::PinError => 2,
        }
    }
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
    RequestEnqueue {
        kind: RuntimeRequestKind,
        sequence: Option<u16>,
        status: RequestEnqueueStatus,
    },
    RequestReceive {
        kind: RuntimeRequestKind,
        sequence: Option<u16>,
        queue_age_ms: Option<u32>,
    },
    DisplayRequestStart {
        kind: RuntimeRequestKind,
        current_page: u32,
        target_page: Option<u32>,
    },
    DisplayRequestEnd {
        kind: RuntimeRequestKind,
        duration_ms: u32,
        status: RuntimeCompletionStatus,
    },
    BusyWaitStart {
        site: BusyWaitSite,
        timeout_ms: u32,
        busy_state: Option<bool>,
    },
    BusyWaitEnd {
        site: BusyWaitSite,
        elapsed_ms: u32,
        status: BusyWaitStatus,
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
    MenuTransition,
    ModeChange {
        mode: Mode,
    },
    BookOpen {
        name_index: usize,
    },
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
