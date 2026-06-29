mod common;

use common::{data_after, driver, Delay};
use ssd1677_driver::{Command, ControllerState, RefreshMode};

#[test]
fn refresh_modes_emit_controls_and_track_power_state() {
    let cases = [
        (
            RefreshMode::Full,
            Command::UPDATE_CTRL_NORMAL,
            ControllerState::PoweredDown,
        ),
        (
            RefreshMode::Partial,
            Command::UPDATE_CTRL_FAST,
            ControllerState::Powered,
        ),
        (
            RefreshMode::Grayscale,
            Command::UPDATE_CTRL_GRAYSCALE,
            ControllerState::Powered,
        ),
    ];
    for (mode, control, state) in cases {
        let (mut driver, trace, _) = driver(4);
        driver.trigger_refresh(mode).unwrap();
        let writes = trace.0.borrow();
        assert_eq!(
            data_after(&writes, Command::DISPLAY_UPDATE_CTRL2),
            [control]
        );
        assert!(writes
            .iter()
            .any(|write| write.as_slice() == [Command::MASTER_ACTIVATION]));
        assert_eq!(driver.state(), state);
    }
}

#[test]
fn staged_activation_powers_controller_after_full_refresh() {
    let (mut driver, trace, _) = driver(4);
    let mut delay = Delay::default();
    driver
        .refresh_with_delay(RefreshMode::Full, &mut delay)
        .unwrap();
    driver
        .trigger_refresh(RefreshMode::StagedGrayscale)
        .unwrap();

    let writes = trace.0.borrow();
    let controls: Vec<_> = writes
        .windows(2)
        .filter(|pair| pair[0].as_slice() == [Command::DISPLAY_UPDATE_CTRL2])
        .map(|pair| pair[1][0])
        .collect();
    assert_eq!(
        controls,
        [
            Command::UPDATE_CTRL_NORMAL,
            Command::UPDATE_CTRL_STAGED_GRAYSCALE | 0xc0
        ]
    );
    assert_eq!(driver.state(), ControllerState::Powered);
}
