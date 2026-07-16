use binbook_diagnostic_protocol::{
    encode_transfer_trace_snapshot, FrameDecodeEvent, Status, TransferTraceAction,
    TransferTraceRequest, TransferTraceState, TRANSFER_TRACE_SNAPSHOT_BYTES,
};
#[cfg(feature = "sd-storage")]
use embedded_sd_storage::{BlockIoEvent, BlockIoOperation, BlockIoPhase};
use heapless::Vec;

#[cfg(feature = "sd-storage")]
use crate::transfer_block_io::BlockIoAggregate;
use crate::{
    transfer_metrics::{TransferMetrics, TransferPhase},
    transfer_owner_timing::{OwnerTimingEvent, OwnerTimingPhase, OwnerTimingTracker},
    transfer_trace_store::TransferTraceStore,
};

pub type TransferTracePayload = Vec<u8, TRANSFER_TRACE_SNAPSHOT_BYTES>;

pub struct TransferTraceCommand {
    store: TransferTraceStore,
    owner_timing: OwnerTimingTracker,
    commit_sequences: Vec<u32, 2>,
}

impl TransferTraceCommand {
    pub const fn new() -> Self {
        Self {
            store: TransferTraceStore::new(),
            owner_timing: OwnerTimingTracker::new(),
            commit_sequences: Vec::new(),
        }
    }

    pub fn handle(
        &mut self,
        request: TransferTraceRequest,
        protocol_errors: u32,
        now_us: u64,
    ) -> Result<Option<TransferTracePayload>, Status> {
        match request.action {
            TransferTraceAction::Start => {
                let id = self
                    .store
                    .start(request.direction, 2, 3_584, protocol_errors, now_us)?;
                self.owner_timing = OwnerTimingTracker::new();
                self.commit_sequences.clear();
                self.snapshot(id, now_us).map(Some)
            }
            TransferTraceAction::Get => self.snapshot(request.trace_id, now_us).map(Some),
            TransferTraceAction::Clear => {
                self.store.clear(request.trace_id)?;
                Ok(None)
            }
        }
    }

    pub fn metrics_mut(&mut self) -> Option<&mut TransferMetrics> {
        self.store.metrics_mut()
    }

    pub fn active_trace_id(&mut self) -> Option<u32> {
        self.metrics_mut().map(|metrics| metrics.trace_id())
    }

    pub fn active_direction(&mut self) -> Option<binbook_diagnostic_protocol::TransferDirection> {
        self.metrics_mut().map(|metrics| metrics.direction())
    }

    pub fn observe_decode(&mut self, event: FrameDecodeEvent, now_us: u64) {
        let Some(metrics) = self.metrics_mut() else {
            return;
        };
        match event {
            FrameDecodeEvent::FramingStart => metrics.enter(TransferPhase::Framing, now_us),
            FrameDecodeEvent::FramingEnd => {
                let _ = metrics.exit(TransferPhase::Framing, now_us);
            }
            FrameDecodeEvent::CrcStart => metrics.enter(TransferPhase::FrameCrc, now_us),
            FrameDecodeEvent::CrcEnd => {
                let _ = metrics.exit(TransferPhase::FrameCrc, now_us);
            }
            FrameDecodeEvent::Complete {
                logical_bytes,
                encoded_bytes,
            } => metrics.record_frame(u32::from(logical_bytes), u32::from(encoded_bytes)),
        }
    }

    pub fn observe_owner_timing(&mut self, event: OwnerTimingEvent) {
        if self.metrics_mut().is_none() {
            return;
        }
        let sequence = event.sequence;
        let completed = event.phase == OwnerTimingPhase::Completed;
        let Ok(Some((phase, elapsed_us))) = self.owner_timing.observe(event) else {
            return;
        };
        let commit =
            phase == OwnerTimingPhase::Executing && self.commit_sequences.contains(&sequence);
        let Some(metrics) = self.metrics_mut() else {
            return;
        };
        match phase {
            OwnerTimingPhase::Queued => metrics.record_owner_queue_us(elapsed_us),
            OwnerTimingPhase::Executing => metrics.record_owner_execute_us(elapsed_us),
            OwnerTimingPhase::Completed => {}
        }
        if commit {
            metrics.record_commit_us(elapsed_us);
        }
        if completed {
            if let Some(index) = self
                .commit_sequences
                .iter()
                .position(|item| *item == sequence)
            {
                self.commit_sequences.remove(index);
            }
        }
    }

