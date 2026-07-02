use crate::panel::RefreshMode;

pub trait RenderTimingObserver {
    fn now_ms(&self) -> u64 {
        0
    }

    fn page_metadata_read(&mut self, _from: u32, _target: u32, _duration_ms: u32) {}

    fn plane_write_start(&mut self, _role: PlaneRole, _target: RamTarget, _plane_bytes: u32) {}

    fn plane_row_fill_summary(&mut self, _role: PlaneRole, _duration_ms: u32, _row_count: u32) {}

    fn plane_spi_write_summary(
        &mut self,
        _role: PlaneRole,
        _duration_ms: u32,
        _bytes_written: u32,
    ) {
    }

    fn plane_write_end(&mut self, _role: PlaneRole, _duration_ms: u32, _status: RenderStageStatus) {
    }

    fn refresh_trigger(
        &mut self,
        _mode: RefreshMode,
        _duration_ms: u32,
        _status: RenderStageStatus,
    ) {
    }
}

pub struct NoopRenderTimingObserver;

impl RenderTimingObserver for NoopRenderTimingObserver {}

pub struct RenderObservers<'a, B, T> {
    pub busy: &'a mut B,
    pub timing: &'a mut T,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PlaneRole {
    PreviousFastBase,
    TargetFastBase,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RenderStageStatus {
    Ok,
    Error,
    Cancelled,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RamTarget {
    Black,
    Red,
}

pub(crate) const fn elapsed_u32(start_ms: u64, end_ms: u64) -> u32 {
    let elapsed = end_ms.saturating_sub(start_ms);
    if elapsed > u32::MAX as u64 {
        u32::MAX
    } else {
        elapsed as u32
    }
}
