use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;
use embedded_hal_async::delay::DelayNs as AsyncDelayNs;

use crate::{Error, Ssd1677};

const POLL_INTERVAL_MS: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusyWaitOutcome {
    Ready,
    Timeout,
    PinError,
}

pub trait BusyWaitObserver {
    fn busy_wait_start(&mut self, timeout_ms: u32, busy_state: Option<bool>);
    fn busy_wait_end(&mut self, elapsed_ms: u32, outcome: BusyWaitOutcome);
}

pub struct NoopBusyWaitObserver;

impl BusyWaitObserver for NoopBusyWaitObserver {
    fn busy_wait_start(&mut self, _timeout_ms: u32, _busy_state: Option<bool>) {}

    fn busy_wait_end(&mut self, _elapsed_ms: u32, _outcome: BusyWaitOutcome) {}
}

impl<SPI, DC, RST, BUSY> Ssd1677<SPI, DC, RST, BUSY>
where
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    pub fn wait_ready_with_delay(&mut self, delay: &mut impl DelayNs) -> Result<(), Error> {
        for _ in 0..self.config.busy_timeout_ms {
            if !self.is_busy()? {
                return Ok(());
            }
            delay.delay_ms(POLL_INTERVAL_MS);
        }
        Err(Error::Timeout)
    }

    pub async fn wait_ready_async(&mut self, delay: &mut impl AsyncDelayNs) -> Result<(), Error> {
        self.wait_ready_async_observed(delay, &mut NoopBusyWaitObserver)
            .await
    }

    pub async fn wait_ready_async_observed(
        &mut self,
        delay: &mut impl AsyncDelayNs,
        observer: &mut impl BusyWaitObserver,
    ) -> Result<(), Error> {
        let initial_busy = match self.is_busy() {
            Ok(value) => value,
            Err(error) => {
                observer.busy_wait_start(self.config.busy_timeout_ms, None);
                observer.busy_wait_end(0, BusyWaitOutcome::PinError);
                return Err(error);
            }
        };
        observer.busy_wait_start(self.config.busy_timeout_ms, Some(initial_busy));
        if !initial_busy {
            observer.busy_wait_end(0, BusyWaitOutcome::Ready);
            return Ok(());
        }
        let mut elapsed_ms = 0;
        for _ in 0..self.config.busy_timeout_ms {
            if !self.is_busy()? {
                observer.busy_wait_end(elapsed_ms, BusyWaitOutcome::Ready);
                return Ok(());
            }
            delay.delay_ms(POLL_INTERVAL_MS).await;
            elapsed_ms += POLL_INTERVAL_MS;
        }
        observer.busy_wait_end(self.config.busy_timeout_ms, BusyWaitOutcome::Timeout);
        Err(Error::Timeout)
    }
}
