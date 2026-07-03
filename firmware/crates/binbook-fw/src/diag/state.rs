use binbook_diagnostic_protocol::{
    FrameHeader, PanelModeCode, RefreshModeCode, Status, StatusPayload,
};

use crate::{
    diag::command::DisplayProbeKind,
    diag_log::DiagLog,
    input::{Button, PageTurn},
};

use super::resolve_log_get;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticSnapshot {
    pub current_page: u32,
    pub page_count: u32,
    pub panel_mode: PanelModeCode,
    pub refresh_mode: RefreshModeCode,
    pub dropped_log_count: u32,
    pub protocol_error_count: u32,
    pub last_error: i32,
}

impl DiagnosticSnapshot {
    pub fn status_payload(&self) -> StatusPayload {
        StatusPayload {
            current_page: self.current_page,
            page_count: self.page_count,
            panel_mode: self.panel_mode,
            dropped_log_count: self.dropped_log_count,
            protocol_error_count: self.protocol_error_count,
            last_error: self.last_error,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingAction {
    KeyPress { button: Button },
    RenderTurn { turn: PageTurn },
    RenderPage { target_page: u32 },
    SetRefreshMode { mode: RefreshModeCode },
    DisplayProbe(DisplayProbeKind),
    CrashGet,
    CrashClear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingCommand {
    pub header: FrameHeader,
    pub action: PendingAction,
}

pub struct DiagnosticPendingQueue<const N: usize> {
    items: [Option<PendingCommand>; N],
    head: usize,
    len: usize,
}

impl<const N: usize> DiagnosticPendingQueue<N> {
    pub const fn new() -> Self {
        Self {
            items: [None; N],
            head: 0,
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn front(&self) -> Option<PendingCommand> {
        if self.len == 0 {
            None
        } else {
            self.items[self.head]
        }
    }

    pub fn try_push(&mut self, pending: PendingCommand) -> Result<(), PendingCommand> {
        if self.len == N {
            return Err(pending);
        }

        let index = (self.head + self.len) % N;
        self.items[index] = Some(pending);
        self.len += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Option<PendingCommand> {
        if self.len == 0 {
            return None;
        }

        let pending = self.items[self.head];
        self.items[self.head] = None;
        self.head = (self.head + 1) % N;
        self.len -= 1;
        pending
    }
}

impl<const N: usize> Default for DiagnosticPendingQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct DiagnosticLoopState<const PENDING: usize, const LOG: usize> {
    snapshot: DiagnosticSnapshot,
    pending: DiagnosticPendingQueue<PENDING>,
    log: DiagLog<LOG>,
}

impl<const PENDING: usize, const LOG: usize> DiagnosticLoopState<PENDING, LOG> {
    pub fn new(snapshot: DiagnosticSnapshot, log: DiagLog<LOG>) -> Self {
        Self {
            snapshot,
            pending: DiagnosticPendingQueue::new(),
            log,
        }
    }

    pub fn enqueue_pending(&mut self, pending: PendingCommand) -> Result<(), PendingCommand> {
        self.pending.try_push(pending)
    }

    pub fn enqueue_pending_with_status(&mut self, pending: PendingCommand) -> Status {
        match self.pending.try_push(pending) {
            Ok(()) => Status::Ok,
            Err(_) => Status::Error,
        }
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn status_payload(&self) -> StatusPayload {
        self.snapshot.status_payload()
    }

    pub fn snapshot(&self) -> DiagnosticSnapshot {
        self.snapshot
    }

    pub fn update_snapshot(&mut self, snapshot: DiagnosticSnapshot) {
        self.snapshot = snapshot;
    }

    pub fn log_mut(&mut self) -> &mut DiagLog<LOG> {
        &mut self.log
    }

    pub fn resolve_log_get(&self, cursor: u32, max_bytes: u16, resp_buf: &mut [u8]) -> usize {
        resolve_log_get(&self.log, cursor, max_bytes, resp_buf)
    }

    pub fn complete_pending(&mut self) -> Option<PendingCommand> {
        self.pending.pop()
    }
}
