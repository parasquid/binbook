mod common;

use common::{driver, Delay};
use ssd1677_driver::Error;

#[test]
fn sync_busy_wait_times_out_at_configured_limit() {
    let (mut driver, _, busy) = driver(3);
    busy.0.set(true);
    let mut delay = Delay::default();
    assert_eq!(
        driver.wait_ready_with_delay(&mut delay),
        Err(Error::Timeout)
    );
    assert_eq!(delay.0, [1_000_000; 3]);
}

#[test]
fn async_busy_wait_uses_async_delay_and_times_out() {
    let (mut driver, _, busy) = driver(2);
    busy.0.set(true);
    let mut delay = Delay::default();
    assert_eq!(
        pollster::block_on(driver.wait_ready_async(&mut delay)),
        Err(Error::Timeout)
    );
    assert_eq!(delay.0, [1_000_000; 2]);
}
