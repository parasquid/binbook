use heapless::Vec;

use crate::storage_owner::StorageClient;

pub const OWNER_TIMING_CAPACITY: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerTimingPhase {
    Queued,
    Executing,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerTimingEvent {
    pub client: StorageClient,
    pub sequence: u32,
    pub phase: OwnerTimingPhase,
    pub timestamp_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerTimingTrackerError {
    Duplicate,
    Correlation,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingTiming {
    sequence: u32,
    queued_us: u64,
    executing_us: Option<u64>,
}

pub struct OwnerTimingTracker {
    pending: Vec<PendingTiming, OWNER_TIMING_CAPACITY>,
}

impl OwnerTimingTracker {
    pub const fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    pub fn observe(
        &mut self,
        event: OwnerTimingEvent,
    ) -> Result<Option<(OwnerTimingPhase, u64)>, OwnerTimingTrackerError> {
        if event.client != StorageClient::Diagnostic {
            return Ok(None);
        }
        match event.phase {
            OwnerTimingPhase::Queued => {
                if self.find(event.sequence).is_some() {
                    return Err(OwnerTimingTrackerError::Duplicate);
                }
                self.pending
                    .push(PendingTiming {
                        sequence: event.sequence,
                        queued_us: event.timestamp_us,
                        executing_us: None,
                    })
                    .map_err(|_| OwnerTimingTrackerError::Full)?;
                Ok(None)
            }
            OwnerTimingPhase::Executing => {
                let index = self
                    .find(event.sequence)
                    .ok_or(OwnerTimingTrackerError::Correlation)?;
                let pending = &mut self.pending[index];
                if pending.executing_us.is_some() {
                    return Err(OwnerTimingTrackerError::Duplicate);
                }
                pending.executing_us = Some(event.timestamp_us);
                Ok(Some((
                    OwnerTimingPhase::Queued,
                    event.timestamp_us.saturating_sub(pending.queued_us),
                )))
            }
            OwnerTimingPhase::Completed => {
                let index = self
                    .find(event.sequence)
                    .ok_or(OwnerTimingTrackerError::Correlation)?;
                let started = self.pending[index]
                    .executing_us
                    .ok_or(OwnerTimingTrackerError::Correlation)?;
                self.pending.remove(index);
                Ok(Some((
                    OwnerTimingPhase::Executing,
                    event.timestamp_us.saturating_sub(started),
                )))
            }
        }
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    fn find(&self, sequence: u32) -> Option<usize> {
        self.pending
            .iter()
            .position(|pending| pending.sequence == sequence)
    }
}

impl Default for OwnerTimingTracker {
    fn default() -> Self {
        Self::new()
    }
}
