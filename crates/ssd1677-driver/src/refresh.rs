#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshMode {
    Full,
    Partial,
    Grayscale,
    StagedGrayscale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerState {
    Unknown,
    Powered,
    PoweredDown,
}

impl<SPI, DC, RST, BUSY> Ssd1677<SPI, DC, RST, BUSY>
where
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    pub fn trigger_refresh(&mut self, mode: RefreshMode) -> Result<(), Error> {
        if matches!(mode, RefreshMode::Partial | RefreshMode::Grayscale) {
            self.command_data(Command::DISPLAY_UPDATE_CTRL1, &[0, 0])?;
        } else if mode == RefreshMode::StagedGrayscale {
            self.command_data(Command::DISPLAY_UPDATE_CTRL1, &[0])?;
        }
        let control = match mode {
            RefreshMode::Full => Command::UPDATE_CTRL_NORMAL,
            RefreshMode::Partial => Command::UPDATE_CTRL_FAST,
            RefreshMode::Grayscale => Command::UPDATE_CTRL_GRAYSCALE,
            RefreshMode::StagedGrayscale => {
                Command::UPDATE_CTRL_STAGED_GRAYSCALE
                    | (u8::from(self.state != ControllerState::Powered) * 0xc0)
            }
        };
        self.command_data(Command::DISPLAY_UPDATE_CTRL2, &[control])?;
        self.command(Command::MASTER_ACTIVATION)?;
        self.state = if mode == RefreshMode::Full {
            ControllerState::PoweredDown
        } else {
            ControllerState::Powered
        };
        Ok(())
    }

    pub fn refresh_with_delay(
        &mut self,
        mode: RefreshMode,
        delay: &mut impl DelayNs,
    ) -> Result<(), Error> {
        self.trigger_refresh(mode)?;
        self.wait_ready_with_delay(delay)
    }

    pub async fn refresh_async(
        &mut self,
        mode: RefreshMode,
        delay: &mut impl AsyncDelayNs,
    ) -> Result<(), Error> {
        self.trigger_refresh(mode)?;
        self.wait_ready_async(delay).await
    }

    pub async fn activate_staged_grayscale_async(
        &mut self,
        delay: &mut impl AsyncDelayNs,
    ) -> Result<(), Error> {
        self.trigger_refresh(RefreshMode::StagedGrayscale)?;
        self.wait_ready_async(delay).await
    }
}
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;
use embedded_hal_async::delay::DelayNs as AsyncDelayNs;

use crate::{Command, Error, Ssd1677};
