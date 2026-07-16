#![cfg(feature = "diagnostic-console")]

use binbook_fw::bulk_slots::{
    BulkSlotError, BulkSlotPool, BulkSlotState, TRANSFER_DATA_BYTES, TRANSFER_SLOT_COUNT,
};

#[test]
fn upload_slots_apply_exact_transitions_and_release_only_after_storage() {
    let mut pool = Box::new(BulkSlotPool::new());
    let generation = pool.begin_session();
    let descriptor = pool.claim_upload(0, TRANSFER_DATA_BYTES as u16).unwrap();
    assert_eq!(descriptor.generation, generation);
    assert_eq!(pool.state(descriptor.slot).unwrap(), BulkSlotState::RxReady);
    assert_eq!(pool.occupied(), 1);
    assert_eq!(pool.peak_occupied(), 1);

    pool.take_upload_for_storage(descriptor).unwrap();
    assert_eq!(
        pool.state(descriptor.slot).unwrap(),
        BulkSlotState::StorageOwned
    );
    let durable = pool.complete_upload(descriptor).unwrap();
    assert_eq!(durable, TRANSFER_DATA_BYTES as u32);
    assert_eq!(pool.state(descriptor.slot).unwrap(), BulkSlotState::Free);
    assert_eq!(pool.occupied(), 0);
}

#[test]
fn full_pool_backpressures_without_overwriting_any_slot() {
    let mut pool = Box::new(BulkSlotPool::new());
    pool.begin_session();
    let mut descriptors = Vec::new();
    for index in 0..TRANSFER_SLOT_COUNT {
        descriptors.push(pool.claim_upload((index * 512) as u32, 512).unwrap());
    }
    assert_eq!(pool.occupied(), TRANSFER_SLOT_COUNT as u8);
    assert_eq!(pool.claim_upload(2_048, 512), Err(BulkSlotError::Full));
    assert_eq!(descriptors.len(), TRANSFER_SLOT_COUNT);
}

#[test]
fn cancellation_invalidates_stale_descriptors_before_storage_touch() {
    let mut pool = Box::new(BulkSlotPool::new());
    pool.begin_session();
    let stale = pool.claim_upload(0, 512).unwrap();
    let next_generation = pool.abort_session();
    assert_ne!(stale.generation, next_generation);
    assert_eq!(pool.occupied(), 0);
    assert_eq!(
        pool.take_upload_for_storage(stale),
        Err(BulkSlotError::StaleGeneration)
    );
    let fresh = pool.claim_upload(0, 512).unwrap();
    assert_eq!(fresh.generation, next_generation);
}

#[test]
fn readback_slots_stay_owned_until_host_ack() {
    let mut pool = Box::new(BulkSlotPool::new());
    pool.begin_session();
    let descriptor = pool.reserve_read(1024, 512).unwrap();
    assert_eq!(
        pool.state(descriptor.slot).unwrap(),
        BulkSlotState::StorageOwned
    );
    pool.complete_read(descriptor).unwrap();
    assert_eq!(pool.state(descriptor.slot).unwrap(), BulkSlotState::TxReady);
    pool.mark_awaiting_ack(descriptor).unwrap();
    assert_eq!(
        pool.state(descriptor.slot).unwrap(),
        BulkSlotState::AwaitingAck
    );
    pool.ack_read(descriptor).unwrap();
    assert_eq!(pool.state(descriptor.slot).unwrap(), BulkSlotState::Free);
}

#[test]
fn wrong_transition_descriptor_or_length_is_rejected() {
    let mut pool = Box::new(BulkSlotPool::new());
    pool.begin_session();
    assert_eq!(
        pool.claim_upload(0, (TRANSFER_DATA_BYTES + 1) as u16),
        Err(BulkSlotError::Length)
    );
    let descriptor = pool.claim_upload(0, 512).unwrap();
    assert_eq!(
        pool.complete_upload(descriptor),
        Err(BulkSlotError::WrongState)
    );
    let mut wrong = descriptor;
    wrong.offset = 512;
    assert_eq!(
        pool.take_upload_for_storage(wrong),
        Err(BulkSlotError::DescriptorMismatch)
    );
}
