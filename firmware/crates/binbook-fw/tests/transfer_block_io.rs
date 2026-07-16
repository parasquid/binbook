#![cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]

use binbook_fw::transfer_block_io::BlockIoAccumulator;
use embedded_sd_storage::{BlockIoEvent, BlockIoOperation, BlockIoPhase};

fn event(operation: BlockIoOperation, phase: BlockIoPhase, blocks: u16) -> BlockIoEvent {
    BlockIoEvent {
        operation,
        phase,
        start_block: 0,
        blocks,
        success: phase == BlockIoPhase::Complete,
    }
}

#[test]
fn aggregates_more_than_the_old_channel_capacity_without_loss() {
    let accumulator = BlockIoAccumulator::new();
    for call in 0..100u64 {
        let started = 1_000 + call * 20;
        accumulator.observe(
            event(BlockIoOperation::Write, BlockIoPhase::Start, 8),
            started,
        );
        accumulator.observe(
            event(BlockIoOperation::Write, BlockIoPhase::Complete, 8),
            started + 11,
        );
    }
    let value = accumulator.take();
    assert_eq!(value.write_calls, 100);
    assert_eq!(value.write_blocks, 800);
    assert_eq!(value.write_us, 1_100);
    assert_eq!(value.anomalies, 0);
    assert_eq!(accumulator.take(), Default::default());
}

#[test]
fn separates_operations_handles_clock_wrap_and_flags_unmatched_events() {
    let accumulator = BlockIoAccumulator::new();
    accumulator.observe(
        event(BlockIoOperation::Read, BlockIoPhase::Start, 3),
        u64::from(u32::MAX) - 3,
    );
    accumulator.observe(event(BlockIoOperation::Read, BlockIoPhase::Start, 3), 1);
    accumulator.observe(
        event(BlockIoOperation::Read, BlockIoPhase::Complete, 3),
        u64::from(u32::MAX) + 4,
    );
    accumulator.observe(
        event(BlockIoOperation::Write, BlockIoPhase::Complete, 1),
        10,
    );
    let value = accumulator.take();
    assert_eq!(
        (value.read_calls, value.read_blocks, value.read_us),
        (1, 3, 7)
    );
    assert_eq!(value.write_calls, 0);
    assert_eq!(value.anomalies, 2);
}
