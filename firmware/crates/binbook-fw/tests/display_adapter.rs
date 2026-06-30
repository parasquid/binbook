use binbook_fw::{
    async_refresh::{DISPLAY_COMPLETION_CAPACITY, PAGE_TURN_QUEUE_CAPACITY},
    input::PageTurn,
    runtime_engine::{map_display_event, RuntimeEventKind},
};
use xteink_x4_display::events::{DisplayEvent, DisplayEventKind, PageTurn as DisplayPageTurn};

#[test]
fn semantic_boundary_event_maps_without_changing_sequence_or_turn() {
    let mapped = map_display_event(DisplayEvent {
        timestamp_ms: 42,
        kind: DisplayEventKind::TurnBoundaryNoop {
            sequence: Some(7),
            page: 0,
            turn: DisplayPageTurn::Previous,
        },
    });
    assert_eq!(mapped.timestamp_ms, 42);
    assert!(matches!(
        mapped.kind,
        RuntimeEventKind::TurnBoundaryNoop {
            sequence: Some(7),
            page: 0,
            turn: PageTurn::Previous
        }
    ));
}

#[test]
fn firmware_queue_capacities_remain_protocol_sized() {
    assert_eq!(PAGE_TURN_QUEUE_CAPACITY, 16);
    assert_eq!(DISPLAY_COMPLETION_CAPACITY, 16);
}
