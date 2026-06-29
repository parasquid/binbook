use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;
use embedded_hal_async::delay::DelayNs as AsyncDelayNs;

use crate::{Command, ControllerState, Error, PanelConfig, RefreshMode, Waveform};

pub struct Ssd1677<SPI, DC, RST, BUSY> {
    pub(crate) spi: SPI,
    pub(crate) dc: DC,
    reset: RST,
    pub(crate) busy: BUSY,
    pub(crate) config: PanelConfig,
    pub(crate) state: ControllerState,
}

impl<SPI, DC, RST, BUSY> Ssd1677<SPI, DC, RST, BUSY>
where
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    pub fn new(spi: SPI, dc: DC, reset: RST, busy: BUSY, config: PanelConfig) -> Self {
        Self {
            spi,
            dc,
            reset,
            busy,
            config,
            state: ControllerState::Unknown,
        }
    }

    pub fn state(&self) -> ControllerState {
        self.state
    }

    pub fn release(self) -> (SPI, DC, RST, BUSY) {
        (self.spi, self.dc, self.reset, self.busy)
    }

    pub(crate) fn command(&mut self, command: u8) -> Result<(), Error> {
        self.dc.set_low().map_err(|_| Error::Gpio)?;
        self.spi.write(&[command]).map_err(|_| Error::Spi)
    }

    fn data(&mut self, data: &[u8]) -> Result<(), Error> {
        self.dc.set_high().map_err(|_| Error::Gpio)?;
        self.spi.write(data).map_err(|_| Error::Spi)
    }

    pub(crate) fn command_data(&mut self, command: u8, data: &[u8]) -> Result<(), Error> {
        self.command(command)?;
        self.data(data)
    }

    pub fn is_busy(&mut self) -> Result<bool, Error> {
        self.busy.is_high().map_err(|_| Error::Gpio)
    }

    fn reset_with_delay(&mut self, delay: &mut impl DelayNs) -> Result<(), Error> {
        self.reset.set_high().map_err(|_| Error::Gpio)?;
        delay.delay_ms(20);
        self.reset.set_low().map_err(|_| Error::Gpio)?;
        delay.delay_ms(20);
        self.reset.set_high().map_err(|_| Error::Gpio)?;
        delay.delay_ms(200);
        self.state = ControllerState::Unknown;
        Ok(())
    }

    async fn reset_async(&mut self, delay: &mut impl AsyncDelayNs) -> Result<(), Error> {
        self.reset.set_high().map_err(|_| Error::Gpio)?;
        delay.delay_ms(20).await;
        self.reset.set_low().map_err(|_| Error::Gpio)?;
        delay.delay_ms(20).await;
        self.reset.set_high().map_err(|_| Error::Gpio)?;
        delay.delay_ms(200).await;
        self.state = ControllerState::Unknown;
        Ok(())
    }

    fn configure(&mut self, grayscale: bool) -> Result<(), Error> {
        let config = self.config;
        self.command_data(Command::TEMP_SENSOR_CTRL, &[config.temperature_sensor])?;
        self.command_data(Command::BOOSTER_SOFT_START, &config.booster_soft_start)?;
        self.command_data(Command::DRIVER_OUTPUT_CTRL, &config.driver_output)?;
        self.command_data(Command::DATA_ENTRY_MODE, &[0x03])?;
        let border = if grayscale {
            config.grayscale_border_waveform
        } else {
            config.bw_border_waveform
        };
        self.command_data(Command::BORDER_WAVEFORM, &[border])?;
        self.set_window(0, 0, config.width, config.height)?;
        self.state = ControllerState::Powered;
        Ok(())
    }

    pub fn init_with_delay(&mut self, delay: &mut impl DelayNs) -> Result<(), Error> {
        self.reset_with_delay(delay)?;
        self.wait_ready_with_delay(delay)?;
        self.command(Command::SW_RESET)?;
        self.wait_ready_with_delay(delay)?;
        self.configure(false)
    }

    pub fn init_grayscale_with_delay(&mut self, delay: &mut impl DelayNs) -> Result<(), Error> {
        self.reset_with_delay(delay)?;
        self.wait_ready_with_delay(delay)?;
        self.command(Command::SW_RESET)?;
        self.wait_ready_with_delay(delay)?;
        self.configure(true)
    }

    pub async fn init_async(&mut self, delay: &mut impl AsyncDelayNs) -> Result<(), Error> {
        self.reset_async(delay).await?;
        self.wait_ready_async(delay).await?;
        self.command(Command::SW_RESET)?;
        self.wait_ready_async(delay).await?;
        self.configure(false)
    }

    pub async fn init_grayscale_async(
        &mut self,
        delay: &mut impl AsyncDelayNs,
    ) -> Result<(), Error> {
        self.reset_async(delay).await?;
        self.wait_ready_async(delay).await?;
        self.command(Command::SW_RESET)?;
        self.wait_ready_async(delay).await?;
        self.configure(true)
    }

    pub fn load_waveform(&mut self, waveform: &Waveform<'_>) -> Result<(), Error> {
        self.command_data(Command::WRITE_LUT, waveform.lut)?;
        self.command_data(Command::GATE_VOLTAGE, &[waveform.gate_voltage])?;
        self.command_data(Command::SOURCE_VOLTAGE, &waveform.source_voltage)?;
        self.command_data(Command::VCOM_VOLTAGE, &[waveform.vcom_voltage])
    }

    pub fn set_window(&mut self, x: u16, y: u16, width: u16, height: u16) -> Result<(), Error> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidParameter);
        }
        let x_end = x.checked_add(width - 1).ok_or(Error::InvalidParameter)?;
        let y_end = y.checked_add(height - 1).ok_or(Error::InvalidParameter)?;
        self.command_data(Command::SET_RAM_X_ADDR, &range_bytes(x, x_end))?;
        self.command_data(Command::SET_RAM_Y_ADDR, &range_bytes(y, y_end))?;
        self.command_data(Command::SET_RAM_X_COUNTER, &x.to_le_bytes())?;
        self.command_data(Command::SET_RAM_Y_COUNTER, &y.to_le_bytes())
    }

    pub fn write_row(&mut self, row: u16, data: &[u8]) -> Result<(), Error> {
        self.write_row_to(Command::WRITE_RAM_BW, row, data)
    }

    pub fn write_red_row(&mut self, row: u16, data: &[u8]) -> Result<(), Error> {
        self.write_row_to(Command::WRITE_RAM_RED, row, data)
    }

    fn write_row_to(&mut self, command: u8, row: u16, data: &[u8]) -> Result<(), Error> {
        self.command_data(Command::SET_RAM_Y_COUNTER, &row.to_le_bytes())?;
        self.command_data(Command::SET_RAM_X_COUNTER, &[0, 0])?;
        self.command_data(command, data)
    }

    pub fn write_frame_rows<const N: usize>(
        &mut self,
        rows: u16,
        fill: impl FnMut(u16, &mut [u8; N]),
    ) -> Result<(), Error> {
        self.write_frame_rows_to(Command::WRITE_RAM_BW, rows, fill)
    }

    pub fn write_red_frame_rows<const N: usize>(
        &mut self,
        rows: u16,
        fill: impl FnMut(u16, &mut [u8; N]),
    ) -> Result<(), Error> {
        self.write_frame_rows_to(Command::WRITE_RAM_RED, rows, fill)
    }

    fn write_frame_rows_to<const N: usize>(
        &mut self,
        command: u8,
        rows: u16,
        mut fill: impl FnMut(u16, &mut [u8; N]),
    ) -> Result<(), Error> {
        let mut row = [0xff; N];
        self.command(command)?;
        self.dc.set_high().map_err(|_| Error::Gpio)?;
        for y in 0..rows {
            fill(y, &mut row);
            self.spi.write(&row).map_err(|_| Error::Spi)?;
        }
        Ok(())
    }

    pub fn write_solid_window(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        value: u8,
    ) -> Result<(), Error> {
        self.write_solid_window_to(Command::WRITE_RAM_BW, x, y, width, height, value)
    }

    pub fn write_red_solid_window(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        value: u8,
    ) -> Result<(), Error> {
        self.write_solid_window_to(Command::WRITE_RAM_RED, x, y, width, height, value)
    }

    fn write_solid_window_to(
        &mut self,
        command: u8,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        value: u8,
    ) -> Result<(), Error> {
        self.set_window(x, y, width, height)?;
        let bytes = usize::from(width.div_ceil(8));
        if bytes > 100 {
            return Err(Error::InvalidParameter);
        }
        let row = [value; 100];
        self.command(command)?;
        self.dc.set_high().map_err(|_| Error::Gpio)?;
        for _ in 0..height {
            self.spi.write(&row[..bytes]).map_err(|_| Error::Spi)?;
        }
        Ok(())
    }

    pub fn clear_with_delay(&mut self, delay: &mut impl DelayNs) -> Result<(), Error> {
        self.set_window(0, 0, self.config.width, self.config.height)?;
        self.write_red_frame_rows::<100>(self.config.height, |_, row| row.fill(0xff))?;
        self.set_window(0, 0, self.config.width, self.config.height)?;
        self.write_frame_rows::<100>(self.config.height, |_, row| row.fill(0xff))?;
        self.refresh_with_delay(RefreshMode::Full, delay)
    }
}

fn range_bytes(start: u16, end: u16) -> [u8; 4] {
    let [start_low, start_high] = start.to_le_bytes();
    let [end_low, end_high] = end.to_le_bytes();
    [start_low, start_high, end_low, end_high]
}
