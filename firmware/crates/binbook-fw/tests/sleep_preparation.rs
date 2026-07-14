use binbook_fw::power::{PowerAck, PowerRequest, PowerRequestCompletion, SleepPreparationOutcome};

#[test]
fn only_the_matching_successful_sleep_request_authorizes_cpu_sleep() {
    assert!(
        !PowerRequestCompletion::new(PowerRequest::FullRefresh, PowerAck::Ready)
            .may_enter_cpu_deep_sleep()
    );
    assert!(
        !PowerRequestCompletion::new(PowerRequest::FullRefresh, PowerAck::Failed)
            .may_enter_cpu_deep_sleep()
    );
    assert!(
        !PowerRequestCompletion::new(PowerRequest::PrepareSleep, PowerAck::Failed)
            .may_enter_cpu_deep_sleep()
    );
    assert!(
        PowerRequestCompletion::new(PowerRequest::PrepareSleep, PowerAck::Ready)
            .may_enter_cpu_deep_sleep()
    );
}

#[test]
fn state_save_failure_cancels_sleep_before_panel_shutdown_and_selects_notice() {
    let outcome = SleepPreparationOutcome::after_state_save(false);

    assert_eq!(outcome, SleepPreparationOutcome::StateSaveFailed);
    assert_eq!(outcome.status_code(), 1);
    assert_eq!(outcome.ack(), PowerAck::Failed);
    assert!(outcome.render_sleep_cancelled());
    assert!(!outcome.should_sleep_panel());
    assert!(!outcome.may_enter_cpu_deep_sleep());
}

#[test]
fn panel_and_cpu_sleep_are_allowed_only_after_verified_state_save() {
    let pending = SleepPreparationOutcome::after_state_save(true);
    assert_eq!(pending, SleepPreparationOutcome::AwaitPanelSleep);
    assert!(pending.should_sleep_panel());
    assert!(!pending.may_enter_cpu_deep_sleep());

    let failed = SleepPreparationOutcome::after_panel_sleep(false);
    assert_eq!(failed.status_code(), 2);
    assert_eq!(failed.ack(), PowerAck::Failed);
    assert!(!failed.may_enter_cpu_deep_sleep());

    let ready = SleepPreparationOutcome::after_panel_sleep(true);
    assert_eq!(ready.status_code(), 0);
    assert_eq!(ready.ack(), PowerAck::Ready);
    assert!(ready.may_enter_cpu_deep_sleep());
}
