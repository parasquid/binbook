#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::PanelModeCode;
use binbook_fw::{
    async_refresh::{DisplayRequest, RefreshPhase},
    diag::DiagnosticSnapshot,
    diag_log::{
        DiagLogRecord, EVT_BUSY_WAIT_END, EVT_DISPLAY_REQUEST_END, EVT_INPUT_DECISION,
        EVT_INPUT_TRANSITION, EVT_REFRESH_PHASE, EVT_REQUEST_ENQUEUE, EVT_REQUEST_RECEIVE,
        EVT_TURN_BOUNDARY_NOOP, EVT_TURN_DROPPED, EVT_TURN_STARTED,
    },
    input::{Button, InputDecision, PageTurn},
    runtime_aggregator::RuntimeAggregator,
    runtime_engine::{
        BusyWaitSite, BusyWaitStatus, RequestEnqueueStatus, RuntimeCompletionStatus, RuntimeEvent,
        RuntimeEventKind, RuntimeRequestKind,
    },
};

fn event(timestamp_ms: u64, kind: RuntimeEventKind) -> RuntimeEvent {
    RuntimeEvent { timestamp_ms, kind }
}

fn aggregator<const N: usize>() -> RuntimeAggregator<N, 32> {
    RuntimeAggregator::new(DiagnosticSnapshot {
        current_page: 0,
        page_count: 4,
        panel_mode: PanelModeCode::Unknown,
        dropped_log_count: 0,
        protocol_error_count: 0,
        last_error: 0,
    })
}
#[test]
fn queue_drops_and_real_phase_events_are_recorded_with_origin_timestamps() {
    let mut aggregator = aggregator::<2>();
    aggregator.commit(event(
        350,
        RuntimeEventKind::PhaseChanged(RefreshPhase::GrayRefreshing),
    ));
    aggregator.commit(event(
        400,
        RuntimeEventKind::TurnDropped {
            turn: PageTurn::Next,
        },
    ));

    let mut records = [DiagLogRecord::default(); 4];
    let result = aggregator.log().read_from_sequence(0, &mut records);
    assert_eq!(result.record_count, 2);
    assert_eq!(records[0].event, EVT_REFRESH_PHASE);
    assert_eq!(records[0].tick_ms, 350);
    assert_eq!(records[1].event, EVT_TURN_DROPPED);
    assert_eq!(records[1].tick_ms, 400);
}

#[test]
fn navigation_burst_events_keep_exact_log_argument_layouts() {
    let mut aggregator = aggregator::<2>();
    for kind in [
        RuntimeEventKind::InputTransition {
            ch1: 500,
            ch2: 4095,
            observed: Some(Button::Right),
        },
        RuntimeEventKind::InputDecision {
            observed: Some(Button::Right),
            decision: InputDecision::Press(Button::Right),
            elapsed_ms: 101,
        },
        RuntimeEventKind::TurnStarted {
            sequence: Some(44),
            from: 8,
            target: 9,
        },
        RuntimeEventKind::TurnBoundaryNoop {
            sequence: Some(45),
            page: 0,
            turn: PageTurn::Previous,
        },
    ] {
        aggregator.commit(event(123, kind));
    }

    let mut records = [DiagLogRecord::default(); 4];
    aggregator.log().read_from_sequence(0, &mut records);
    assert_eq!(
        (
            records[0].event,
            records[0].arg0,
            records[0].arg1,
            records[0].arg2
        ),
        (EVT_INPUT_TRANSITION, 500, 4095, Button::Right as i32)
    );
    assert_eq!(
        (
            records[1].event,
            records[1].arg0,
            records[1].arg1,
            records[1].arg2
        ),
        (EVT_INPUT_DECISION, Button::Right as i32, 0, 101)
    );
    assert_eq!(
        (
            records[2].event,
            records[2].arg0,
            records[2].arg1,
            records[2].arg2
        ),
        (EVT_TURN_STARTED, 44, 8, 9)
    );
    assert_eq!(
        (
            records[3].event,
            records[3].arg0,
            records[3].arg1,
            records[3].arg2
        ),
        (EVT_TURN_BOUNDARY_NOOP, 45, 0, PageTurn::Previous as i32)
    );
}

