//! Host-testable refresh coordinator for deferred grayscale refresh.

use crate::input::{Button, PageTurn};

/// Mode constants for `button_to_request`.
pub const MODE_MENU: u8 = 0;
pub const MODE_READING: u8 = 1;
#[cfg(feature = "sd-storage")]
pub const STARTUP_DISPLAY_MODE: u8 = MODE_MENU;
#[cfg(not(feature = "sd-storage"))]
pub const STARTUP_DISPLAY_MODE: u8 = MODE_READING;

/// Map a [`Button`] press to a [`DisplayRequest`] based on the current display
/// mode. Returns `None` when the button has no action in the given mode.
#[must_use]
pub fn button_to_request(button: Button, mode: u8) -> Option<DisplayRequest> {
    match mode {
        MODE_MENU => match button {
            Button::Up | Button::Left => Some(DisplayRequest::MenuPrev),
            Button::Down | Button::Right => Some(DisplayRequest::MenuNext),
            Button::Select => Some(DisplayRequest::MenuSelect),
            Button::Back | Button::Power => None,
        },
        MODE_READING => match button {
            Button::Up | Button::Left => Some(DisplayRequest::Turn {
                turn: PageTurn::Previous,
                completion_sequence: None,
            }),
            Button::Down | Button::Right => Some(DisplayRequest::Turn {
                turn: PageTurn::Next,
                completion_sequence: None,
            }),
            #[cfg(feature = "sd-storage")]
            Button::Select | Button::Back => Some(DisplayRequest::MenuBack),
            #[cfg(not(feature = "sd-storage"))]
            Button::Select | Button::Back => None,
            Button::Power => None,
        },
        _ => None,
    }
}

