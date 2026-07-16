#![cfg(feature = "diagnostic-console")]

use binbook_fw::bulk_idle_timeout::{BulkIdleTimeout, BULK_IDLE_TIMEOUT_MS};

#[test]
fn deadline_is_armed_touched_and_fires_once() {
    let mut timeout = BulkIdleTimeout::new();
    assert!(!timeout.is_armed());

    timeout.arm(10);
    assert_eq!(timeout.remaining_ms(10), Some(BULK_IDLE_TIMEOUT_MS));
    assert!(!timeout.take_expired(10 + BULK_IDLE_TIMEOUT_MS - 1));

    timeout.touch(20);
    assert_eq!(timeout.remaining_ms(20), Some(BULK_IDLE_TIMEOUT_MS));
    assert!(timeout.take_expired(20 + BULK_IDLE_TIMEOUT_MS));
    assert!(!timeout.is_armed());
    assert!(!timeout.take_expired(u64::MAX));
}

#[test]
fn deadline_math_saturates_and_disarm_cancels() {
    let mut timeout = BulkIdleTimeout::new();
    timeout.arm(u64::MAX - 1);
    assert_eq!(timeout.remaining_ms(u64::MAX - 1), Some(1));
    timeout.disarm();
    assert_eq!(timeout.remaining_ms(u64::MAX), None);
}
