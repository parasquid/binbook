mod common;

use ssd1677_driver::{Command, ControllerState, Error, RefreshMode};

#[test]
fn deep_sleep_sends_exact_command_and_blocks_io_until_reset_init() {
    let (mut driver, trace, _) = common::driver(10);

    driver.deep_sleep().unwrap();

    assert_eq!(trace.0.borrow().as_slice(), &[vec![Command::DEEP_SLEEP]]);
    assert_eq!(driver.state(), ControllerState::DeepSleep);
    assert_eq!(driver.write_row(0, &[0xff]), Err(Error::InvalidState));
    assert_eq!(
        driver.trigger_refresh(RefreshMode::Full),
        Err(Error::InvalidState)
    );
}
