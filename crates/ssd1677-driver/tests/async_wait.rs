mod common;

use common::{driver, Busy, Delay};
use ssd1677_driver::{BusyWaitObserver, BusyWaitOutcome, Error};

#[derive(Default)]
struct Observer {
    starts: Vec<(u32, Option<bool>)>,
    ends: Vec<(u32, BusyWaitOutcome)>,
}

impl BusyWaitObserver for Observer {
    fn busy_wait_start(&mut self, timeout_ms: u32, busy_state: Option<bool>) {
        self.starts.push((timeout_ms, busy_state));
    }

    fn busy_wait_end(&mut self, elapsed_ms: u32, outcome: BusyWaitOutcome) {
        self.ends.push((elapsed_ms, outcome));
    }
}

struct ReleaseAfterTwoDelays {
    busy: Busy,
    delays: Vec<u32>,
}

impl embedded_hal_async::delay::DelayNs for ReleaseAfterTwoDelays {
    async fn delay_ns(&mut self, ns: u32) {
        self.delays.push(ns);
        if self.delays.len() == 2 {
            self.busy.0.set(false);
        }
    }
}

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

#[test]
fn busy_wait_observer_records_ready_and_timeout_paths() {
    let (mut ready_driver, _, ready_busy) = driver(3);
    ready_busy.0.set(false);
    let mut ready_delay = Delay::default();
    let mut ready_observer = Observer::default();

    assert_eq!(
        pollster::block_on(
            ready_driver.wait_ready_async_observed(&mut ready_delay, &mut ready_observer)
        ),
        Ok(())
    );
    assert_eq!(ready_observer.starts, [(3, Some(false))]);
    assert_eq!(ready_observer.ends, [(0, BusyWaitOutcome::Ready)]);

    let (mut timeout_driver, _, timeout_busy) = driver(2);
    timeout_busy.0.set(true);
    let mut timeout_delay = Delay::default();
    let mut timeout_observer = Observer::default();

    assert_eq!(
        pollster::block_on(
            timeout_driver.wait_ready_async_observed(&mut timeout_delay, &mut timeout_observer)
        ),
        Err(Error::Timeout)
    );
    assert_eq!(timeout_observer.starts, [(2, Some(true))]);
    assert_eq!(timeout_observer.ends, [(2, BusyWaitOutcome::Timeout)]);
    assert_eq!(timeout_delay.0, [1_000_000; 2]);
}

#[test]
fn busy_wait_observer_reports_elapsed_poll_time_when_ready_after_delay() {
    let (mut driver, _, busy) = driver(4);
    busy.0.set(true);
    let mut delay = ReleaseAfterTwoDelays {
        busy: busy.clone(),
        delays: Vec::new(),
    };
    let mut observer = Observer::default();

    assert_eq!(
        pollster::block_on(driver.wait_ready_async_observed(&mut delay, &mut observer)),
        Ok(())
    );

    assert_eq!(delay.delays, [1_000_000; 2]);
    assert_eq!(observer.starts, [(4, Some(true))]);
    assert_eq!(observer.ends, [(2, BusyWaitOutcome::Ready)]);
}
