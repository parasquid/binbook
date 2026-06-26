#![no_std]
#![no_main]

use esp_hal::{
    analog::adc::{Adc, AdcCalBasic, AdcConfig, Attenuation},
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
#[cfg(feature = "debug-log")]
use esp_println::println;

#[cfg(feature = "debug-log")]
macro_rules! dbgprintln {
    ($($arg:tt)*) => { println!($($arg)*) };
}
#[cfg(not(feature = "debug-log"))]
macro_rules! dbgprintln {
    ($($arg:tt)*) => {};
}

esp_bootloader_esp_idf::esp_app_desc!();

const SPI_FREQUENCY: Rate = Rate::from_mhz(4);
const PROBE_BOOK: &[u8] = include_bytes!("../fixtures/nav_probe.binbook");
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

    let mut adc_config = AdcConfig::new();
    let mut ch1_pin =
        adc_config.enable_pin_with_cal::<_, AdcCalBasic<_>>(peripherals.GPIO1, Attenuation::_11dB);
    let mut ch2_pin =
        adc_config.enable_pin_with_cal::<_, AdcCalBasic<_>>(peripherals.GPIO2, Attenuation::_11dB);
    let mut adc = Adc::new(peripherals.ADC1, adc_config);

    let mut scratch = [0u8; BINBOOK_SCRATCH_BYTES];
    let mut book =
        binbook::BinBook::open(PROBE_BOOK, &mut scratch).expect("failed to open embedded BinBook");
    let page_count = book.page_count();

    let mut current_page: u32 = 0;
    let mut refresh_state = binbook_fw::refresh::RefreshState::new();
    let mut panel_mode = binbook_fw::display::PanelMode::Unknown;
    render_current_page(&mut display, &mut book, &delay, &mut refresh_state, &mut panel_mode, current_page);

    let mut input_state = binbook_fw::input::InputState::new();
    let mut tick: u64 = 0;

    dbgprintln!("[NAV] Firmware started. page_count={}", page_count);

    loop {
        delay.ms(50);
        tick = tick.saturating_add(50);

        let ch1 = loop {
            match adc.read_oneshot(&mut ch1_pin) {
                Ok(v) => break v,
                Err(nb::Error::WouldBlock) => {}
                Err(nb::Error::Other(())) => break 0,
            }
        };
        let ch2 = loop {
            match adc.read_oneshot(&mut ch2_pin) {
                Ok(v) => break v,
                Err(nb::Error::WouldBlock) => {}
                Err(nb::Error::Other(())) => break 0,
            }
        };

        if (tick % 500) == 0 {
            let decoded = binbook_fw::input::decode_buttons(ch1, ch2);
            dbgprintln!("[ADC] ch1={} ch2={} decoded={:?} tick={}", ch1, ch2, decoded, tick);
        }

        if let Some(event) = input_state.poll_raw(ch1, ch2, tick) {
            dbgprintln!("[NAV] event={:?}", event);
            if let binbook_fw::input::ButtonEvent::Press(button) = event {
                match button {
                    binbook_fw::input::Button::Select | binbook_fw::input::Button::Back => {
                        dbgprintln!("[NAV] {:?} pressed (no action yet)", button);
                    }
                    _ => {}
                }
                if let Some(turn) = binbook_fw::input::page_turn_for_button(button) {
                    let new_page =
                        binbook_fw::input::apply_page_turn(current_page, page_count, turn);
                    dbgprintln!("[NAV] turn={:?} current_page={} new_page={}", turn, current_page, new_page);
                    if new_page != current_page {
                        current_page = new_page;
                        dbgprintln!("[NAV] rendering page {}", current_page);
                        render_current_page(&mut display, &mut book, &delay, &mut refresh_state, &mut panel_mode, current_page);
                    }
                }
            }
        }
    }
}

fn render_current_page<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut binbook::BinBook<&[u8], &mut [u8; BINBOOK_SCRATCH_BYTES]>,
    delay: &dyn xteink_hal::Delay,
    refresh_state: &mut binbook_fw::refresh::RefreshState,
    panel_mode: &mut binbook_fw::display::PanelMode,
    page_index: u32,
) where
    SPI: xteink_hal::Spi,
    CS: xteink_hal::OutputPin,
    DC: xteink_hal::OutputPin,
    RST: xteink_hal::OutputPin,
    BUSY: xteink_hal::InputPin,
{
    #[cfg(not(feature = "chunk-dirty-probe"))]
    {
        let transition_mask =
            binbook_fw::display::find_transition_mask(book, refresh_state.previous_page(), page_index);
        let decision = refresh_state.decide(page_index, transition_mask);
        dbgprintln!(
            "[REFRESH] policy=FullScreenDifferentialDefault page={} decision={} panel_mode={:?}",
            page_index,
            decision.name(),
            panel_mode
        );
        dbgprintln!("[PANEL] mode={:?}", panel_mode);
        if let Err(e) = binbook_fw::display::display_page_with_policy(
            display,
            book,
            &PROBE_BOOK,
            delay,
            refresh_state,
            panel_mode,
            page_index,
        ) {
            dbgprintln!("[NAV] display error on page {}: {:?}", page_index, e);
        }
    }
    #[cfg(feature = "chunk-dirty-probe")]
    {
        let transition_mask =
            binbook_fw::display::find_transition_mask(book, refresh_state.previous_page(), page_index);
        let decision = refresh_state.decide_with_policy(
            page_index,
            transition_mask,
            binbook_fw::refresh::RefreshPolicy::ChunkDirtyDifferentialDefault,
        );
        dbgprintln!(
            "[PROBE] chunk_dirty_window page={} decision={} panel_mode={:?}",
            page_index,
            decision.name(),
            panel_mode
        );
        dbgprintln!("[PANEL] mode={:?}", panel_mode);
        if let Err(e) = binbook_fw::display::display_page_with_chunk_dirty_probe_policy(
            display,
            book,
            &PROBE_BOOK,
            delay,
            refresh_state,
            panel_mode,
            page_index,
        ) {
            dbgprintln!("[NAV] display error on page {}: {:?}", page_index, e);
        }
    }
}
