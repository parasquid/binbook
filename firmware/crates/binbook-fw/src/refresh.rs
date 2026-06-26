pub const X4_CHUNK_COUNT: u8 = 30;
pub const DEFAULT_FULL_REFRESH_CADENCE: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshDecision {
    FullGrayscale,
    FullBwSeed,
    AdjacentDirtyPartial { changed_chunk_mask: u32 },
    FullScreenDifferential,
    Noop,
}

impl RefreshDecision {
    pub const fn name(self) -> &'static str {
        match self {
            RefreshDecision::FullGrayscale => "FullGrayscale",
            RefreshDecision::FullBwSeed => "FullBwSeed",
            RefreshDecision::AdjacentDirtyPartial { .. } => "AdjacentDirtyPartial",
            RefreshDecision::FullScreenDifferential => "FullScreenDifferential",
            RefreshDecision::Noop => "Noop",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshPolicy {
    FullScreenDifferentialDefault,
    ChunkDirtyDifferentialDefault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshState {
    previous_page: Option<u32>,
    fast_refresh_count: u32,
    full_refresh_cadence: u32,
    bw_differential_ready: bool,
}

impl RefreshState {
    pub const fn new() -> Self {
        Self {
            previous_page: None,
            fast_refresh_count: 0,
            full_refresh_cadence: DEFAULT_FULL_REFRESH_CADENCE,
            bw_differential_ready: false,
        }
    }

    pub fn decide(&self, target_page: u32, transition_mask: Option<u32>) -> RefreshDecision {
        self.decide_with_policy(
            target_page,
            transition_mask,
            RefreshPolicy::FullScreenDifferentialDefault,
        )
    }

    pub fn decide_with_policy(
        &self,
        target_page: u32,
        transition_mask: Option<u32>,
        policy: RefreshPolicy,
    ) -> RefreshDecision {
        let Some(previous_page) = self.previous_page else {
            return RefreshDecision::FullGrayscale;
        };
        if previous_page == target_page {
            return RefreshDecision::Noop;
        }
        if self.fast_refresh_count >= self.full_refresh_cadence {
            return RefreshDecision::FullGrayscale;
        }
        if !self.bw_differential_ready {
            return RefreshDecision::FullBwSeed;
        }
        match policy {
            RefreshPolicy::FullScreenDifferentialDefault => RefreshDecision::FullScreenDifferential,
            RefreshPolicy::ChunkDirtyDifferentialDefault => {
                if let Some(mask) = transition_mask {
                    RefreshDecision::AdjacentDirtyPartial {
                        changed_chunk_mask: mask,
                    }
                } else {
                    RefreshDecision::FullScreenDifferential
                }
            }
        }
    }

    pub fn record_success(&mut self, target_page: u32, decision: RefreshDecision) {
        self.previous_page = Some(target_page);
        match decision {
            RefreshDecision::FullGrayscale => {
                self.fast_refresh_count = 0;
                self.bw_differential_ready = false;
            }
            RefreshDecision::FullBwSeed
            | RefreshDecision::AdjacentDirtyPartial { .. }
            | RefreshDecision::FullScreenDifferential => {
                self.fast_refresh_count = self.fast_refresh_count.saturating_add(1);
                self.bw_differential_ready = true;
            }
            RefreshDecision::Noop => {}
        }
    }

    pub fn previous_page(&self) -> Option<u32> {
        self.previous_page
    }
}
