use crate::async_refresh::{DisplayProbeKind, DisplayRequest};
use crate::xteink_x4_display::events::PageRefreshMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountedDiagnosticAction {
    SetRefreshMode(PageRefreshMode),
    Probe(DisplayProbeKind),
}

#[must_use]
pub const fn mounted_diagnostic_action(
    request: &DisplayRequest,
) -> Option<MountedDiagnosticAction> {
    match *request {
        DisplayRequest::SetRefreshMode { mode, .. } => {
            Some(MountedDiagnosticAction::SetRefreshMode(mode))
        }
        DisplayRequest::Probe { kind, .. } => Some(MountedDiagnosticAction::Probe(kind)),
        _ => None,
    }
}

#[must_use]
pub const fn mounted_request_needs_gray_refinement(request: &DisplayRequest) -> bool {
    matches!(
        *request,
        DisplayRequest::MenuSelect { .. }
            | DisplayRequest::Turn { .. }
            | DisplayRequest::Goto { .. }
            | DisplayRequest::Probe {
                kind: DisplayProbeKind::FullRefreshCurrent,
                ..
            }
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerReaderPhase {
    Menu,
    Opening,
    RenderingOpen {
        target: u32,
    },
    Reading {
        page: u32,
        page_count: u32,
    },
    RenderingTurn {
        from: u32,
        target: u32,
        page_count: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerReaderError {
    InvalidPhase,
    EmptyBook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerReaderLifecycle {
    phase: OwnerReaderPhase,
}

impl OwnerReaderLifecycle {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            phase: OwnerReaderPhase::Menu,
        }
    }

    #[must_use]
    pub const fn phase(self) -> OwnerReaderPhase {
        self.phase
    }

    pub fn begin_open(&mut self) -> Result<(), OwnerReaderError> {
        if self.phase != OwnerReaderPhase::Menu {
            return Err(OwnerReaderError::InvalidPhase);
        }
        self.phase = OwnerReaderPhase::Opening;
        Ok(())
    }

    pub fn metadata_opened(
        &mut self,
        page_count: u32,
        saved_page: u32,
    ) -> Result<u32, OwnerReaderError> {
        if self.phase != OwnerReaderPhase::Opening {
            return Err(OwnerReaderError::InvalidPhase);
        }
        if page_count == 0 {
            self.phase = OwnerReaderPhase::Menu;
            return Err(OwnerReaderError::EmptyBook);
        }
        let target = saved_page.min(page_count - 1);
        self.phase = OwnerReaderPhase::RenderingOpen { target };
        Ok(target)
    }

    pub fn commit_open(&mut self, page_count: u32) -> Result<u32, OwnerReaderError> {
        let OwnerReaderPhase::RenderingOpen { target } = self.phase else {
            return Err(OwnerReaderError::InvalidPhase);
        };
        if page_count == 0 || target >= page_count {
            return Err(OwnerReaderError::EmptyBook);
        }
        self.phase = OwnerReaderPhase::Reading {
            page: target,
            page_count,
        };
        Ok(target)
    }

    pub fn fail_open(&mut self) -> Result<(), OwnerReaderError> {
        if !matches!(
            self.phase,
            OwnerReaderPhase::Opening | OwnerReaderPhase::RenderingOpen { .. }
        ) {
            return Err(OwnerReaderError::InvalidPhase);
        }
        self.phase = OwnerReaderPhase::Menu;
        Ok(())
    }

    pub fn begin_turn(&mut self, delta: i8) -> Result<(u32, u32), OwnerReaderError> {
        let OwnerReaderPhase::Reading { page, page_count } = self.phase else {
            return Err(OwnerReaderError::InvalidPhase);
        };
        let target = if delta < 0 {
            page.saturating_sub(1)
        } else if delta > 0 {
            page.saturating_add(1).min(page_count - 1)
        } else {
            page
        };
        self.phase = OwnerReaderPhase::RenderingTurn {
            from: page,
            target,
            page_count,
        };
        Ok((page, target))
    }

    pub fn begin_goto(&mut self, requested: u32) -> Result<(u32, u32), OwnerReaderError> {
        let OwnerReaderPhase::Reading { page, page_count } = self.phase else {
            return Err(OwnerReaderError::InvalidPhase);
        };
        let target = requested.min(page_count - 1);
        self.phase = OwnerReaderPhase::RenderingTurn {
            from: page,
            target,
            page_count,
        };
        Ok((page, target))
    }

    pub fn finish_turn(&mut self, success: bool) -> Result<u32, OwnerReaderError> {
        let OwnerReaderPhase::RenderingTurn {
            from,
            target,
            page_count,
        } = self.phase
        else {
            return Err(OwnerReaderError::InvalidPhase);
        };
        let page = if success { target } else { from };
        self.phase = OwnerReaderPhase::Reading { page, page_count };
        Ok(page)
    }

    pub fn close(&mut self) -> Result<u32, OwnerReaderError> {
        let page = match self.phase {
            OwnerReaderPhase::Reading { page, .. } => page,
            _ => return Err(OwnerReaderError::InvalidPhase),
        };
        self.phase = OwnerReaderPhase::Menu;
        Ok(page)
    }
}

impl Default for OwnerReaderLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerStateFlush {
    dirty: bool,
    deadline_ms: u64,
}

impl OwnerStateFlush {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            dirty: false,
            deadline_ms: 0,
        }
    }

    pub fn mark_dirty(&mut self, now_ms: u64, delay_ms: u64) {
        self.dirty = true;
        self.deadline_ms = now_ms.saturating_add(delay_ms);
    }

    pub fn postpone(&mut self, now_ms: u64, delay_ms: u64) {
        if self.dirty {
            self.deadline_ms = now_ms.saturating_add(delay_ms);
        }
    }

    #[must_use]
    pub const fn is_dirty(self) -> bool {
        self.dirty
    }

    #[must_use]
    pub const fn wait_ms(self, now_ms: u64, idle_wait_ms: u64) -> u64 {
        if self.dirty {
            self.deadline_ms.saturating_sub(now_ms)
        } else {
            idle_wait_ms
        }
    }

    #[must_use]
    pub const fn due(self, now_ms: u64) -> bool {
        self.dirty && now_ms >= self.deadline_ms
    }

    pub fn record_success(&mut self) {
        self.dirty = false;
    }

    pub fn record_failure(&mut self, now_ms: u64, retry_ms: u64) {
        self.dirty = true;
        self.deadline_ms = now_ms.saturating_add(retry_ms);
    }
}

impl Default for OwnerStateFlush {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerGrayRefinement {
    pending: Option<PendingGray>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingGray {
    page: u32,
    epoch: u32,
    deadline_ms: u64,
}

impl OwnerGrayRefinement {
    #[must_use]
    pub const fn new() -> Self {
        Self { pending: None }
    }

    pub fn schedule(&mut self, page: u32, epoch: u32, now_ms: u64, delay_ms: u64) {
        self.pending = Some(PendingGray {
            page,
            epoch,
            deadline_ms: now_ms.saturating_add(delay_ms),
        });
    }

    pub fn cancel(&mut self) {
        self.pending = None;
    }

    #[must_use]
    pub const fn wait_ms(self, now_ms: u64, idle_wait_ms: u64) -> u64 {
        match self.pending {
            Some(pending) => pending.deadline_ms.saturating_sub(now_ms),
            None => idle_wait_ms,
        }
    }

    pub fn take_due(&mut self, now_ms: u64, current_epoch: u32) -> Option<(u32, u32)> {
        let pending = self.pending?;
        if pending.epoch != current_epoch {
            self.pending = None;
            return None;
        }
        if now_ms < pending.deadline_ms {
            return None;
        }
        self.pending = None;
        Some((pending.page, pending.epoch))
    }
}

impl Default for OwnerGrayRefinement {
    fn default() -> Self {
        Self::new()
    }
}
