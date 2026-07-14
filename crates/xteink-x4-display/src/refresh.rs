pub const X4_CHUNK_COUNT: u8 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshDecision {
    FullGrayscale,
    FullBwSeed,
    AdjacentDirtyPartial { changed_chunk_mask: u32 },
    FullScreenDifferential,
    Noop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshPolicy {
    FullScreenDifferential,
    ChunkDirtyDifferential,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshState {
    previous_page: Option<u32>,
    differential_ready: bool,
}

impl RefreshState {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            previous_page: None,
            differential_ready: false,
        }
    }

    #[must_use]
    pub fn decide(&self, target: u32, mask: Option<u32>) -> RefreshDecision {
        self.decide_with_policy(target, mask, RefreshPolicy::FullScreenDifferential)
    }

    #[must_use]
    pub fn decide_with_policy(
        &self,
        target: u32,
        mask: Option<u32>,
        policy: RefreshPolicy,
    ) -> RefreshDecision {
        let Some(previous) = self.previous_page else {
            return RefreshDecision::FullGrayscale;
        };
        if previous == target {
            return RefreshDecision::Noop;
        }
        if !self.differential_ready {
            return RefreshDecision::FullBwSeed;
        }
        match (policy, mask) {
            (RefreshPolicy::ChunkDirtyDifferential, Some(changed_chunk_mask)) => {
                RefreshDecision::AdjacentDirtyPartial { changed_chunk_mask }
            }
            _ => RefreshDecision::FullScreenDifferential,
        }
    }

    pub fn record_success(&mut self, target: u32, decision: RefreshDecision) {
        self.previous_page = Some(target);
        match decision {
            RefreshDecision::FullGrayscale => {
                self.differential_ready = false;
            }
            RefreshDecision::FullBwSeed
            | RefreshDecision::AdjacentDirtyPartial { .. }
            | RefreshDecision::FullScreenDifferential => {
                self.differential_ready = true;
            }
            RefreshDecision::Noop => {}
        }
    }

    pub fn invalidate(&mut self) {
        *self = Self::new();
    }
}

impl Default for RefreshState {
    fn default() -> Self {
        Self::new()
    }
}
