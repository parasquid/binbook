use xteink_x4_display::{
    panel::RefreshMode,
    render::{PlaneRole, RamTarget, RenderStageStatus, RenderTimingObserver},
};

use binbook_fw::runtime_engine::{RuntimeEvent, RuntimeEventKind};

use super::RUNTIME_EVENT_CHANNEL;

pub(super) struct RuntimeRenderTimingObserver;

impl RuntimeRenderTimingObserver {
    pub(super) const fn new() -> Self {
        Self
    }
}

impl RenderTimingObserver for RuntimeRenderTimingObserver {
    fn now_ms(&self) -> u64 {
        embassy_time::Instant::now().as_millis()
    }

    fn page_metadata_read(&mut self, from: u32, target: u32, duration_ms: u32) {
        send_timing_event(RuntimeEventKind::PageMetadataRead {
            from,
            target,
            duration_ms,
        });
    }

    fn plane_write_start(&mut self, role: PlaneRole, target: RamTarget, plane_bytes: u32) {
        send_timing_event(RuntimeEventKind::PlaneWriteStart {
            role: plane_role_code(role),
            ram_target: ram_target_code(target),
            plane_bytes,
        });
    }

    fn plane_row_fill_summary(&mut self, role: PlaneRole, duration_ms: u32, row_count: u32) {
        send_timing_event(RuntimeEventKind::PlaneRowFillSummary {
            role: plane_role_code(role),
            duration_ms,
            row_count,
        });
    }

    fn plane_spi_write_summary(&mut self, role: PlaneRole, duration_ms: u32, bytes_written: u32) {
        send_timing_event(RuntimeEventKind::PlaneSpiWriteSummary {
            role: plane_role_code(role),
            duration_ms,
            bytes_written,
        });
    }

    fn plane_write_end(&mut self, role: PlaneRole, duration_ms: u32, status: RenderStageStatus) {
        send_timing_event(RuntimeEventKind::PlaneWriteEnd {
            role: plane_role_code(role),
            duration_ms,
            status: stage_status_code(status),
        });
    }

    fn refresh_trigger(&mut self, mode: RefreshMode, duration_ms: u32, status: RenderStageStatus) {
        send_timing_event(RuntimeEventKind::RefreshTrigger {
            mode: refresh_mode_code(mode),
            duration_ms,
            status: stage_status_code(status),
        });
    }
}

fn send_timing_event(kind: RuntimeEventKind) {
    let _ = RUNTIME_EVENT_CHANNEL.sender().try_send(RuntimeEvent {
        timestamp_ms: embassy_time::Instant::now().as_millis(),
        kind,
    });
}

const fn plane_role_code(role: PlaneRole) -> i32 {
    match role {
        PlaneRole::PreviousFastBase => 0,
        PlaneRole::TargetFastBase => 1,
    }
}

const fn ram_target_code(target: RamTarget) -> i32 {
    match target {
        RamTarget::Black => 0,
        RamTarget::Red => 1,
    }
}

const fn stage_status_code(status: RenderStageStatus) -> i32 {
    match status {
        RenderStageStatus::Ok => 0,
        RenderStageStatus::Error => 1,
        RenderStageStatus::Cancelled => 2,
    }
}

const fn refresh_mode_code(mode: RefreshMode) -> i32 {
    match mode {
        RefreshMode::Full => 0,
        RefreshMode::Partial => 1,
        RefreshMode::Grayscale => 2,
        RefreshMode::StagedGrayscale => 3,
    }
}
