use portable_atomic::{AtomicBool, Ordering};

pub struct LibraryRefreshBarrier {
    pending: AtomicBool,
}

impl LibraryRefreshBarrier {
    pub const fn new_pending() -> Self {
        Self {
            pending: AtomicBool::new(true),
        }
    }

    pub fn mark_pending(&self) {
        self.pending.store(true, Ordering::Release);
    }

    pub fn mark_settled(&self) {
        self.pending.store(false, Ordering::Release);
    }

    pub fn is_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }
}

impl Default for LibraryRefreshBarrier {
    fn default() -> Self {
        Self::new_pending()
    }
}