pub const PAGE_TURN_QUEUE_CAPACITY: usize = 16;
pub const DISPLAY_COMPLETION_CAPACITY: usize = 16;
pub const INPUT_POLL_INTERVAL_MS: u64 = 50;
pub const GRAY_SETTLE_DELAY_MS: u64 = 350;
pub const DISPLAY_BUSY_TIMEOUT_MS: u64 = 60_000;
pub const DISPLAY_STREAM_STRIP_ROWS: u16 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayProbeKind {
    FullRefreshCurrent,
    ClearWhite,
    WindowCorners,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayCompletionStatus {
    Ok,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayRequest {
    Turn {
        turn: PageTurn,
        completion_sequence: Option<u16>,
    },
    Goto {
        page: u32,
        completion_sequence: u16,
    },
    Probe {
        kind: DisplayProbeKind,
        completion_sequence: u16,
    },
    MenuNext,
    MenuPrev,
    MenuSelect,
    MenuBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayCompletion {
    pub sequence: u16,
    pub status: DisplayCompletionStatus,
    pub page: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshPhase {
    BwReady,
    BwRefreshing,
    GrayDelay,
    GrayRefreshing,
    BaseSync,
    Recovering,
    Fault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshAction {
    RenderBw { from: u32, target: u32 },
    RenderGray { page: u32 },
    SyncBwBase { page: u32 },
    RecoverBw { page: u32 },
    WaitForRequest,
    WaitUntil { deadline_ms: u64 },
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshCoordinator {
    page_count: u32,
    displayed_page: u32,
    active_target: Option<u32>,
    gray_deadline_ms: Option<u64>,
    phase: RefreshPhase,
    next_action: RefreshAction,
}

impl RefreshCoordinator {
    pub fn new(page_count: u32) -> Self {
        Self {
            page_count,
            displayed_page: 0,
            active_target: None,
            gray_deadline_ms: None,
            phase: RefreshPhase::Recovering,
            next_action: RefreshAction::RecoverBw { page: 0 },
        }
    }

    pub fn phase(&self) -> RefreshPhase {
        self.phase
    }

    pub fn next_action(&self) -> RefreshAction {
        self.next_action
    }

    pub fn displayed_page(&self) -> u32 {
        self.displayed_page
    }

    pub fn start_bw(&mut self, target: u32) -> RefreshAction {
        if target >= self.page_count {
            self.next_action = RefreshAction::None;
            return self.next_action;
        }

        self.active_target = Some(target);
        self.phase = RefreshPhase::BwRefreshing;
        self.next_action = RefreshAction::RenderBw {
            from: self.displayed_page,
            target,
        };
        self.next_action
    }

    pub fn record_bw_complete(&mut self, target: u32, now_ms: u64) -> RefreshAction {
        if self.phase != RefreshPhase::BwRefreshing || self.active_target != Some(target) {
            self.next_action = RefreshAction::None;
            return self.next_action;
        }

        self.displayed_page = target;
        self.active_target = None;
        self.gray_deadline_ms = Some(now_ms + GRAY_SETTLE_DELAY_MS);
        self.phase = RefreshPhase::GrayDelay;
        self.next_action = RefreshAction::WaitUntil {
            deadline_ms: now_ms + GRAY_SETTLE_DELAY_MS,
        };
        self.next_action
    }

    pub fn gray_deadline_elapsed(&mut self, now_ms: u64) -> RefreshAction {
        if self.phase != RefreshPhase::GrayDelay {
            self.next_action = RefreshAction::None;
            return self.next_action;
        }

        let Some(deadline_ms) = self.gray_deadline_ms else {
            self.next_action = RefreshAction::None;
            return self.next_action;
        };

        if now_ms < deadline_ms {
            self.next_action = RefreshAction::WaitUntil { deadline_ms };
            return self.next_action;
        }

        self.phase = RefreshPhase::GrayRefreshing;
        self.next_action = RefreshAction::RenderGray {
            page: self.displayed_page,
        };
        self.next_action
    }

    pub fn record_gray_complete(&mut self) -> RefreshAction {
        if self.phase != RefreshPhase::GrayRefreshing {
            self.next_action = RefreshAction::None;
            return self.next_action;
        }

        self.phase = RefreshPhase::BaseSync;
        self.next_action = RefreshAction::SyncBwBase {
            page: self.displayed_page,
        };
        self.next_action
    }

    pub fn record_gray_cancelled(&mut self) -> RefreshAction {
        if self.phase != RefreshPhase::GrayRefreshing {
            self.next_action = RefreshAction::None;
            return self.next_action;
        }
        self.phase = RefreshPhase::BwReady;
        self.next_action = RefreshAction::WaitForRequest;
        self.next_action
    }

    pub fn record_base_sync_complete(&mut self) -> RefreshAction {
        if self.phase != RefreshPhase::BaseSync {
            self.next_action = RefreshAction::None;
            return self.next_action;
        }
        self.phase = RefreshPhase::BwReady;
        self.next_action = RefreshAction::WaitForRequest;
        self.next_action
    }

    pub fn request_arrived(&mut self) -> RefreshAction {
        if self.phase == RefreshPhase::GrayDelay {
            self.gray_deadline_ms = None;
            self.phase = RefreshPhase::BwReady;
        }
        self.next_action = RefreshAction::None;
        self.next_action
    }

    pub fn record_seed_complete(&mut self, page: u32, now_ms: u64) -> RefreshAction {
        self.displayed_page = page;
        self.active_target = None;
        self.gray_deadline_ms = Some(now_ms + GRAY_SETTLE_DELAY_MS);
        self.phase = RefreshPhase::GrayDelay;
        self.next_action = RefreshAction::WaitUntil {
            deadline_ms: now_ms + GRAY_SETTLE_DELAY_MS,
        };
        self.next_action
    }

    pub fn record_failure(&mut self) -> RefreshAction {
        match self.phase {
            RefreshPhase::Recovering => {
                self.phase = RefreshPhase::Fault;
                self.next_action = RefreshAction::None;
                self.next_action
            }
            RefreshPhase::Fault => {
                self.next_action = RefreshAction::None;
                self.next_action
            }
            _ => {
                let page = self.active_target.unwrap_or(self.displayed_page);
                self.active_target = Some(page);
                self.phase = RefreshPhase::Recovering;
                self.next_action = RefreshAction::RecoverBw { page };
                self.next_action
            }
        }
    }

    pub fn begin_recovery(&mut self, page: u32) -> RefreshAction {
        if page >= self.page_count || self.phase == RefreshPhase::Fault {
            self.next_action = RefreshAction::None;
            return self.next_action;
        }
        self.active_target = Some(page);
        self.phase = RefreshPhase::Recovering;
        self.next_action = RefreshAction::RecoverBw { page };
        self.next_action
    }

    pub fn record_recovery_complete(&mut self, page: u32, _now_ms: u64) -> RefreshAction {
        if self.phase != RefreshPhase::Recovering || self.active_target != Some(page) {
            self.next_action = RefreshAction::None;
            return self.next_action;
        }

        self.displayed_page = page;
        self.active_target = None;
        self.phase = RefreshPhase::BwReady;
        self.next_action = RefreshAction::WaitForRequest;
        self.next_action
    }
}

#[cfg(test)]
#[path = "async_refresh_tests.rs"]
mod tests;
