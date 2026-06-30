#![cfg(feature = "diagnostic-console")]

#[test]
#[cfg(feature = "diagnostic-console")]
fn diag_structured_logging_event_constants_exist() {
    use binbook_fw::diag_log::{
        EVT_ADC_SAMPLE, EVT_BUTTON_EVENT, EVT_DISPLAY_ERROR, EVT_FIRMWARE_STARTED,
        EVT_IDLE_SUMMARY, EVT_PAGE_TURN, EVT_PANEL_MODE, EVT_REFRESH_DECISION, EVT_RENDER_START,
    };

    let codes = [
        EVT_FIRMWARE_STARTED,
        EVT_ADC_SAMPLE,
        EVT_BUTTON_EVENT,
        EVT_PAGE_TURN,
        EVT_RENDER_START,
        EVT_REFRESH_DECISION,
        EVT_PANEL_MODE,
        EVT_DISPLAY_ERROR,
        EVT_IDLE_SUMMARY,
    ];
    assert!(codes.iter().all(|code| *code > 0));
    for (index, code) in codes.iter().enumerate() {
        assert!(!codes[..index].contains(code));
    }
}

#[test]
#[cfg(feature = "diagnostic-console")]
fn diag_structured_logging_push_event_records_sequence() {
    use binbook_fw::diag_log::{DiagEvent, DiagLog, DEFAULT_LOG_CAPACITY};

    let mut log = DiagLog::<DEFAULT_LOG_CAPACITY>::new();

    assert_eq!(log.next_sequence(), 0);

    log.push(
        100,
        DiagEvent {
            level: 2,
            subsystem: 3,
            event: 0x0010,
            arg0: 0,
            arg1: 0,
            arg2: 42,
        },
    );
    log.push(
        200,
        DiagEvent {
            level: 3,
            subsystem: 1,
            event: 0x0800,
            arg0: 0,
            arg1: 0,
            arg2: 100,
        },
    );

    assert_eq!(log.next_sequence(), 2);
    let mut out = [binbook_fw::diag_log::DiagLogRecord::default(); 4];
    let result = log.read_from_sequence(0, &mut out);
    assert_eq!(result.record_count, 2);
    assert_eq!(out[0].sequence, 0);
    assert_eq!(out[0].level, 2);
    assert_eq!(out[0].subsystem, 3);
    assert_eq!(out[0].event, 0x0010);
    assert_eq!(out[0].arg2, 42);
    assert_eq!(out[1].sequence, 1);
    assert_eq!(out[1].level, 3);
    assert_eq!(out[1].subsystem, 1);
}

#[test]
#[cfg(feature = "diagnostic-console")]
fn diag_structured_logging_idle_deduper_bounds_records() {
    use binbook_fw::diag_log::{DiagDeduper, DiagLog, DEFAULT_LOG_CAPACITY, IDLE_SUMMARY_MS};

    let mut log = DiagLog::<DEFAULT_LOG_CAPACITY>::new();
    let mut deduper = DiagDeduper::new();

    for tick in (1..IDLE_SUMMARY_MS).step_by(10) {
        deduper.push_idle_or_summary(&mut log, tick);
    }

    assert_eq!(
        log.next_sequence(),
        0,
        "no records should be written during suppressed idle ticks"
    );

    deduper.push_idle_or_summary(&mut log, IDLE_SUMMARY_MS);
    assert_eq!(
        log.next_sequence(),
        1,
        "exactly one summary record at cadence boundary"
    );

    let mut out = [binbook_fw::diag_log::DiagLogRecord::default(); 1];
    let result = log.read_from_sequence(0, &mut out);
    assert_eq!(result.record_count, 1);
    assert_eq!(out[0].level, 2);
    assert_eq!(out[0].subsystem, 6);
    assert_eq!(out[0].event, binbook_fw::diag_log::EVT_IDLE_SUMMARY);
    assert_eq!(out[0].arg0, IDLE_SUMMARY_MS as i32 / 10);
}