    pub fn mark_commit_sequence(&mut self, sequence: u32) {
        if self.metrics_mut().is_some() && !self.commit_sequences.contains(&sequence) {
            let _ = self.commit_sequences.push(sequence);
        }
    }

    #[cfg(feature = "sd-storage")]
    pub fn observe_block_io(&mut self, event: BlockIoEvent, now_us: u64) {
        let phase = match event.operation {
            BlockIoOperation::Read => TransferPhase::SdRead,
            BlockIoOperation::Write => TransferPhase::SdWrite,
        };
        let Some(metrics) = self.metrics_mut() else {
            return;
        };
        match event.phase {
            BlockIoPhase::Start => metrics.enter(phase, now_us),
            BlockIoPhase::Complete => {
                let _ = metrics.exit(phase, now_us);
                let blocks = u32::from(event.blocks);
                let bytes = blocks.saturating_mul(512);
                match event.operation {
                    BlockIoOperation::Read => metrics.record_sd_read(1, blocks, bytes),
                    BlockIoOperation::Write => metrics.record_sd_write(1, blocks, bytes),
                }
            }
        }
    }

    #[cfg(feature = "sd-storage")]
    pub fn observe_block_io_aggregate(&mut self, value: BlockIoAggregate) {
        let Some(metrics) = self.metrics_mut() else {
            return;
        };
        metrics.record_sd_read(
            value.read_calls,
            value.read_blocks,
            value.read_blocks.saturating_mul(512),
        );
        metrics.record_sd_write(
            value.write_calls,
            value.write_blocks,
            value.write_blocks.saturating_mul(512),
        );
        metrics.record_sd_read_us(u64::from(value.read_us));
        metrics.record_sd_write_us(u64::from(value.write_us));
        if value.anomalies != 0 {
            metrics.record_instrumentation_drop();
        }
    }

    pub fn record_instrumentation_drops(&mut self, drops: u32) {
        if drops != 0 {
            if let Some(metrics) = self.metrics_mut() {
                metrics.record_instrumentation_drop();
            }
        }
    }

    pub fn enter_phase(&mut self, phase: TransferPhase, now_us: u64) {
        if let Some(metrics) = self.metrics_mut() {
            metrics.enter(phase, now_us);
        }
    }

    pub fn exit_phase(&mut self, phase: TransferPhase, now_us: u64) {
        if let Some(metrics) = self.metrics_mut() {
            let _ = metrics.exit(phase, now_us);
        }
    }

    pub fn record_usb_tx(&mut self, encoded_bytes: usize) {
        if let Some(metrics) = self.metrics_mut() {
            metrics.record_usb_tx(encoded_bytes.min(u32::MAX as usize) as u32);
        }
    }

    pub fn record_rx_buffered(&mut self, buffered_bytes: usize) {
        if let Some(metrics) = self.metrics_mut() {
            metrics.record_rx_buffered(buffered_bytes);
        }
    }

    pub fn set_negotiated_shape(&mut self, selected_window: u8, chunk_size: u16) {
        if let Some(metrics) = self.metrics_mut() {
            metrics.set_negotiated_shape(selected_window, chunk_size);
        }
    }

    pub fn finish(
        &mut self,
        state: TransferTraceState,
        protocol_errors: u32,
        now_us: u64,
        library_pending: bool,
    ) -> Result<(), Status> {
        self.store
            .finish(state, protocol_errors, now_us, library_pending)
            .map(|_| ())
    }

    pub fn complete_library_refresh(&mut self, trace_id: u32, elapsed_us: u32) {
        self.store.complete_library_refresh(trace_id, elapsed_us);
    }

    fn snapshot(&mut self, trace_id: u32, now_us: u64) -> Result<TransferTracePayload, Status> {
        let snapshot = self.store.get(trace_id, now_us)?;
        let mut encoded = [0; TRANSFER_TRACE_SNAPSHOT_BYTES];
        let len = encode_transfer_trace_snapshot(&snapshot, &mut encoded)
            .map_err(|_| Status::InternalError)?;
        let mut payload = TransferTracePayload::new();
        payload
            .extend_from_slice(&encoded[..len])
            .map_err(|_| Status::InternalError)?;
        Ok(payload)
    }
}

impl Default for TransferTraceCommand {
    fn default() -> Self {
        Self::new()
    }
}
