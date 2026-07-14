use xteink_x4_display::refresh::{RefreshDecision, RefreshPolicy, RefreshState};

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
fn repeated_differential_updates_never_force_automatic_full_cleanup() {
    let mut state = RefreshState::new();
    state.record_success(0, RefreshDecision::FullBwSeed);
    for page in 1..=100 {
        state.record_success(page, RefreshDecision::FullScreenDifferential);
        assert_eq!(
            state.decide(page + 1, None),
            RefreshDecision::FullScreenDifferential
        );
    }
}
