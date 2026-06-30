use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;
use ssd1677_driver::{PanelConfig, RefreshMode, Ssd1677, Waveform};
use xteink_hal::{AsyncDelay, AsyncDelayAdapter, Delay, DelayAdapter, HalResult};

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

pub const STAGED_GRAY_LUT_REVISION: u16 = 1;

const GRAY_LUT: [u8; 112] = [
    0x80, 0x48, 0x4a, 0x22, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0a, 0x48, 0x68, 0x08, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x88, 0x48, 0x60, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xa8, 0x48,
    0x45, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x07, 0x1e, 0x1c, 0x02, 0x00, 0x05, 0x01, 0x05, 0x01, 0x02, 0x08, 0x01, 0x01, 0x04,
    0x04, 0x00, 0x02, 0x01, 0x02, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x01, 0x22, 0x22, 0x22, 0x22, 0x22, 0x17, 0x41, 0xa8, 0x32, 0x30, 0x00, 0x00,
];

const STAGED_GRAY_LUT: [u8; 112] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x54, 0x54, 0x40, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xaa, 0xa0, 0xa8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xa2, 0x22,
    0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x01, 0x01, 0x01, 0x00, 0x01, 0x01, 0x01, 0x01, 0x00, 0x01, 0x01, 0x01, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x8f, 0x8f, 0x8f, 0x8f, 0x8f, 0x17, 0x41, 0xa8, 0x32, 0x30, 0x00, 0x00,
];

type Inner<SPI, DC, RST, BUSY> = Ssd1677<SPI, DC, RST, BUSY>;

pub struct DisplayDriver<SPI, DC, RST, BUSY>(Inner<SPI, DC, RST, BUSY>);

impl<SPI, DC, RST, BUSY> DisplayDriver<SPI, DC, RST, BUSY>
where
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    pub fn new(spi: SPI, dc: DC, reset: RST, busy: BUSY) -> Self {
        let config = PanelConfig {
            width: 800,
            height: 480,
            driver_output: [0xdf, 0x01, 0x02],
            booster_soft_start: [0xae, 0xc7, 0xc3, 0xc0, 0x80],
            temperature_sensor: 0x80,
            bw_border_waveform: 0x01,
            grayscale_border_waveform: 0x00,
            busy_timeout_ms: 60_000,
        };
        Self(Ssd1677::new(spi, dc, reset, busy, config))
    }

    pub fn is_busy(&mut self) -> HalResult<bool> {
        self.0.is_busy().map_err(Into::into)
    }

    pub fn trigger_refresh(&mut self, mode: RefreshMode) -> HalResult<()> {
        self.0.trigger_refresh(mode).map_err(Into::into)
    }

    pub fn init_with_delay(&mut self, delay: &dyn Delay) -> HalResult<()> {
        self.0
            .init_with_delay(&mut DelayAdapter(delay))
            .map_err(Into::into)
    }

    pub fn init_grayscale_with_delay(&mut self, delay: &dyn Delay) -> HalResult<()> {
        self.0.init_grayscale_with_delay(&mut DelayAdapter(delay))?;
        self.load_grayscale_lut()
    }

    pub async fn init_async<D: AsyncDelay + ?Sized>(&mut self, delay: &D) -> HalResult<()> {
        self.0
            .init_async(&mut AsyncDelayAdapter(delay))
            .await
            .map_err(Into::into)
    }

    pub async fn init_grayscale_async<D: AsyncDelay + ?Sized>(
        &mut self,
        delay: &D,
    ) -> HalResult<()> {
        self.0
            .init_grayscale_async(&mut AsyncDelayAdapter(delay))
            .await?;
        self.load_grayscale_lut()
    }

    fn load_grayscale_lut(&mut self) -> HalResult<()> {
        self.0
            .load_waveform(&waveform(&GRAY_LUT))
            .map_err(Into::into)
    }

    pub fn load_staged_grayscale_lut(&mut self) -> HalResult<()> {
        self.0
            .load_waveform(&waveform(&STAGED_GRAY_LUT))
            .map_err(Into::into)
    }

    pub async fn activate_staged_grayscale_async<D: AsyncDelay + ?Sized>(
        &mut self,
        delay: &D,
    ) -> HalResult<()> {
        self.0
            .activate_staged_grayscale_async(&mut AsyncDelayAdapter(delay))
            .await
            .map_err(Into::into)
    }

    pub fn set_window(&mut self, x: u16, y: u16, width: u16, height: u16) -> HalResult<()> {
        self.0.set_window(x, y, width, height).map_err(Into::into)
    }

    pub fn write_row(&mut self, row: u16, data: &[u8]) -> HalResult<()> {
        self.0.write_row(row, data).map_err(Into::into)
    }

    pub fn write_red_row(&mut self, row: u16, data: &[u8]) -> HalResult<()> {
        self.0.write_red_row(row, data).map_err(Into::into)
    }

    pub fn write_frame_rows<const N: usize>(
        &mut self,
        rows: u16,
        fill: impl FnMut(u16, &mut [u8; N]),
    ) -> HalResult<()> {
        self.0.write_frame_rows(rows, fill).map_err(Into::into)
    }

    pub fn write_red_frame_rows<const N: usize>(
        &mut self,
        rows: u16,
        fill: impl FnMut(u16, &mut [u8; N]),
    ) -> HalResult<()> {
        self.0.write_red_frame_rows(rows, fill).map_err(Into::into)
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
            .write_red_solid_window(x, y, width, height, value)
            .map_err(Into::into)
    }

    pub fn refresh_with_delay(&mut self, mode: RefreshMode, delay: &dyn Delay) -> HalResult<()> {
        self.0
            .refresh_with_delay(mode, &mut DelayAdapter(delay))
            .map_err(Into::into)
    }

    pub async fn refresh_async<D: AsyncDelay + ?Sized>(
        &mut self,
        mode: RefreshMode,
        delay: &D,
    ) -> HalResult<()> {
        self.0
            .refresh_async(mode, &mut AsyncDelayAdapter(delay))
            .await
            .map_err(Into::into)
    }

    pub fn clear_with_delay(&mut self, delay: &dyn Delay) -> HalResult<()> {
        self.0
            .clear_with_delay(&mut DelayAdapter(delay))
            .map_err(Into::into)
    }
}

fn waveform(bytes: &'static [u8; 112]) -> Waveform<'static> {
    Waveform {
        lut: &bytes[..105],
        gate_voltage: bytes[105],
        source_voltage: [bytes[106], bytes[107], bytes[108]],
        vcom_voltage: bytes[109],
    }
}
