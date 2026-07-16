use binbook_diagnostic_protocol::{TransferDirection, TransferTraceState};

use crate::{transfer_metrics::TransferPhase, transfer_trace_command::TransferTraceCommand};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LegacyTraceLifecycle {
    upload_data_active: bool,
    readback_data_active: bool,
}

impl LegacyTraceLifecycle {
    pub const fn new() -> Self {
        Self {
            upload_data_active: false,
            readback_data_active: false,
        }
    }

    pub fn start_data(
        &mut self,
        direction: TransferDirection,
        trace: &mut TransferTraceCommand,
        now_us: u64,
    ) {
        let Some(active) = self.active_mut(direction) else {
            return;
        };
        if !*active && trace.active_direction() == Some(direction) {
            trace.enter_phase(TransferPhase::DataPhase, now_us);
            *active = true;
        }
    }

    pub fn finish(
        &mut self,
        direction: TransferDirection,
        state: TransferTraceState,
        protocol_errors: u32,
        now_us: u64,
        library_pending: bool,
        trace: &mut TransferTraceCommand,
    ) {
        if self.end_data(direction, trace, now_us) || trace.active_direction() == Some(direction) {
            let _ = trace.finish(state, protocol_errors, now_us, library_pending);
        }
    }

    pub fn end_data(
        &mut self,
        direction: TransferDirection,
        trace: &mut TransferTraceCommand,
        now_us: u64,
    ) -> bool {
        let Some(active) = self.active_mut(direction) else {
            return false;
        };
        if core::mem::take(active) {
            trace.exit_phase(TransferPhase::DataPhase, now_us);
            true
        } else {
            false
        }
    }

    fn active_mut(&mut self, direction: TransferDirection) -> Option<&mut bool> {
        match direction {
            TransferDirection::Upload => Some(&mut self.upload_data_active),
            TransferDirection::Readback => Some(&mut self.readback_data_active),
            TransferDirection::Unspecified => None,
        }
    }
}

pub fn trace_phase(
    trace: &mut TransferTraceCommand,
    direction: TransferDirection,
    phase: TransferPhase,
    enter: bool,
    now_us: u64,
) {
    if trace.active_direction() != Some(direction) {
        return;
    }
    if enter {
        trace.enter_phase(phase, now_us);
    } else {
        trace.exit_phase(phase, now_us);
    }
}
