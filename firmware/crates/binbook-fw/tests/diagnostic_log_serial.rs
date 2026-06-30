#![cfg(feature = "diagnostic-console")]

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_records_full_layout_and_tick() {
    use binbook_fw::diag_log::{DiagEvent, DiagLog, DEFAULT_LOG_CAPACITY};

    let mut log = DiagLog::<DEFAULT_LOG_CAPACITY>::new();
    log.push(
        1000,
        DiagEvent {
            level: 2,
            subsystem: 3,
            event: 0x0010,
            arg0: -5,
            arg1: 100,
            arg2: 0,
        },
    );
    let mut records = [binbook_fw::diag_log::DiagLogRecord::default(); 1];
    let result = log.read_from_sequence(0, &mut records);
    assert_eq!(result.record_count, 1);
    assert_eq!(records[0].sequence, 0);
    assert_eq!(records[0].tick_ms, 1000);
    assert_eq!(records[0].level, 2);
    assert_eq!(records[0].subsystem, 3);
    assert_eq!(records[0].event, 0x0010);
    assert_eq!(records[0].arg0, -5);
    assert_eq!(records[0].arg1, 100);
    assert_eq!(records[0].arg2, 0);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_cursor_is_sequence_after_overwrite() {
    use binbook_fw::diag_log::{DiagEvent, DiagLog};

    let mut log = DiagLog::<4>::new();
    for i in 0..8u32 {
        log.push(
            i * 100,
            DiagEvent {
                level: 1,
                subsystem: 1,
                event: i as u16,
                arg0: 0,
                arg1: 0,
                arg2: 0,
            },
        );
    }
    let mut records = [binbook_fw::diag_log::DiagLogRecord::default(); 4];
    let result = log.read_from_sequence(4, &mut records);
    assert_eq!(result.record_count, 4);
    assert_eq!(records[0].sequence, 4);
    assert_eq!(records[1].sequence, 5);
    assert_eq!(records[2].sequence, 6);
    assert_eq!(records[3].sequence, 7);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_cursor_before_oldest_starts_at_oldest_retained() {
    use binbook_fw::diag_log::{DiagEvent, DiagLog};

    let mut log = DiagLog::<4>::new();
    for i in 0..6u32 {
        log.push(
            i * 100,
            DiagEvent {
                level: 1,
                subsystem: 1,
                event: i as u16,
                arg0: 0,
                arg1: 0,
                arg2: 0,
            },
        );
    }
    let mut records = [binbook_fw::diag_log::DiagLogRecord::default(); 4];
    let result = log.read_from_sequence(0, &mut records);
    assert_eq!(result.record_count, 4);
    assert_eq!(records[0].sequence, 2);
    assert_eq!(records[3].sequence, 5);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_clear_removes_records_and_dropped_but_keeps_sequence_monotonic() {
    use binbook_fw::diag_log::{DiagEvent, DiagLog};

    let mut log = DiagLog::<4>::new();
    for i in 0..6u32 {
        log.push(
            i * 100,
            DiagEvent {
                level: 1,
                subsystem: 1,
                event: i as u16,
                arg0: 0,
                arg1: 0,
                arg2: 0,
            },
        );
    }
    assert!(
        log.read_from_sequence(0, &mut [binbook_fw::diag_log::DiagLogRecord::default(); 4])
            .record_count
            > 0
    );
    assert!(log.dropped_records() > 0);
    log.clear();
    assert_eq!(log.dropped_records(), 0);
    let mut records = [binbook_fw::diag_log::DiagLogRecord::default(); 4];
    let result = log.read_from_sequence(0, &mut records);
    assert_eq!(result.record_count, 0);
    log.push(
        700,
        DiagEvent {
            level: 1,
            subsystem: 1,
            event: 99,
            arg0: 0,
            arg1: 0,
            arg2: 0,
        },
    );
    let result2 = log.read_from_sequence(0, &mut records);
    assert_eq!(result2.record_count, 1);
    assert_eq!(records[0].sequence, 6);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_idle_transition_records_suppressed_count() {
    use binbook_fw::diag_log::{DiagDeduper, DiagLog};

    let mut deduper = DiagDeduper::new();
    let mut log = DiagLog::<16>::new();
    deduper.push_enter_idle(&mut log, 0);
    for tick in (1..500).step_by(10) {
        deduper.push_idle_tick(&mut log, tick);
    }
    deduper.push_leave_idle(&mut log, 500);
    let mut records = [binbook_fw::diag_log::DiagLogRecord::default(); 16];
    let result = log.read_from_sequence(0, &mut records);
    assert!(result.record_count >= 2);
    assert_eq!(records[0].event, binbook_fw::diag_log::EVT_IDLE_ENTERED);
    assert_eq!(
        records[result.record_count as usize - 1].event,
        binbook_fw::diag_log::EVT_IDLE_LEFT
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_idle_summary_is_bounded_by_idle_summary_ms() {
    use binbook_fw::diag_log::{DiagDeduper, DiagLog, IDLE_SUMMARY_MS};

    let mut deduper = DiagDeduper::new();
    let mut log = DiagLog::<16>::new();
    deduper.push_enter_idle(&mut log, 0);
    for tick in (1..IDLE_SUMMARY_MS * 3).step_by(10) {
        deduper.push_idle_tick(&mut log, tick);
    }
    let mut records = [binbook_fw::diag_log::DiagLogRecord::default(); 16];
    let result = log.read_from_sequence(0, &mut records);
    let summary_count = (0..result.record_count)
        .filter(|&i| records[i as usize].event == binbook_fw::diag_log::EVT_IDLE_SUMMARY)
        .count();
    assert!(
        summary_count <= 3,
        "expected at most 3 summaries in {}ms, got {}",
        IDLE_SUMMARY_MS * 3,
        summary_count
    );
}
