use binbook_diagnostic_protocol::{
    ProtocolError, TransferDirection, TransferTraceSnapshot, TransferTraceState,
    TRANSFER_TRACE_FLAG_SATURATED,
};

use crate::transfer_metrics_snapshot::{add, add_u64, empty_snapshot, saturating_u32};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransferPhase {
    UsbRx,
    UsbTx,
    Framing,
    FrameCrc,
    OwnerQueue,
    OwnerExecute,
    SdRead,
    SdWrite,
    FatAllocation,
    UploadPrepare,
    DataIo,
    Crc,
    Commit,
    LibraryRefresh,
    DataPhase,
}

const PHASE_COUNT: usize = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferMetrics {
    snapshot: TransferTraceSnapshot,
    phase_depth: [u8; PHASE_COUNT],
    started_us: u64,
    updated_us: u64,
}

impl TransferMetrics {
    pub fn start(
        trace_id: u32,
        direction: TransferDirection,
        selected_window: u8,
        chunk_size: u16,
        protocol_errors: u32,
        now_us: u64,
    ) -> Self {
        Self {
            snapshot: empty_snapshot(
                trace_id,
                direction,
                selected_window,
                chunk_size,
                protocol_errors,
            ),
            phase_depth: [0; PHASE_COUNT],
            started_us: now_us,
            updated_us: now_us,
        }
    }

    pub fn enter(&mut self, phase: TransferPhase, now_us: u64) {
        self.update(now_us);
        let depth = &mut self.phase_depth[phase as usize];
        if *depth == u8::MAX {
            self.snapshot.flags |= TRANSFER_TRACE_FLAG_SATURATED;
        } else {
            *depth += 1;
        }
    }

    pub fn exit(&mut self, phase: TransferPhase, now_us: u64) -> Result<(), ProtocolError> {
        self.update(now_us);
        let depth = &mut self.phase_depth[phase as usize];
        if *depth == 0 {
            return Err(ProtocolError::InvalidValue);
        }
        *depth -= 1;
        Ok(())
    }

    pub fn record_frame(&mut self, logical_bytes: u32, encoded_rx_bytes: u32) {
        add(
            &mut self.snapshot.logical_bytes,
            logical_bytes,
            &mut self.snapshot.flags,
        );
        add(&mut self.snapshot.frames, 1, &mut self.snapshot.flags);
        add(
            &mut self.snapshot.usb_rx_bytes,
            encoded_rx_bytes,
            &mut self.snapshot.flags,
        );
    }

    pub fn record_rx_buffered(&mut self, buffered_bytes: usize) {
        self.snapshot.peak_rx_bytes = self
            .snapshot
            .peak_rx_bytes
            .max(buffered_bytes.min(usize::from(u16::MAX)) as u16);
    }

    pub fn record_usb_tx(&mut self, encoded_bytes: u32) {
        add(
            &mut self.snapshot.usb_tx_bytes,
            encoded_bytes,
            &mut self.snapshot.flags,
        );
    }

    pub fn record_sd_read(&mut self, calls: u32, blocks: u32, logical_bytes: u32) {
        add(
            &mut self.snapshot.sd_read_calls,
            calls,
            &mut self.snapshot.flags,
        );
        add(
            &mut self.snapshot.sd_read_blocks,
            blocks,
            &mut self.snapshot.flags,
        );
        add(
            &mut self.snapshot.sd_read_logical_bytes,
            logical_bytes,
            &mut self.snapshot.flags,
        );
    }

    pub fn record_sd_write(&mut self, calls: u32, blocks: u32, logical_bytes: u32) {
        add(
            &mut self.snapshot.sd_write_calls,
            calls,
            &mut self.snapshot.flags,
        );
        add(
            &mut self.snapshot.sd_write_blocks,
            blocks,
            &mut self.snapshot.flags,
        );
        add(
            &mut self.snapshot.sd_write_logical_bytes,
            logical_bytes,
            &mut self.snapshot.flags,
        );
    }

    pub fn record_sd_read_us(&mut self, elapsed_us: u64) {
        add_u64(
            &mut self.snapshot.sd_read_us,
            elapsed_us,
            &mut self.snapshot.flags,
        );
    }

    pub fn record_sd_write_us(&mut self, elapsed_us: u64) {
        add_u64(
            &mut self.snapshot.sd_write_us,
            elapsed_us,
            &mut self.snapshot.flags,
        );
    }

