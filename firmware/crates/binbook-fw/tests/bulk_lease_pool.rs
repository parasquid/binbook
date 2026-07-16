#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::TransferDirection;
use binbook_fw::{
    bulk_lease_pool::{BulkLeaseError, BulkLeasePool},
    bulk_slots::{BulkSlotDescriptor, TRANSFER_DATA_BYTES, TRANSFER_SLOT_COUNT},
    storage_owner::BufferSlot,
};

fn buffers() -> [&'static mut [u8; TRANSFER_DATA_BYTES]; TRANSFER_SLOT_COUNT] {
    core::array::from_fn(|_| {
        Box::leak(Box::new([0u8; TRANSFER_DATA_BYTES])) as &'static mut [u8; TRANSFER_DATA_BYTES]
    })
}

fn descriptor(generation: u32, slot: u8) -> BulkSlotDescriptor {
    BulkSlotDescriptor {
        slot,
        generation,
        offset: u32::from(slot) * 512,
        len: 512,
        direction: TransferDirection::Upload,
    }
}

#[test]
fn leases_move_one_static_slot_without_copying_or_blocking_other_slots() {
    let mut pool = BulkLeasePool::new(buffers());
    pool.begin_session(7).unwrap();
    let first = descriptor(7, 0);
    let second = descriptor(7, 1);
    pool.write(first, &[1; 512]).unwrap();
    pool.write(second, &[2; 512]).unwrap();
    let lease = pool.take(first).unwrap();
    assert_eq!(lease.slot(), BufferSlot::Bulk0);
    assert_eq!(pool.available(), 3);
    assert_eq!(pool.bytes(first), Err(BulkLeaseError::Unavailable));
    assert_eq!(pool.bytes(second).unwrap(), &[2; 512]);
    pool.restore(first, lease).unwrap();
    assert_eq!(pool.bytes(first).unwrap(), &[1; 512]);
}

#[test]
fn generation_invalidation_rejects_stale_access_but_accepts_returned_lease() {
    let mut pool = BulkLeasePool::new(buffers());
    pool.begin_session(7).unwrap();
    let stale = descriptor(7, 0);
    let lease = pool.take(stale).unwrap();
    pool.invalidate(8);
    assert_eq!(
        pool.write(stale, &[1; 512]),
        Err(BulkLeaseError::StaleGeneration)
    );
    pool.restore(stale, lease).unwrap();
    let fresh = descriptor(8, 0);
    pool.write(fresh, &[3; 512]).unwrap();
    assert_eq!(pool.bytes(fresh).unwrap(), &[3; 512]);
}
