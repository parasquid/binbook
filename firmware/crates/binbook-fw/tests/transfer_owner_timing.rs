#![cfg(feature = "diagnostic-console")]

use binbook_fw::{
    storage_owner::StorageClient,
    transfer_owner_timing::{
        OwnerTimingEvent, OwnerTimingPhase, OwnerTimingTracker, OwnerTimingTrackerError,
    },
};

fn event(sequence: u32, phase: OwnerTimingPhase, timestamp_us: u64) -> OwnerTimingEvent {
    OwnerTimingEvent {
        client: StorageClient::Diagnostic,
        sequence,
        phase,
        timestamp_us,
    }
}

#[test]
fn correlates_queue_and_execution_durations_by_sequence_not_fifo_order() {
    let mut tracker = OwnerTimingTracker::new();
    assert_eq!(
        tracker.observe(event(10, OwnerTimingPhase::Queued, 100)),
        Ok(None)
    );
    assert_eq!(
        tracker.observe(event(11, OwnerTimingPhase::Queued, 101)),
        Ok(None)
    );
    assert_eq!(
        tracker.observe(event(11, OwnerTimingPhase::Executing, 111)),
        Ok(Some((OwnerTimingPhase::Queued, 10)))
    );
    assert_eq!(
        tracker.observe(event(11, OwnerTimingPhase::Completed, 131)),
        Ok(Some((OwnerTimingPhase::Executing, 20)))
    );
    assert_eq!(
        tracker.observe(event(10, OwnerTimingPhase::Executing, 140)),
        Ok(Some((OwnerTimingPhase::Queued, 40)))
    );
    assert_eq!(tracker.pending(), 1);
}

#[test]
fn ignores_other_clients_and_rejects_missing_or_duplicate_transitions() {
    let mut tracker = OwnerTimingTracker::new();
    let mut reader = event(1, OwnerTimingPhase::Queued, 1);
    reader.client = StorageClient::Reader;
    assert_eq!(tracker.observe(reader), Ok(None));
    assert_eq!(tracker.pending(), 0);

    assert_eq!(
        tracker.observe(event(1, OwnerTimingPhase::Queued, 2)),
        Ok(None)
    );
    assert_eq!(
        tracker.observe(event(1, OwnerTimingPhase::Queued, 3)),
        Err(OwnerTimingTrackerError::Duplicate)
    );
    assert_eq!(
        tracker.observe(event(9, OwnerTimingPhase::Completed, 4)),
        Err(OwnerTimingTrackerError::Correlation)
    );
}

#[test]
fn pending_correlation_memory_is_bounded_to_owner_queue_capacity() {
    let mut tracker = OwnerTimingTracker::new();
    for sequence in 0..8 {
        assert_eq!(
            tracker.observe(event(sequence, OwnerTimingPhase::Queued, 1)),
            Ok(None)
        );
    }
    assert_eq!(
        tracker.observe(event(8, OwnerTimingPhase::Queued, 1)),
        Err(OwnerTimingTrackerError::Full)
    );
}
