pub const POWER_HOLD_MS: u64 = 2_000;
pub const POWER_DOUBLE_TAP_MS: u64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    None,
    FullRefresh,
    PrepareSleep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerRequest {
    FullRefresh,
    PrepareSleep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAck {
    Ready,
    Failed,
}

/// Pairs the acknowledgement with the request that is currently awaiting it.
/// Every power request must consume exactly one acknowledgement; only a ready
/// acknowledgement for `PrepareSleep` may authorize CPU deep sleep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerRequestCompletion {
    request: PowerRequest,
    acknowledgement: PowerAck,
}

impl PowerRequestCompletion {
    pub const fn new(request: PowerRequest, acknowledgement: PowerAck) -> Self {
        Self {
            request,
            acknowledgement,
        }
    }

    pub const fn may_enter_cpu_deep_sleep(self) -> bool {
        matches!(
            (self.request, self.acknowledgement),
            (PowerRequest::PrepareSleep, PowerAck::Ready)
        )
    }
}

/// Result of the ordered state-save → panel-sleep preparation sequence.
///
/// CPU deep sleep is permitted only for `Ready`. Keeping this decision in the
/// host-testable firmware core prevents board runtimes from accidentally
/// acknowledging sleep after a failed persistence or panel operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepPreparationOutcome {
    AwaitPanelSleep,
    StateSaveFailed,
    PanelSleepFailed,
    Ready,
}

impl SleepPreparationOutcome {
    pub const fn after_state_save(succeeded: bool) -> Self {
        if succeeded {
            Self::AwaitPanelSleep
        } else {
            Self::StateSaveFailed
        }
    }

    pub const fn after_panel_sleep(succeeded: bool) -> Self {
        if succeeded {
            Self::Ready
        } else {
            Self::PanelSleepFailed
        }
    }

    pub const fn status_code(self) -> u8 {
        match self {
            Self::Ready => 0,
            Self::StateSaveFailed => 1,
            Self::PanelSleepFailed => 2,
            Self::AwaitPanelSleep => u8::MAX,
        }
    }

    pub const fn ack(self) -> PowerAck {
        if matches!(self, Self::Ready) {
            PowerAck::Ready
        } else {
            PowerAck::Failed
        }
    }

    pub const fn render_sleep_cancelled(self) -> bool {
        matches!(self, Self::StateSaveFailed)
    }

    pub const fn should_sleep_panel(self) -> bool {
        matches!(self, Self::AwaitPanelSleep)
    }

    pub const fn may_enter_cpu_deep_sleep(self) -> bool {
        matches!(self, Self::Ready)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerReleaseGate;

impl PowerReleaseGate {
    pub const fn ready(pressed: bool) -> bool {
        !pressed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoldState {
    WaitingForRelease,
    Armed {
        first_tap_ms: Option<u64>,
    },
    Holding {
        since_ms: u64,
        first_tap_ms: Option<u64>,
    },
    Triggered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerHold {
    state: HoldState,
}

impl PowerHold {
    pub const fn new() -> Self {
        Self {
            state: HoldState::WaitingForRelease,
        }
    }

    pub fn update(&mut self, pressed: bool, now_ms: u64) -> PowerAction {
        match (self.state, pressed) {
            (HoldState::WaitingForRelease | HoldState::Triggered, false) => {
                self.state = HoldState::Armed { first_tap_ms: None };
            }
            (HoldState::Armed { first_tap_ms }, true) => {
                let first_tap_ms = first_tap_ms
                    .filter(|first| now_ms.saturating_sub(*first) <= POWER_DOUBLE_TAP_MS);
                self.state = HoldState::Holding {
                    since_ms: now_ms,
                    first_tap_ms,
                };
            }
            (
                HoldState::Holding {
                    first_tap_ms: Some(first_tap_ms),
                    ..
                },
                false,
            ) if now_ms.saturating_sub(first_tap_ms) <= POWER_DOUBLE_TAP_MS => {
                self.state = HoldState::Armed { first_tap_ms: None };
                return PowerAction::FullRefresh;
            }
            (HoldState::Holding { .. }, false) => {
                self.state = HoldState::Armed {
                    first_tap_ms: Some(now_ms),
                };
            }
            (HoldState::Holding { since_ms, .. }, true)
                if now_ms.saturating_sub(since_ms) >= POWER_HOLD_MS =>
            {
                self.state = HoldState::Triggered;
                return PowerAction::PrepareSleep;
            }
            _ => {}
        }
        PowerAction::None
    }
}

impl Default for PowerHold {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_release_then_continuous_two_second_hold() {
        let mut hold = PowerHold::new();
        assert_eq!(hold.update(true, 0), PowerAction::None);
        assert_eq!(hold.update(true, 3_000), PowerAction::None);
        assert_eq!(hold.update(false, 3_020), PowerAction::None);
        assert_eq!(hold.update(true, 4_000), PowerAction::None);
        assert_eq!(hold.update(true, 5_999), PowerAction::None);
        assert_eq!(hold.update(true, 6_000), PowerAction::PrepareSleep);
        assert_eq!(hold.update(true, 8_000), PowerAction::None);
    }

    #[test]
    fn early_release_cancels_hold() {
        let mut hold = PowerHold::new();
        hold.update(false, 0);
        hold.update(true, 10);
        hold.update(false, 1_000);
        hold.update(true, 2_000);
        assert_eq!(hold.update(true, 3_999), PowerAction::None);
        assert_eq!(hold.update(true, 4_000), PowerAction::PrepareSleep);
    }

    #[test]
    fn two_short_taps_request_one_user_full_refresh() {
        let mut hold = PowerHold::new();
        assert_eq!(hold.update(false, 0), PowerAction::None);
        assert_eq!(hold.update(true, 100), PowerAction::None);
        assert_eq!(hold.update(false, 180), PowerAction::None);
        assert_eq!(hold.update(true, 400), PowerAction::None);
        assert_eq!(hold.update(false, 470), PowerAction::FullRefresh);
        assert_eq!(hold.update(false, 900), PowerAction::None);
    }

    #[test]
    fn separated_short_taps_remain_noops() {
        let mut hold = PowerHold::new();
        hold.update(false, 0);
        hold.update(true, 100);
        assert_eq!(hold.update(false, 180), PowerAction::None);
        hold.update(true, 180 + POWER_DOUBLE_TAP_MS + 1);
        assert_eq!(
            hold.update(false, 260 + POWER_DOUBLE_TAP_MS),
            PowerAction::None
        );
    }

    #[test]
    fn holding_the_second_press_still_requests_sleep_not_refresh() {
        let mut hold = PowerHold::new();
        hold.update(false, 0);
        hold.update(true, 100);
        hold.update(false, 180);
        hold.update(true, 300);
        assert_eq!(hold.update(true, 2_299), PowerAction::None);
        assert_eq!(hold.update(true, 2_300), PowerAction::PrepareSleep);
        assert_eq!(hold.update(false, 2_400), PowerAction::None);
    }

    #[test]
    fn deep_sleep_entry_is_blocked_until_the_initiating_hold_is_released() {
        assert!(!PowerReleaseGate::ready(true));
        assert!(PowerReleaseGate::ready(false));
    }
}
