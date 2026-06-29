use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;
use embedded_hal_async::delay::DelayNs as AsyncDelayNs;

use crate::{Error, Ssd1677};

const POLL_INTERVAL_MS: u32 = 1;

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
        for _ in 0..self.config.busy_timeout_ms {
            if !self.is_busy()? {
                return Ok(());
            }
            delay.delay_ms(POLL_INTERVAL_MS).await;
        }
        Err(Error::Timeout)
    }
}
