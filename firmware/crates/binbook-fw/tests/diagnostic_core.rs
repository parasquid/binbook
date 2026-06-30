#![cfg(feature = "diagnostic-console")]

use binbook_fw::diag_log::{DiagDeduper, DiagEvent, DiagLog, DiagLogRecord};
#[test]
fn diag_log_push_assigns_ascending_sequence_numbers() {
    let mut log = DiagLog::<16>::new();
    log.push(
        100,
        DiagEvent {
            level: 2,
            subsystem: 3,
            event: 0x0010,
            arg0: 0,
            arg1: 0,
            arg2: 0,
        },
    );
    log.push(
        200,
        DiagEvent {
            level: 2,
            subsystem: 3,
            event: 0x0011,
            arg0: 0,
            arg1: 0,
            arg2: 0,
        },
    );
    let mut buf = [DiagLogRecord::default(); 16];
    let result = log.read_from_sequence(0, &mut buf);
    assert_eq!(result.record_count, 2);
    assert_eq!(buf[0].sequence, 0);
    assert_eq!(buf[1].sequence, 1);
}

#[test]
fn diag_log_read_from_cursor_returns_records_in_order() {
    let mut log = DiagLog::<4>::new();
    for i in 0..4u32 {
        log.push(
            i * 100,
            DiagEvent {
                level: 2,
                subsystem: 3,
                event: i as u16,
                arg0: 0,
                arg1: 0,
                arg2: 0,
            },
        );
    }
    let mut buf = [DiagLogRecord::default(); 4];
    let result = log.read_from_sequence(0, &mut buf);
    assert_eq!(result.record_count, 4);
    assert_eq!(buf[0].event, 0);
    assert_eq!(buf[3].event, 3);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_push_overwrites_oldest_and_increments_dropped() {
    let mut log = DiagLog::<4>::new();
    for i in 0..8u32 {
        log.push(
            i * 100,
            DiagEvent {
                level: 2,
                subsystem: 3,
                event: i as u16,
                arg0: 0,
                arg1: 0,
                arg2: 0,
            },
        );
    }
    assert_eq!(log.dropped_records(), 4);
    let mut buf = [DiagLogRecord::default(); 4];
    let result = log.read_from_sequence(0, &mut buf);
    assert_eq!(result.record_count, 4);
    assert_eq!(buf[0].event, 4);
    assert_eq!(buf[3].event, 7);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_clear_resets_records_and_dropped() {
    let mut log = DiagLog::<4>::new();
    for i in 0..6u32 {
        log.push(
            i * 100,
            DiagEvent {
                level: 2,
                subsystem: 3,
                event: i as u16,
                arg0: 0,
                arg1: 0,
                arg2: 0,
            },
        );
    }
    assert_eq!(log.dropped_records(), 2);
    log.clear();
    assert_eq!(log.dropped_records(), 0);
    let mut buf = [DiagLogRecord::default(); 4];
    let result = log.read_from_sequence(0, &mut buf);
    assert_eq!(result.record_count, 0);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_deduper_suppresses_idle_repeats() {
    let mut deduper = DiagDeduper::new();
    let mut log = DiagLog::<16>::new();
    for _ in 0..100 {
        deduper.push_idle_or_summary(&mut log, 50);
    }
    let mut buf = [DiagLogRecord::default(); 16];
    let result = log.read_from_sequence(0, &mut buf);
    assert!(
        result.record_count <= 2,
        "expected at most 2 idle records, got {}",
        result.record_count
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_record_is_plain_copy() {
    let a = DiagLogRecord::default();
    let b = a;
    assert_eq!(a.sequence, b.sequence);
}
