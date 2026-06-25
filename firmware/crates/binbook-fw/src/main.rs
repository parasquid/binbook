//! Xteink X4 BinBook GRAY2 render-probe firmware binary.
//!
//! This binary initializes the SSD1677 display using the verified Xteink X4
//! pins, opens an embedded BinBook fixture, and renders page 0 through the
//! GRAY2 display path. It is a rendering milestone, not the final reader
//! application.

#![no_std]
#![no_main]

use esp_hal::{
    delay::Delay as EspDelay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig},
    spi::{
        master::{Config as SpiConfig, Spi as EspSpi},
        Mode,
    },
    time::Rate,
    Blocking,
};
use ssd1677_driver::Ssd1677Driver;
use xteink_hal::{Delay as _, HalError, HalResult};

use esp_backtrace as _;

esp_bootloader_esp_idf::esp_app_desc!();

const SPI_FREQUENCY: Rate = Rate::from_mhz(4);
const PROBE_BOOK: &[u8] = include_bytes!("../fixtures/gray2_probe.binbook");
const BINBOOK_SCRATCH_BYTES: usize = 8192;

struct Delay(EspDelay);

impl xteink_hal::Delay for Delay {
    fn ms(&self, ms: u32) {
        self.0.delay_millis(ms);
    }
}

struct Spi(EspSpi<'static, Blocking>);

impl xteink_hal::Spi for Spi {
    fn write_command(&mut self, cmd: u8, data: &[u8]) -> HalResult<()> {
        self.write(&[cmd])?;
        self.write(data)
    }

    fn write(&mut self, data: &[u8]) -> HalResult<()> {
        self.0.write(data).map_err(|_| HalError::Spi)
    }

    fn read(&mut self, buf: &mut [u8]) -> HalResult<()> {
        self.0.read(buf).map_err(|_| HalError::Spi)
    }
}

struct OutputPin(Output<'static>);

impl xteink_hal::OutputPin for OutputPin {
    fn set_high(&mut self) -> HalResult<()> {
        self.0.set_high();
        Ok(())
    }

    fn set_low(&mut self) -> HalResult<()> {
        self.0.set_low();
        Ok(())
    }
}

struct InputPin(Input<'static>);

impl xteink_hal::InputPin for InputPin {
    fn is_high(&self) -> HalResult<bool> {
        Ok(self.0.is_high())
    }
}

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let delay = Delay(EspDelay::new());

    let spi = EspSpi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(SPI_FREQUENCY)
            .with_mode(Mode::_0),
    )
    .expect("failed to configure SPI2")
    .with_sck(peripherals.GPIO8)
    .with_mosi(peripherals.GPIO10);

    let cs = OutputPin(Output::new(
        peripherals.GPIO21,
        Level::High,
        OutputConfig::default(),
    ));
    let dc = OutputPin(Output::new(
        peripherals.GPIO4,
        Level::Low,
        OutputConfig::default(),
    ));
    let rst = OutputPin(Output::new(
        peripherals.GPIO5,
        Level::High,
        OutputConfig::default(),
    ));
    let busy = InputPin(Input::new(peripherals.GPIO6, InputConfig::default()));

    let mut display = Ssd1677Driver::new(Spi(spi), cs, dc, rst, busy);
    display
        .init_grayscale_with_delay(&delay)
        .expect("failed to initialize SSD1677 display");
    render_embedded_probe(&mut display, &delay).expect("failed to render BinBook GRAY2 probe");

    loop {
        delay.ms(1000);
    }
}

fn render_embedded_probe<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    delay: &dyn xteink_hal::Delay,
) -> HalResult<()>
where
    SPI: xteink_hal::Spi,
    CS: xteink_hal::OutputPin,
    DC: xteink_hal::OutputPin,
    RST: xteink_hal::OutputPin,
    BUSY: xteink_hal::InputPin,
{
    let scratch = [0u8; BINBOOK_SCRATCH_BYTES];
    let mut book =
        binbook::BinBook::open(PROBE_BOOK, scratch).map_err(|_| HalError::InvalidParam)?;
    let page = book.page(0).map_err(|_| HalError::InvalidParam)?;

    if page.info.pixel_format != binbook::page_index::PIXEL_FORMAT_GRAY2_PACKED
        || page.info.compression_method != binbook::page_index::COMPRESSION_RLE_PACKBITS
        || page.info.stored_width != binbook_fw::display::DISPLAY_WIDTH
        || page.info.stored_height != binbook_fw::display::DISPLAY_HEIGHT
        || page.info.plane_dir.bitmap != 0x01
    {
        return Err(HalError::InvalidParam);
    }

    binbook_fw::display::display_gray2_page(display, page.compressed_data(), delay)
}
