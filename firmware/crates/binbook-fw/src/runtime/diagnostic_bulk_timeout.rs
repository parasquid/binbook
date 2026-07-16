use binbook_fw::bulk_idle_timeout::{BulkIdleTimeout, BULK_IDLE_TIMEOUT_MS};

use super::{diagnostic_bulk::DiagnosticBulkRuntime, STORAGE_REQUEST_CHANNEL};

impl DiagnosticBulkRuntime {
    pub(super) fn arm_idle_timeout(&mut self, now_ms: u64) {
        self.idle_timeout.arm(now_ms);
    }

    pub(super) fn touch_idle_timeout(&mut self, now_ms: u64) {
        self.idle_timeout.touch(now_ms);
    }

    pub(super) fn disarm_idle_timeout(&mut self) {
        self.idle_timeout.disarm();
    }

    pub(super) fn timeout_wait_ms(&self, now_ms: u64) -> u64 {
        self.idle_timeout
            .remaining_ms(now_ms)
            .unwrap_or(BULK_IDLE_TIMEOUT_MS)
            .max(1)
    }

    pub(super) async fn handle_idle_timeout(&mut self, now_ms: u64) {
        if !self.idle_timeout.take_expired(now_ms) {
            return;
        }
        match self.coordinator.begin_timeout_cleanup() {
            Ok(Some(request)) => STORAGE_REQUEST_CHANNEL.send(request).await,
            Ok(None) => {}
            Err(_) => self.idle_timeout.arm(now_ms),
        }
    }
}

const _: BulkIdleTimeout = BulkIdleTimeout::new();
