use xteink_x4_display::{
    engine::{ControllerRamState, DisplayEngine},
    events::{DisplayEventKind, OperationOutcome},
};

mod common;
use common::{block_on, Backend, Events};

#[test]
fn staged_overlay_cancellation_invalidates_controller_inputs() {
    let mut engine = DisplayEngine::new(2);
    engine.begin_overlay();
    engine.cancel_overlay();
    assert_eq!(
        engine.controller_state(),
        ControllerRamState::NeedsFullBwInputs
    );
}

#[test]
fn overlay_checks_cancellation_at_the_backend_boundary() {
    let mut engine = DisplayEngine::new(2);
    let mut backend = Backend {
        gray: Ok(OperationOutcome::Cancelled),
        ..Backend::default()
    };
    let mut events = Events::default();
    block_on(engine.initialize(&mut backend, &mut events, 0));
    block_on(engine.advance(&mut backend, &mut events, 350));
    assert_eq!(
        engine.controller_state(),
        ControllerRamState::NeedsFullBwInputs
    );
    assert!(!backend.operations.contains(&"sync-base"));
    assert!(events
        .0
        .iter()
        .any(|event| matches!(event.kind, DisplayEventKind::GrayCancelled { page: 0 })));
}

#[test]
fn cancelled_background_sync_never_activates_panel() {
    let mut engine = DisplayEngine::new(2);
    let mut backend = Backend {
        sync: Ok(OperationOutcome::Cancelled),
        ..Backend::default()
    };
    let mut events = Events::default();
    block_on(engine.initialize(&mut backend, &mut events, 0));
    block_on(engine.advance(&mut backend, &mut events, 350));
    assert_eq!(
        engine.controller_state(),
        ControllerRamState::NeedsFullBwInputs
    );
    assert!(events
        .0
        .iter()
        .any(|event| matches!(event.kind, DisplayEventKind::BaseSyncCancelled { page: 0 })));
}

#[test]
fn background_sync_cancellation_invalidates_controller_inputs() {
    let mut engine = DisplayEngine::new(2);
    engine.begin_base_sync();
    engine.cancel_base_sync();
    assert_eq!(
        engine.controller_state(),
        ControllerRamState::NeedsFullBwInputs
    );
}

#[test]
fn request_epoch_change_after_overlay_skips_background_sync() {
    let mut engine = DisplayEngine::new(2);
    let mut backend = Backend {
        epoch_after_gray: Some(1),
        ..Backend::default()
    };
    let mut events = Events::default();
    block_on(engine.initialize(&mut backend, &mut events, 0));
    block_on(engine.advance(&mut backend, &mut events, 350));
    assert!(!backend.operations.contains(&"sync-base"));
    assert_eq!(
        engine.controller_state(),
        ControllerRamState::NeedsFullBwInputs
    );
    assert!(events
        .0
        .iter()
        .any(|event| matches!(event.kind, DisplayEventKind::GrayCompleted { page: 0 })));
}
