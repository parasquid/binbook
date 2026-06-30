use embedded_hal::{
    digital::{InputPin, OutputPin},
    spi::SpiDevice,
};
use ssd1677_driver::RefreshMode;
use xteink_hal::{AsyncDelay, AsyncDelayAdapter, Delay, DelayAdapter, HalError, HalResult};
use xteink_x4_display::panel::X4Panel;

pub type LegacyDisplayDriver<SPI, CS, DC, RST, BUSY> = DisplayDriver<
    xteink_hal::SpiDeviceAdapter<SPI, CS>,
    xteink_hal::OutputPinAdapter<DC>,
    xteink_hal::OutputPinAdapter<RST>,
    xteink_hal::InputPinAdapter<BUSY>,
>;

pub fn new_legacy_display<SPI, CS, DC, RST, BUSY>(
    spi: SPI,
    cs: CS,
    dc: DC,
    reset: RST,
    busy: BUSY,
) -> LegacyDisplayDriver<SPI, CS, DC, RST, BUSY>
where
    SPI: xteink_hal::Spi,
    CS: xteink_hal::OutputPin,
    DC: xteink_hal::OutputPin,
    RST: xteink_hal::OutputPin,
    BUSY: xteink_hal::InputPin,
{
    DisplayDriver::new(
        xteink_hal::SpiDeviceAdapter::new(spi, cs),
        xteink_hal::OutputPinAdapter(dc),
        xteink_hal::OutputPinAdapter(reset),
        xteink_hal::InputPinAdapter(busy),
    )
}

pub use xteink_x4_display::panel::STAGED_GRAY_LUT_REVISION;

pub struct DisplayDriver<SPI, DC, RST, BUSY>(X4Panel<SPI, DC, RST, BUSY>);

impl<SPI, DC, RST, BUSY> DisplayDriver<SPI, DC, RST, BUSY>
where
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    pub fn new(spi: SPI, dc: DC, reset: RST, busy: BUSY) -> Self {
        Self(X4Panel::new(spi, dc, reset, busy))
    }

    pub fn inner_mut(&mut self) -> &mut X4Panel<SPI, DC, RST, BUSY> {
        &mut self.0
    }

    pub fn is_busy(&mut self) -> HalResult<bool> {
        self.0.controller().is_busy().map_err(Into::into)
    }
    pub fn trigger_refresh(&mut self, mode: RefreshMode) -> HalResult<()> {
        self.0
            .controller()
            .trigger_refresh(mode)
            .map_err(Into::into)
    }
    pub fn init_with_delay(&mut self, delay: &dyn Delay) -> HalResult<()> {
        self.0
            .init_bw(&mut DelayAdapter(delay))
            .map_err(map_display_error)
    }
    pub fn init_grayscale_with_delay(&mut self, delay: &dyn Delay) -> HalResult<()> {
        self.0
            .init_absolute_gray(&mut DelayAdapter(delay))
            .map_err(map_display_error)
    }
    pub async fn init_async<D: AsyncDelay + ?Sized>(&mut self, delay: &D) -> HalResult<()> {
        self.0
            .init_bw_async(&mut AsyncDelayAdapter(delay))
            .await
            .map_err(map_display_error)
    }
    pub async fn init_grayscale_async<D: AsyncDelay + ?Sized>(
        &mut self,
        delay: &D,
    ) -> HalResult<()> {
        self.0
            .init_absolute_gray_async(&mut AsyncDelayAdapter(delay))
            .await
            .map_err(map_display_error)
    }
    pub fn load_staged_grayscale_lut(&mut self) -> HalResult<()> {
        self.0.load_staged_gray().map_err(map_display_error)
    }
    pub async fn activate_staged_grayscale_async<D: AsyncDelay + ?Sized>(
        &mut self,
        delay: &D,
    ) -> HalResult<()> {
        self.0
            .activate_staged_gray(&mut AsyncDelayAdapter(delay))
            .await
            .map_err(map_display_error)
    }
    pub fn set_window(&mut self, x: u16, y: u16, width: u16, height: u16) -> HalResult<()> {
        self.0
            .controller()
            .set_window(x, y, width, height)
            .map_err(Into::into)
    }
    pub fn write_row(&mut self, row: u16, data: &[u8]) -> HalResult<()> {
        self.0.controller().write_row(row, data).map_err(Into::into)
    }
    pub fn write_red_row(&mut self, row: u16, data: &[u8]) -> HalResult<()> {
        self.0
            .controller()
            .write_red_row(row, data)
            .map_err(Into::into)
    }
    pub fn write_frame_rows<const N: usize>(
        &mut self,
        rows: u16,
        fill: impl FnMut(u16, &mut [u8; N]),
    ) -> HalResult<()> {
        self.0
            .controller()
            .write_frame_rows(rows, fill)
            .map_err(Into::into)
    }
    pub fn write_red_frame_rows<const N: usize>(
        &mut self,
        rows: u16,
        fill: impl FnMut(u16, &mut [u8; N]),
    ) -> HalResult<()> {
        self.0
            .controller()
            .write_red_frame_rows(rows, fill)
            .map_err(Into::into)
    }
    pub fn write_solid_window(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        value: u8,
    ) -> HalResult<()> {
        self.0
            .controller()
            .write_solid_window(x, y, width, height, value)
            .map_err(Into::into)
    }
    pub fn write_red_solid_window(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        value: u8,
    ) -> HalResult<()> {
        self.0
            .controller()
            .write_red_solid_window(x, y, width, height, value)
            .map_err(Into::into)
    }
    pub fn refresh_with_delay(&mut self, mode: RefreshMode, delay: &dyn Delay) -> HalResult<()> {
        self.0
            .controller()
            .refresh_with_delay(mode, &mut DelayAdapter(delay))
            .map_err(Into::into)
    }
    pub async fn refresh_async<D: AsyncDelay + ?Sized>(
        &mut self,
        mode: RefreshMode,
        delay: &D,
    ) -> HalResult<()> {
        self.0
            .controller()
            .refresh_async(mode, &mut AsyncDelayAdapter(delay))
            .await
            .map_err(Into::into)
    }
    pub fn clear_with_delay(&mut self, delay: &dyn Delay) -> HalResult<()> {
        self.0
            .controller()
            .clear_with_delay(&mut DelayAdapter(delay))
            .map_err(Into::into)
    }
}

fn map_display_error(error: xteink_x4_display::DisplayError) -> HalError {
    match error {
        xteink_x4_display::DisplayError::Controller => HalError::Spi,
        xteink_x4_display::DisplayError::Source => HalError::Flash,
        _ => HalError::InvalidParam,
    }
}