    pub fn record_slots(&mut self, occupied: u8) {
        self.snapshot.peak_slots = self.snapshot.peak_slots.max(occupied);
    }

    pub fn record_queue_stall(&mut self) {
        let Some(value) = self.snapshot.queue_stalls.checked_add(1) else {
            self.snapshot.queue_stalls = u16::MAX;
            self.snapshot.flags |= TRANSFER_TRACE_FLAG_SATURATED;
            return;
        };
        self.snapshot.queue_stalls = value;
    }

    pub fn record_owner_queue_us(&mut self, elapsed_us: u64) {
        add_u64(
            &mut self.snapshot.owner_queue_us,
            elapsed_us,
            &mut self.snapshot.flags,
        );
    }

    pub fn record_owner_execute_us(&mut self, elapsed_us: u64) {
        add_u64(
            &mut self.snapshot.owner_execute_us,
            elapsed_us,
            &mut self.snapshot.flags,
        );
    }

    pub fn record_instrumentation_drop(&mut self) {
        self.snapshot.flags |= TRANSFER_TRACE_FLAG_SATURATED;
    }

    pub fn record_commit_us(&mut self, elapsed_us: u64) {
        add_u64(
            &mut self.snapshot.commit_us,
            elapsed_us,
            &mut self.snapshot.flags,
        );
    }

    pub fn finish(
        &mut self,
        state: TransferTraceState,
        protocol_errors: u32,
        now_us: u64,
    ) -> TransferTraceSnapshot {
        self.snapshot(now_us);
        self.snapshot.state = state;
        self.snapshot.protocol_errors_end = protocol_errors;
        self.snapshot
    }

    pub fn snapshot(&mut self, now_us: u64) -> TransferTraceSnapshot {
        self.update(now_us);
        self.snapshot.end_to_end_us = saturating_u32(
            now_us.saturating_sub(self.started_us),
            &mut self.snapshot.flags,
        );
        self.snapshot
    }

    pub const fn trace_id(&self) -> u32 {
        self.snapshot.trace_id
    }

    pub const fn direction(&self) -> TransferDirection {
        self.snapshot.direction
    }

    pub fn set_negotiated_shape(&mut self, selected_window: u8, chunk_size: u16) {
        self.snapshot.selected_window = selected_window;
        self.snapshot.chunk_size = chunk_size;
    }

    fn update(&mut self, now_us: u64) {
        let elapsed = now_us.saturating_sub(self.updated_us);
        self.updated_us = now_us.max(self.updated_us);
        for index in 0..PHASE_COUNT {
            if self.phase_depth[index] != 0 {
                self.add_phase(index, elapsed);
            }
        }
        let usb_active = self.phase_depth[TransferPhase::UsbRx as usize] != 0
            || self.phase_depth[TransferPhase::UsbTx as usize] != 0;
        let sd_active = self.phase_depth[TransferPhase::SdRead as usize] != 0
            || self.phase_depth[TransferPhase::SdWrite as usize] != 0;
        if usb_active && sd_active {
            add_u64(
                &mut self.snapshot.usb_sd_overlap_us,
                elapsed,
                &mut self.snapshot.flags,
            );
        }
    }

    fn add_phase(&mut self, index: usize, elapsed: u64) {
        let field = match index {
            0 => &mut self.snapshot.usb_rx_us,
            1 => &mut self.snapshot.usb_tx_us,
            2 => &mut self.snapshot.framing_us,
            3 => &mut self.snapshot.frame_crc_us,
            4 => &mut self.snapshot.owner_queue_us,
            5 => &mut self.snapshot.owner_execute_us,
            6 => &mut self.snapshot.sd_read_us,
            7 => &mut self.snapshot.sd_write_us,
            8 => &mut self.snapshot.fat_allocation_us,
            9 => &mut self.snapshot.upload_prepare_us,
            10 => &mut self.snapshot.data_io_us,
            11 => &mut self.snapshot.crc_us,
            12 => &mut self.snapshot.commit_us,
            13 => &mut self.snapshot.library_refresh_us,
            14 => &mut self.snapshot.data_phase_us,
            _ => return,
        };
        add_u64(field, elapsed, &mut self.snapshot.flags);
    }
}
