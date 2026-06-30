use xteink_x4_display::refresh::{
    RefreshDecision, RefreshPolicy, RefreshState, DEFAULT_FULL_REFRESH_CADENCE,
};

#[test]
fn refresh_policy_covers_absolute_seed_full_screen_and_gated_dirty_chunks() {
    let mut state = RefreshState::new();
    assert_eq!(state.decide(0, None), RefreshDecision::FullGrayscale);
    state.record_success(0, RefreshDecision::FullGrayscale);
    assert_eq!(state.decide(1, None), RefreshDecision::FullBwSeed);
    state.record_success(1, RefreshDecision::FullBwSeed);
    assert_eq!(
        state.decide(2, Some(3)),
        RefreshDecision::FullScreenDifferential
    );
    assert_eq!(
        state.decide_with_policy(2, Some(3), RefreshPolicy::ChunkDirtyDifferential),
        RefreshDecision::AdjacentDirtyPartial {
            changed_chunk_mask: 3
        }
    );
}

#[test]
fn cadence_forces_periodic_absolute_cleanup() {
    let mut state = RefreshState::new();
    state.record_success(0, RefreshDecision::FullBwSeed);
    for page in 1..=DEFAULT_FULL_REFRESH_CADENCE {
        state.record_success(page, RefreshDecision::FullScreenDifferential);
    }
    assert_eq!(
        state.decide(DEFAULT_FULL_REFRESH_CADENCE + 1, None),
        RefreshDecision::FullGrayscale
    );
}
