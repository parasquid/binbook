use binbook_diagnostic_protocol::{
    Status, TransferDirection, TransferTraceSnapshot, TransferTraceState,
    TRANSFER_TRACE_FLAG_LIBRARY_PENDING, TRANSFER_TRACE_FLAG_SATURATED,
};

use crate::transfer_metrics::TransferMetrics;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferTraceStore {
    active: Option<TransferMetrics>,
    terminal: Option<TransferTraceSnapshot>,
    next_id: u32,
}

impl Default for TransferTraceStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TransferTraceStore {
    pub const fn new() -> Self {
        Self::with_next_id(1)
    }

    pub const fn with_next_id(next_id: u32) -> Self {
        Self {
            active: None,
            terminal: None,
            next_id: if next_id == 0 { 1 } else { next_id },
        }
    }

    pub fn start(
        &mut self,
        direction: TransferDirection,
        selected_window: u8,
        chunk_size: u16,
        protocol_errors: u32,
        now_us: u64,
    ) -> Result<u32, Status> {
        if self.active.is_some() {
            return Err(Status::BadRequest);
        }
        let trace_id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.terminal = None;
        self.active = Some(TransferMetrics::start(
            trace_id,
            direction,
            selected_window,
            chunk_size,
            protocol_errors,
            now_us,
        ));
        Ok(trace_id)
    }

    pub fn metrics_mut(&mut self) -> Option<&mut TransferMetrics> {
        self.active.as_mut()
    }

    pub fn get(&mut self, trace_id: u32, now_us: u64) -> Result<TransferTraceSnapshot, Status> {
        if let Some(active) = self.active.as_mut() {
            let snapshot = active.snapshot(now_us);
            return if snapshot.trace_id == trace_id {
                Ok(snapshot)
            } else {
                Err(Status::NotFound)
            };
        }
        self.terminal
            .filter(|snapshot| snapshot.trace_id == trace_id)
            .ok_or(Status::NotFound)
    }

    pub fn finish(
        &mut self,
        state: TransferTraceState,
        protocol_errors: u32,
        now_us: u64,
        library_pending: bool,
    ) -> Result<TransferTraceSnapshot, Status> {
        let mut active = self.active.take().ok_or(Status::BadRequest)?;
        let mut snapshot = active.finish(state, protocol_errors, now_us);
        if library_pending {
            snapshot.flags |= TRANSFER_TRACE_FLAG_LIBRARY_PENDING;
        }
        self.terminal = Some(snapshot);
        Ok(snapshot)
    }

    pub fn clear(&mut self, trace_id: u32) -> Result<(), Status> {
        if let Some(active) = self.active.as_mut() {
            return if active.trace_id() == trace_id {
                Err(Status::BadRequest)
            } else {
                Err(Status::NotFound)
            };
        }
        match self.terminal {
            Some(snapshot) if snapshot.trace_id == trace_id => {
                self.terminal = None;
                Ok(())
            }
            _ => Err(Status::NotFound),
        }
    }

    pub fn complete_library_refresh(&mut self, trace_id: u32, elapsed_us: u32) {
        let Some(snapshot) = self.terminal.as_mut() else {
            return;
        };
        if snapshot.trace_id != trace_id
            || snapshot.flags & TRANSFER_TRACE_FLAG_LIBRARY_PENDING == 0
        {
            return;
        }
        let Some(total) = snapshot.library_refresh_us.checked_add(elapsed_us) else {
            snapshot.library_refresh_us = u32::MAX;
            snapshot.flags |= TRANSFER_TRACE_FLAG_SATURATED;
            snapshot.flags &= !TRANSFER_TRACE_FLAG_LIBRARY_PENDING;
            return;
        };
        snapshot.library_refresh_us = total;
        snapshot.flags &= !TRANSFER_TRACE_FLAG_LIBRARY_PENDING;
    }
}
