pub const BULK_IDLE_TIMEOUT_MS: u64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BulkIdleTimeout {
    deadline_ms: Option<u64>,
}

impl BulkIdleTimeout {
    pub const fn new() -> Self {
        Self { deadline_ms: None }
    }

    pub fn arm(&mut self, now_ms: u64) {
        self.deadline_ms = Some(now_ms.saturating_add(BULK_IDLE_TIMEOUT_MS));
    }

    pub fn touch(&mut self, now_ms: u64) {
        if self.deadline_ms.is_some() {
            self.arm(now_ms);
        }
    }

    pub fn disarm(&mut self) {
        self.deadline_ms = None;
    }

    pub const fn is_armed(&self) -> bool {
        self.deadline_ms.is_some()
    }

    pub fn remaining_ms(&self, now_ms: u64) -> Option<u64> {
        self.deadline_ms
            .map(|deadline| deadline.saturating_sub(now_ms))
    }

    pub fn take_expired(&mut self, now_ms: u64) -> bool {
        if self.deadline_ms.is_none_or(|deadline| now_ms < deadline) {
            return false;
        }
        self.disarm();
        true
    }
}