#[test]
fn timing_request_enqueue_maps_to_input_log_record() {
    let mut aggregator = aggregator::<2>();
    aggregator.commit(event(
        500,
        RuntimeEventKind::RequestEnqueue {
            kind: RuntimeRequestKind::Turn,
            sequence: Some(7),
            status: RequestEnqueueStatus::Ok,
        },
    ));

    let mut records = [DiagLogRecord::default(); 1];
    let result = aggregator.log().read_from_sequence(0, &mut records);
    assert_eq!(result.record_count, 1);
    assert_eq!(records[0].tick_ms, 500);
    assert_eq!(records[0].event, EVT_REQUEST_ENQUEUE);
    assert_eq!(records[0].subsystem, 2);
    assert_eq!(
        (records[0].arg0, records[0].arg1, records[0].arg2),
        (0, 7, 0)
    );
}

#[test]
fn timing_request_receive_maps_to_navigation_log_record() {
    let mut aggregator = aggregator::<2>();
    aggregator.commit(event(
        502,
        RuntimeEventKind::RequestReceive {
            kind: RuntimeRequestKind::Turn,
            sequence: Some(7),
            queue_age_ms: Some(2),
        },
    ));

    let mut records = [DiagLogRecord::default(); 1];
    let result = aggregator.log().read_from_sequence(0, &mut records);
    assert_eq!(result.record_count, 1);
    assert_eq!(records[0].tick_ms, 502);
    assert_eq!(records[0].event, EVT_REQUEST_RECEIVE);
    assert_eq!(records[0].subsystem, 3);
    assert_eq!(
        (records[0].arg0, records[0].arg1, records[0].arg2),
        (0, 7, 2)
    );
}

#[test]
fn timing_display_request_end_maps_status_and_duration() {
    let mut aggregator = aggregator::<2>();
    aggregator.commit(event(
        900,
        RuntimeEventKind::DisplayRequestEnd {
            kind: RuntimeRequestKind::Turn,
            duration_ms: 398,
            status: RuntimeCompletionStatus::Error,
        },
    ));

    let mut records = [DiagLogRecord::default(); 1];
    let result = aggregator.log().read_from_sequence(0, &mut records);
    assert_eq!(result.record_count, 1);
    assert_eq!(records[0].event, EVT_DISPLAY_REQUEST_END);
    assert_eq!(records[0].level, 4);
    assert_eq!(
        (records[0].arg0, records[0].arg1, records[0].arg2),
        (0, 398, 1)
    );
}

#[test]
fn timing_busy_wait_end_maps_timeout_as_error() {
    let mut aggregator = aggregator::<2>();
    aggregator.commit(event(
        810,
        RuntimeEventKind::BusyWaitEnd {
            site: BusyWaitSite::BwRefresh,
            elapsed_ms: 60_000,
            status: BusyWaitStatus::Timeout,
        },
    ));

    let mut records = [DiagLogRecord::default(); 1];
    let result = aggregator.log().read_from_sequence(0, &mut records);
    assert_eq!(result.record_count, 1);
    assert_eq!(records[0].event, EVT_BUSY_WAIT_END);
    assert_eq!(records[0].level, 4);
    assert_eq!(
        (records[0].arg0, records[0].arg1, records[0].arg2),
        (2, 60_000, 1)
    );
}

#[test]
fn request_kind_for_turn_is_stable() {
    let request = DisplayRequest::Turn {
        turn: PageTurn::Next,
        completion_sequence: Some(12),
    };

    assert_eq!(
        RuntimeRequestKind::from_request(&request),
        RuntimeRequestKind::Turn
    );
    assert_eq!(RuntimeRequestKind::from_request(&request).log_code(), 0);
}

#[test]
fn request_enqueue_status_for_full_channel_is_dropped() {
    assert_eq!(RequestEnqueueStatus::Full.log_code(), 1);
    assert_ne!(
        RequestEnqueueStatus::Ok.log_code(),
        RequestEnqueueStatus::Full.log_code()
    );
}

#[test]
fn request_receive_uses_nonnegative_queue_age_when_sent_timestamp_exists() {
    assert_eq!(
        binbook_fw::runtime_engine::queue_age_ms(104, Some(100)),
        Some(4)
    );
    assert_eq!(
        binbook_fw::runtime_engine::queue_age_ms(100, Some(104)),
        Some(0)
    );
    assert_eq!(binbook_fw::runtime_engine::queue_age_ms(104, None), None);
}

#[test]
fn log_clear_is_nonempty_and_status_and_logs_are_owned_by_aggregator() {
    let mut aggregator = aggregator::<2>();
    aggregator.commit(event(
        1,
        RuntimeEventKind::TurnDropped {
            turn: PageTurn::Previous,
        },
    ));
    assert_eq!(aggregator.log().record_count(), 1);
    let next = aggregator.clear_log();
    assert_eq!(aggregator.log().record_count(), 0);
    assert_eq!(next, 1);
    assert_eq!(aggregator.status_payload().page_count, 4);
}
