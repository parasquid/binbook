use xteink_x4_display::{
    engine::{apply_page_turn, DisplayEngine},
    events::{CompletionStatus, DisplayEventKind, DisplayRequest, PageTurn},
};

mod common;
use common::{block_on, Backend, Events, FAILURE};

#[test]
fn fifo_relative_turns_resolve_from_committed_page() {
    let mut engine = DisplayEngine::new(3);
    engine.commit_page(1).unwrap();
    assert_eq!(engine.target_for(PageTurn::Next), 2);
    engine.commit_page(2).unwrap();
    assert_eq!(engine.target_for(PageTurn::Next), 2);
    assert_eq!(apply_page_turn(0, 3, PageTurn::Previous), 0);
}

#[test]
fn page_is_committed_only_after_bw_operation_succeeds() {
    let mut engine = DisplayEngine::new(3);
    engine.commit_page(0).unwrap();
    let mut backend = Backend::default();
    let mut events = Events::default();
    let completion = block_on(engine.request(
        DisplayRequest::Turn {
            turn: PageTurn::Next,
            sequence: Some(7),
        },
        &mut backend,
        &mut events,
        10,
    ));
    assert_eq!(completion.status, CompletionStatus::Ok);
    assert_eq!(completion.page, 1);
    assert_eq!(backend.operations, ["render-bw"]);
    assert!(matches!(
        events.0[0].kind,
        DisplayEventKind::TurnStarted {
            from: 0,
            target: 1,
            ..
        }
    ));
    assert!(events.0.iter().any(|event| matches!(
        event.kind,
        DisplayEventKind::PageDisplayed { from: 0, page: 1 }
    )));
    let displayed = events
        .0
        .iter()
        .position(|event| matches!(event.kind, DisplayEventKind::PageDisplayed { .. }))
        .unwrap();
    let delayed = events
        .0
        .iter()
        .position(|event| {
            matches!(
                event.kind,
                DisplayEventKind::PhaseChanged(xteink_x4_display::events::DisplayPhase::GrayDelay)
            )
        })
        .unwrap();
    assert!(displayed < delayed);
}

#[test]
fn initialization_reports_recovery_before_gray_delay() {
    let mut engine = DisplayEngine::new(3);
    let mut backend = Backend::default();
    let mut events = Events::default();
    block_on(engine.initialize(&mut backend, &mut events, 0));
    let recovered = events
        .0
        .iter()
        .position(|event| matches!(event.kind, DisplayEventKind::RecoveryCompleted { page: 0 }))
        .unwrap();
    let delayed = events
        .0
        .iter()
        .position(|event| {
            matches!(
                event.kind,
                DisplayEventKind::PhaseChanged(xteink_x4_display::events::DisplayPhase::GrayDelay)
            )
        })
        .unwrap();
    assert!(recovered < delayed);
}

#[test]
fn boundary_turn_completes_without_panel_operation() {
    let mut engine = DisplayEngine::new(3);
    engine.commit_page(0).unwrap();
    let mut backend = Backend::default();
    let mut events = Events::default();
    let completion = block_on(engine.request(
        DisplayRequest::Turn {
            turn: PageTurn::Previous,
            sequence: Some(8),
        },
        &mut backend,
        &mut events,
        10,
    ));
    assert_eq!(completion.page, 0);
    assert!(backend.operations.is_empty());
    assert!(events.0.iter().any(|event| matches!(
        event.kind,
        DisplayEventKind::TurnBoundaryNoop { page: 0, .. }
    )));
}

#[test]
fn rendering_failure_recovers_once_and_repeated_failure_faults() {
    let mut engine = DisplayEngine::new(3);
    engine.commit_page(0).unwrap();
    let mut backend = Backend {
        render_bw: Err(FAILURE),
        ..Backend::default()
    };
    let mut events = Events::default();
    let first = block_on(engine.request(
        DisplayRequest::Goto {
            page: 1,
            sequence: 1,
        },
        &mut backend,
        &mut events,
        0,
    ));
    assert_eq!(first.status, CompletionStatus::Ok);
    assert_eq!(engine.current_page(), 1);
    backend.recovery = Err(FAILURE);
    let second = block_on(engine.request(
        DisplayRequest::Goto {
            page: 2,
            sequence: 2,
        },
        &mut backend,
        &mut events,
        1,
    ));
    assert_eq!(second.status, CompletionStatus::Error);
    assert!(engine.faulted());
}

#[test]
fn failed_page_does_not_commit() {
    let mut engine = DisplayEngine::new(3);
    engine.commit_page(1).unwrap();
    engine.record_failure();
    assert_eq!(engine.current_page(), 1);
    assert!(engine.recovery_required());
    engine.record_failure();
    assert!(engine.faulted());
}
