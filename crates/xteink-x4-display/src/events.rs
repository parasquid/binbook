use crate::{engine::ControllerRamState, probes::ProbeKind, DisplayError};

pub const GRAY_SETTLE_DELAY_MS: u64 = 350;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageTurn {
    Previous,
    Next,
    First,
    Last,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayRequest {
    Turn {
        turn: PageTurn,
        sequence: Option<u16>,
    },
    Goto {
        page: u32,
        sequence: u16,
    },
    Probe {
        kind: ProbeKind,
        sequence: u16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayPhase {
    BwReady,
    BwRefreshing,
    GrayDelay,
    GrayRefreshing,
    BaseSync,
    Recovering,
    Fault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelMode {
    Unknown,
    Grayscale,
    Bw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationOutcome {
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionStatus {
    Ok,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayCompletion {
    pub sequence: Option<u16>,
    pub status: CompletionStatus,
    pub page: u32,
    pub error: Option<DisplayError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayEventKind {
    PhaseChanged(DisplayPhase),
    PanelModeChanged(PanelMode),
    ControllerStateChanged(ControllerRamState),
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
    GrayDelayCancelled {
        page: u32,
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
        kind: ProbeKind,
    },
    ProbeCompleted {
        sequence: u16,
        kind: ProbeKind,
    },
    Failure {
        page: u32,
        error: DisplayError,
    },
    Completion(DisplayCompletion),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayEvent {
    pub timestamp_ms: u64,
    pub kind: DisplayEventKind,
}

pub trait EventSink {
    fn emit(&mut self, event: DisplayEvent);
}
