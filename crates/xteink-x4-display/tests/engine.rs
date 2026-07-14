use xteink_x4_display::{
    engine::{apply_page_turn, ControllerRamState, DisplayEngine},
    events::{
        CompletionStatus, DisplayEventKind, DisplayPhase, DisplayRequest, PageRefreshMode, PageTurn,
    },
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
fn externally_rendered_differential_page_commits_without_recovery_seed() {
    let mut engine = DisplayEngine::new(6);
    engine.commit_differential_page(4, 1_000).unwrap();

    assert_eq!(engine.current_page(), 4);
    assert_eq!(engine.phase(), DisplayPhase::GrayDelay);
    assert_eq!(engine.controller_state(), ControllerRamState::BwBaseReady);
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
fn balanced_refresh_mode_uses_standard_bw_turn_for_generic_request() {
    let mut engine = DisplayEngine::new_with_refresh_mode(3, PageRefreshMode::Balanced);
    engine.commit_page(0).unwrap();
    let mut backend = Backend {
        now: 25,
        ..Backend::default()
    };
    let mut events = Events::default();

    let completion = block_on(engine.request(
        DisplayRequest::Turn {
            turn: PageTurn::Next,
            sequence: Some(11),
        },
        &mut backend,
        &mut events,
        10,
    ));

    assert_eq!(completion.status, CompletionStatus::Ok);
    assert_eq!(completion.page, 1);
    assert_eq!(engine.refresh_mode(), PageRefreshMode::Balanced);
    assert_eq!(backend.operations, ["render-bw"]);
    assert_eq!(engine.phase(), DisplayPhase::GrayDelay);
}

#[test]
fn fast_refresh_mode_uses_preview_turn_for_generic_request() {
    let mut engine = DisplayEngine::new_with_refresh_mode(3, PageRefreshMode::Fast);
    engine.commit_page(0).unwrap();
    let mut backend = Backend {
        now: 25,
        ..Backend::default()
    };
    let mut events = Events::default();

    let completion = block_on(engine.request(
        DisplayRequest::Turn {
            turn: PageTurn::Next,
            sequence: Some(12),
        },
        &mut backend,
        &mut events,
        10,
    ));

    assert_eq!(completion.status, CompletionStatus::Ok);
    assert_eq!(completion.page, 1);
    assert_eq!(engine.refresh_mode(), PageRefreshMode::Fast);
    assert_eq!(backend.operations, ["preview"]);
    assert_eq!(engine.phase(), DisplayPhase::PreviewDelay);
}

#[test]
fn refresh_mode_change_applies_to_future_page_requests() {
    let mut engine = DisplayEngine::new_with_refresh_mode(4, PageRefreshMode::Balanced);
    engine.commit_page(0).unwrap();
    let mut backend = Backend {
        now: 25,
        ..Backend::default()
    };
    let mut events = Events::default();

    let set_completion = block_on(engine.request(
        DisplayRequest::SetRefreshMode {
            mode: PageRefreshMode::Fast,
            sequence: Some(21),
        },
        &mut backend,
        &mut events,
        5,
    ));
    let turn_completion = block_on(engine.request(
        DisplayRequest::Turn {
            turn: PageTurn::Next,
            sequence: Some(22),
        },
        &mut backend,
        &mut events,
        10,
    ));

    assert_eq!(set_completion.status, CompletionStatus::Ok);
    assert_eq!(turn_completion.page, 1);
    assert_eq!(engine.refresh_mode(), PageRefreshMode::Fast);
    assert_eq!(backend.operations, ["preview"]);
    assert!(events.0.iter().any(|event| matches!(
        event.kind,
        DisplayEventKind::RefreshModeChanged(PageRefreshMode::Fast)
    )));
}

#[test]
fn preview_turn_commits_target_and_defers_bw_cleanup() {
    let mut engine = DisplayEngine::new_with_refresh_mode(3, PageRefreshMode::Fast);
    engine.commit_page(0).unwrap();
    let mut backend = Backend {
        now: 25,
        ..Backend::default()
    };
    let mut events = Events::default();

    let completion = block_on(engine.request(
        DisplayRequest::Turn {
            turn: PageTurn::Next,
            sequence: Some(17),
        },
        &mut backend,
        &mut events,
        10,
    ));

    assert_eq!(completion.status, CompletionStatus::Ok);
    assert_eq!(completion.page, 1);
    assert_eq!(engine.current_page(), 1);
    assert_eq!(engine.phase(), DisplayPhase::PreviewDelay);
    assert_eq!(
        engine.controller_state(),
        ControllerRamState::NeedsFullBwInputs
    );
    assert_eq!(backend.operations, ["preview"]);
    assert!(events.0.iter().any(|event| matches!(
        event.kind,
        DisplayEventKind::PageDisplayed { from: 0, page: 1 }
    )));
}

#[test]
fn preview_cleanup_runs_before_gray_delay_when_idle() {
    let mut engine = DisplayEngine::new_with_refresh_mode(3, PageRefreshMode::Fast);
    engine.commit_page(0).unwrap();
    let mut backend = Backend {
        now: 25,
        ..Backend::default()
    };
    let mut events = Events::default();
    let _ = block_on(engine.request(
        DisplayRequest::Turn {
            turn: PageTurn::Next,
            sequence: Some(18),
        },
        &mut backend,
        &mut events,
        10,
    ));
    backend.now = 40;

    let completion = block_on(engine.advance(&mut backend, &mut events, 40));

    assert!(completion.is_none());
    assert_eq!(backend.operations, ["preview", "cleanup-bw"]);
    assert_eq!(engine.phase(), DisplayPhase::GrayDelay);
    assert_eq!(engine.controller_state(), ControllerRamState::BwBaseReady);
}

#[test]
fn turn_after_preview_uses_previewed_page_as_current_and_skips_cleanup() {
    let mut engine = DisplayEngine::new_with_refresh_mode(4, PageRefreshMode::Fast);
    engine.commit_page(0).unwrap();
    let mut backend = Backend {
        now: 25,
        ..Backend::default()
    };
    let mut events = Events::default();
    let _ = block_on(engine.request(
        DisplayRequest::Turn {
            turn: PageTurn::Next,
            sequence: Some(19),
        },
        &mut backend,
        &mut events,
        10,
    ));

    let completion = block_on(engine.request(
        DisplayRequest::Turn {
            turn: PageTurn::Next,
            sequence: Some(20),
        },
        &mut backend,
        &mut events,
        30,
    ));

    assert_eq!(completion.status, CompletionStatus::Ok);
    assert_eq!(completion.page, 2);
    assert_eq!(backend.operations, ["preview", "preview"]);
    assert!(!backend.operations.contains(&"cleanup-bw"));
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
fn initialization_at_resume_page_seeds_and_commits_that_page() {
    let mut engine = DisplayEngine::new(4);
    let mut backend = Backend::default();
    let mut events = Events::default();

    let completion = block_on(engine.initialize_at(2, &mut backend, &mut events, 0));

    assert!(completion.is_none());
    assert_eq!(engine.current_page(), 2);
    assert_eq!(backend.operations, ["init-bw", "recover"]);
    assert!(events
        .0
        .iter()
        .any(|event| matches!(event.kind, DisplayEventKind::RecoveryCompleted { page: 2 })));
}

#[test]
fn initialization_rejects_page_outside_book_without_panel_io() {
    let mut engine = DisplayEngine::new(2);
    let mut backend = Backend::default();
    let mut events = Events::default();

    let completion = block_on(engine.initialize_at(2, &mut backend, &mut events, 0)).unwrap();

    assert_eq!(completion.status, CompletionStatus::Error);
    assert!(backend.operations.is_empty());
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
