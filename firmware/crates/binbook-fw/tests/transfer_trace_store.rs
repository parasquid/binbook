#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::{
    Status, TransferDirection, TransferTraceState, TRANSFER_TRACE_FLAG_LIBRARY_PENDING,
};
use binbook_fw::{transfer_metrics::TransferPhase, transfer_trace_store::TransferTraceStore};

#[test]
fn active_trace_rejects_restart_and_wrong_ids_are_not_found() {
    let mut store = TransferTraceStore::new();
    let id = store
        .start(TransferDirection::Upload, 4, 3584, 3, 100)
        .unwrap();
    assert_ne!(id, 0);
    assert_eq!(
        store.start(TransferDirection::Readback, 2, 3584, 3, 101),
        Err(Status::BadRequest)
    );
    assert_eq!(store.get(id.wrapping_add(1), 110), Err(Status::NotFound));
    assert_eq!(store.clear(id), Err(Status::BadRequest));
}

#[test]
fn get_updates_active_time_and_terminal_trace_can_be_cleared() {
    let mut store = TransferTraceStore::new();
    let id = store
        .start(TransferDirection::Upload, 4, 3584, 0, 1_000)
        .unwrap();
    store
        .metrics_mut()
        .unwrap()
        .enter(TransferPhase::DataIo, 1_010);
    let active = store.get(id, 1_020).unwrap();
    assert_eq!(active.state, TransferTraceState::Active);
    assert_eq!(active.data_io_us, 10);
    assert_eq!(active.end_to_end_us, 20);

    let terminal = store
        .finish(TransferTraceState::Succeeded, 0, 1_030, false)
        .unwrap();
    assert_eq!(terminal.data_io_us, 20);
    assert_eq!(store.get(id, 2_000).unwrap(), terminal);
    assert_eq!(store.clear(id), Ok(()));
    assert_eq!(store.get(id, 2_001), Err(Status::NotFound));
}

#[test]
fn terminal_trace_is_replaced_by_next_start_with_nonzero_wrapping_id() {
    let mut store = TransferTraceStore::with_next_id(u32::MAX);
    let first = store
        .start(TransferDirection::Upload, 1, 512, 0, 0)
        .unwrap();
    assert_eq!(first, u32::MAX);
    store
        .finish(TransferTraceState::Aborted, 0, 1, false)
        .unwrap();
    let second = store
        .start(TransferDirection::Readback, 1, 512, 0, 2)
        .unwrap();
    assert_eq!(second, 1);
    assert_eq!(store.get(first, 3), Err(Status::NotFound));
}

#[test]
fn late_library_refresh_only_updates_the_matching_terminal_trace() {
    let mut store = TransferTraceStore::new();
    let old = store
        .start(TransferDirection::Upload, 4, 3584, 0, 0)
        .unwrap();
    let terminal = store
        .finish(TransferTraceState::Succeeded, 0, 10, true)
        .unwrap();
    assert_ne!(terminal.flags & TRANSFER_TRACE_FLAG_LIBRARY_PENDING, 0);

    store.complete_library_refresh(old.wrapping_add(1), 50);
    assert_eq!(store.get(old, 11).unwrap().library_refresh_us, 0);
    store.complete_library_refresh(old, 50);
    let completed = store.get(old, 12).unwrap();
    assert_eq!(completed.library_refresh_us, 50);
    assert_eq!(completed.flags & TRANSFER_TRACE_FLAG_LIBRARY_PENDING, 0);

    let new = store
        .start(TransferDirection::Readback, 2, 3584, 0, 20)
        .unwrap();
    store.complete_library_refresh(old, 99);
    assert_eq!(store.get(new, 21).unwrap().library_refresh_us, 0);
}
