#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::{
    TransferDirection, TransferTraceState, TRANSFER_TRACE_FLAG_SATURATED,
};
use binbook_fw::transfer_metrics::{TransferMetrics, TransferPhase};

#[test]
fn phase_transitions_separate_usb_sd_and_proven_overlap() {
    let mut metrics = TransferMetrics::start(7, TransferDirection::Upload, 4, 3584, 3, 100);
    metrics.enter(TransferPhase::UsbRx, 110);
    metrics.enter(TransferPhase::SdWrite, 130);
    metrics.exit(TransferPhase::UsbRx, 150).unwrap();
    metrics.exit(TransferPhase::SdWrite, 170).unwrap();
    let snapshot = metrics.finish(TransferTraceState::Succeeded, 3, 200);

    assert_eq!(snapshot.usb_rx_us, 40);
    assert_eq!(snapshot.sd_write_us, 40);
    assert_eq!(snapshot.usb_sd_overlap_us, 20);
    assert_eq!(snapshot.end_to_end_us, 100);
}

#[test]
fn nested_phase_only_stops_after_balanced_exit() {
    let mut metrics = TransferMetrics::start(7, TransferDirection::Upload, 4, 3584, 0, 0);
    metrics.enter(TransferPhase::Framing, 10);
    metrics.enter(TransferPhase::Framing, 20);
    metrics.exit(TransferPhase::Framing, 30).unwrap();
    metrics.exit(TransferPhase::Framing, 40).unwrap();
    assert_eq!(
        metrics
            .finish(TransferTraceState::Succeeded, 0, 50)
            .framing_us,
        30
    );
}

#[test]
fn counters_saturate_and_set_the_snapshot_flag() {
    let mut metrics = TransferMetrics::start(7, TransferDirection::Readback, 2, 3584, 1, 0);
    metrics.enter(TransferPhase::SdRead, 0);
    metrics
        .exit(TransferPhase::SdRead, u64::from(u32::MAX) + 10)
        .unwrap();
    metrics.record_sd_read(u32::MAX, u32::MAX, u32::MAX);
    metrics.record_sd_read(1, 1, 1);
    let snapshot = metrics.finish(
        TransferTraceState::Failed,
        u32::MAX,
        u64::from(u32::MAX) + 20,
    );
    assert_eq!(snapshot.sd_read_us, u32::MAX);
    assert_eq!(snapshot.sd_read_calls, u32::MAX);
    assert_eq!(snapshot.sd_read_blocks, u32::MAX);
    assert_eq!(snapshot.sd_read_logical_bytes, u32::MAX);
    assert_ne!(snapshot.flags & TRANSFER_TRACE_FLAG_SATURATED, 0);
}

#[test]
fn aggregate_records_counts_high_water_and_terminal_state() {
    let mut metrics = TransferMetrics::start(9, TransferDirection::Upload, 4, 3584, 10, 1_000);
    metrics.record_frame(3_584, 4_126);
    metrics.record_frame(416, 500);
    metrics.record_rx_buffered(7_200);
    metrics.record_rx_buffered(64);
    metrics.record_usb_tx(12);
    metrics.record_sd_write(2, 7, 4_000);
    metrics.record_slots(1);
    metrics.record_slots(4);
    metrics.record_queue_stall();
    let snapshot = metrics.finish(TransferTraceState::Aborted, 11, 2_000);
    assert_eq!(snapshot.state, TransferTraceState::Aborted);
    assert_eq!(snapshot.logical_bytes, 4_000);
    assert_eq!(snapshot.frames, 2);
    assert_eq!(snapshot.usb_rx_bytes, 4_626);
    assert_eq!(snapshot.peak_rx_bytes, 7_200);
    assert_eq!(snapshot.usb_tx_bytes, 12);
    assert_eq!(snapshot.sd_write_calls, 2);
    assert_eq!(snapshot.sd_write_blocks, 7);
    assert_eq!(snapshot.peak_slots, 4);
    assert_eq!(snapshot.queue_stalls, 1);
    assert_eq!(snapshot.protocol_errors_start, 10);
    assert_eq!(snapshot.protocol_errors_end, 11);
}

#[test]
fn unbalanced_exit_is_rejected_without_corrupting_time() {
    let mut metrics = TransferMetrics::start(1, TransferDirection::Upload, 1, 512, 0, 0);
    assert!(metrics.exit(TransferPhase::Commit, 10).is_err());
    assert_eq!(
        metrics.finish(TransferTraceState::Failed, 0, 20).commit_us,
        0
    );
}
