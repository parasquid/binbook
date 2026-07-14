use binbook_fw::owner_reader::{
    mounted_diagnostic_action, mounted_request_needs_gray_refinement, MountedDiagnosticAction,
    OwnerGrayRefinement, OwnerReaderError, OwnerReaderLifecycle, OwnerReaderPhase, OwnerStateFlush,
};
use binbook_fw::{
    async_refresh::{DisplayProbeKind, DisplayRequest},
    xteink_x4_display::events::PageRefreshMode,
};

#[test]
fn open_is_not_published_until_render_commits() {
    let mut reader = OwnerReaderLifecycle::new();
    reader.begin_open().unwrap();
    assert_eq!(reader.phase(), OwnerReaderPhase::Opening);
    assert_eq!(reader.metadata_opened(16, 5).unwrap(), 5);
    assert_eq!(
        reader.phase(),
        OwnerReaderPhase::RenderingOpen { target: 5 }
    );
    assert_eq!(reader.commit_open(16).unwrap(), 5);
    assert_eq!(
        reader.phase(),
        OwnerReaderPhase::Reading {
            page: 5,
            page_count: 16
        }
    );
}

#[test]
fn failed_open_and_failed_turn_preserve_visible_committed_state() {
    let mut reader = OwnerReaderLifecycle::new();
    reader.begin_open().unwrap();
    reader.metadata_opened(16, 99).unwrap();
    assert_eq!(reader.commit_open(16).unwrap(), 15);
    assert_eq!(reader.begin_turn(-1).unwrap(), (15, 14));
    assert_eq!(reader.finish_turn(false).unwrap(), 15);
    assert_eq!(reader.close().unwrap(), 15);
    assert_eq!(reader.phase(), OwnerReaderPhase::Menu);
}

#[test]
fn page_boundaries_clamp_and_only_successful_render_advances() {
    let mut reader = OwnerReaderLifecycle::new();
    reader.begin_open().unwrap();
    reader.metadata_opened(3, 0).unwrap();
    reader.commit_open(3).unwrap();
    assert_eq!(reader.begin_turn(-1).unwrap(), (0, 0));
    assert_eq!(reader.finish_turn(true).unwrap(), 0);
    assert_eq!(reader.begin_turn(1).unwrap(), (0, 1));
    assert_eq!(reader.finish_turn(true).unwrap(), 1);
    assert_eq!(reader.begin_turn(1).unwrap(), (1, 2));
    assert_eq!(reader.finish_turn(true).unwrap(), 2);
    assert_eq!(reader.begin_turn(1).unwrap(), (2, 2));
    assert_eq!(reader.finish_turn(true).unwrap(), 2);
}

#[test]
fn goto_renders_an_exact_nonadjacent_page_and_clamps_to_the_book() {
    let mut reader = OwnerReaderLifecycle::new();
    reader.begin_open().unwrap();
    reader.metadata_opened(16, 6).unwrap();
    reader.commit_open(16).unwrap();

    assert_eq!(reader.begin_goto(0).unwrap(), (6, 0));
    assert_eq!(reader.finish_turn(true).unwrap(), 0);
    assert_eq!(reader.begin_goto(15).unwrap(), (0, 15));
    assert_eq!(reader.finish_turn(true).unwrap(), 15);
    assert_eq!(reader.begin_goto(99).unwrap(), (15, 15));
    assert_eq!(reader.finish_turn(true).unwrap(), 15);
}

#[test]
fn empty_book_and_out_of_order_callbacks_are_rejected() {
    let mut reader = OwnerReaderLifecycle::new();
    assert_eq!(reader.commit_open(1), Err(OwnerReaderError::InvalidPhase));
    reader.begin_open().unwrap();
    assert_eq!(
        reader.metadata_opened(0, 0),
        Err(OwnerReaderError::EmptyBook)
    );
    assert_eq!(reader.phase(), OwnerReaderPhase::Menu);
}

#[test]
fn delayed_gray_is_due_only_for_the_current_request_epoch() {
    let mut gray = OwnerGrayRefinement::new();
    gray.schedule(4, 7, 1_000, 350);
    assert_eq!(gray.take_due(1_349, 7), None);
    assert_eq!(gray.take_due(1_350, 8), None);

    gray.schedule(5, 8, 2_000, 350);
    assert_eq!(gray.take_due(2_350, 8), Some((5, 8)));
    assert_eq!(gray.take_due(3_000, 8), None);
}

#[test]
fn dirty_state_flush_is_postponed_by_input_and_retried_after_failure() {
    let mut flush = OwnerStateFlush::new();
    assert!(!flush.is_dirty());
    flush.mark_dirty(1_000, 2_000);
    assert_eq!(flush.wait_ms(1_500, 86_400_000), 1_500);
    flush.postpone(2_000, 2_000);
    assert!(!flush.due(3_999));
    assert!(flush.due(4_000));
    flush.record_failure(4_000, 2_000);
    assert!(flush.is_dirty());
    assert!(!flush.due(5_999));
    assert!(flush.due(6_000));
    flush.record_success();
    assert!(!flush.is_dirty());
}

#[test]
fn mounted_runtime_classifies_every_advertised_diagnostic_display_action() {
    for mode in [PageRefreshMode::Balanced, PageRefreshMode::Fast] {
        assert_eq!(
            mounted_diagnostic_action(&DisplayRequest::SetRefreshMode {
                mode,
                completion_sequence: 7,
            }),
            Some(MountedDiagnosticAction::SetRefreshMode(mode))
        );
    }

    for kind in [
        DisplayProbeKind::FullRefreshCurrent,
        DisplayProbeKind::ClearWhite,
        DisplayProbeKind::WindowCorners,
        DisplayProbeKind::PartialWindowBusyMatrix,
        DisplayProbeKind::CustomLutFastA,
        DisplayProbeKind::CustomLutFastB,
    ] {
        assert_eq!(
            mounted_diagnostic_action(&DisplayRequest::Probe {
                kind,
                completion_sequence: 9,
            }),
            Some(MountedDiagnosticAction::Probe(kind))
        );
    }

    assert_eq!(
        mounted_diagnostic_action(&DisplayRequest::MenuBack {
            completion_sequence: None,
        }),
        None
    );
}

#[test]
fn mounted_runtime_reschedules_gray_after_every_page_displaying_request() {
    assert!(mounted_request_needs_gray_refinement(
        &DisplayRequest::Goto {
            page: 4,
            completion_sequence: 1,
        }
    ));
    assert!(mounted_request_needs_gray_refinement(
        &DisplayRequest::Probe {
            kind: DisplayProbeKind::FullRefreshCurrent,
            completion_sequence: 2,
        }
    ));
    assert!(!mounted_request_needs_gray_refinement(
        &DisplayRequest::Probe {
            kind: DisplayProbeKind::ClearWhite,
            completion_sequence: 3,
        }
    ));
    assert!(!mounted_request_needs_gray_refinement(
        &DisplayRequest::SetRefreshMode {
            mode: PageRefreshMode::Fast,
            completion_sequence: 4,
        }
    ));
}
