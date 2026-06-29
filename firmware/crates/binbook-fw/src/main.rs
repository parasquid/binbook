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

#[cfg(feature = "diagnostic-console")]
use core::cell::RefCell;
#[cfg(feature = "diagnostic-console")]
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};
#[cfg(feature = "firmware-bin")]
use embassy_executor::Spawner;

use esp_backtrace as _;
#[cfg(all(feature = "debug-log", not(feature = "diagnostic-console")))]
use esp_println::println;

#[cfg(all(feature = "debug-log", not(feature = "diagnostic-console")))]
macro_rules! dbgprintln {
    ($($arg:tt)*) => { println!($($arg)*) };
}
#[cfg(not(all(feature = "debug-log", not(feature = "diagnostic-console"))))]
macro_rules! dbgprintln {
    ($($arg:tt)*) => {};
}

esp_bootloader_esp_idf::esp_app_desc!();

#[cfg(feature = "firmware-bin")]
mod runtime;

const DISPLAY_SPI_FREQUENCY_MHZ: u32 = 20;
const PROBE_BOOK: &[u8] = include_bytes!("../fixtures/nav_probe.binbook");
const BINBOOK_SCRATCH_BYTES: usize = 8192;

struct Delay(EspDelay);

impl xteink_hal::Delay for Delay {
    fn ms(&self, ms: u32) {
        self.0.delay_millis(ms);
    }
}

#[cfg(feature = "firmware-bin")]
impl xteink_hal::AsyncDelay for Delay {
    async fn ms(&self, ms: u32) {
        embassy_time::Timer::after_millis(ms as u64).await;
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

#[cfg(feature = "diagnostic-console")]
struct X4Flash(RefCell<esp_storage::FlashStorage<'static>>);

#[cfg(feature = "diagnostic-console")]
impl xteink_hal::Flash for X4Flash {
    fn read(&self, offset: u32, buf: &mut [u8]) -> HalResult<()> {
        ReadNorFlash::read(&mut *self.0.borrow_mut(), offset, buf).map_err(|_| HalError::Flash)
    }

    fn write(&mut self, offset: u32, data: &[u8]) -> HalResult<()> {
        NorFlash::write(&mut *self.0.borrow_mut(), offset, data).map_err(|_| HalError::Flash)
    }

    fn erase_sector(&mut self, offset: u32) -> HalResult<()> {
        NorFlash::erase(
            &mut *self.0.borrow_mut(),
            offset,
            offset + binbook_fw::flash::CRASH_SECTOR_SIZE,
        )
        .map_err(|_| HalError::Flash)
    }

    fn size(&self) -> u32 {
        self.0.borrow().capacity() as u32
    }
}

#[cfg(feature = "diagnostic-console")]
fn panel_mode_code(mode: binbook_fw::display::PanelMode) -> u8 {
    use binbook_diagnostic_protocol::PanelModeCode;
    match mode {
        binbook_fw::display::PanelMode::Unknown => PanelModeCode::Unknown as u8,
        binbook_fw::display::PanelMode::Grayscale => PanelModeCode::Grayscale as u8,
        binbook_fw::display::PanelMode::Bw => PanelModeCode::Bw as u8,
    }
}

#[cfg(feature = "diagnostic-console")]
fn hal_error_code(error: HalError) -> i32 {
    match error {
        HalError::Spi => -1,
        HalError::Gpio => -2,
        HalError::Flash => -3,
        HalError::Timeout => -4,
        HalError::InvalidParam => -5,
    }
}

#[allow(dead_code)]
async fn input_task() {}

#[allow(dead_code)]
async fn display_task() {}

#[allow(dead_code)]
async fn diagnostic_task() {}

#[cfg(feature = "firmware-bin")]
#[embassy_executor::task]
async fn firmware_task(peripherals: runtime::RuntimePeripherals, spawner: Spawner) {
    runtime::run(spawner, peripherals).await;
}

#[cfg(feature = "firmware-bin")]
#[esp_hal::main]
fn main() -> ! {
    let esp_hal::peripherals::Peripherals {
        ADC1,
        GPIO1,
        GPIO2,
        SPI2,
        GPIO8,
        GPIO10,
        GPIO21,
        GPIO4,
        GPIO5,
        GPIO6,
        #[cfg(feature = "diagnostic-console")]
        USB_DEVICE,
        #[cfg(feature = "diagnostic-console")]
        FLASH,
        TIMG0,
        SW_INTERRUPT,
        ..
    } = esp_hal::init(esp_hal::Config::default());

    let timer = esp_hal::timer::timg::TimerGroup::new(TIMG0);
    let software_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(SW_INTERRUPT);
    esp_rtos::start(timer.timer0, software_interrupt.software_interrupt0);

    let peripherals = runtime::RuntimePeripherals {
        adc1: ADC1,
        gpio1: GPIO1,
        gpio2: GPIO2,
        spi2: SPI2,
        gpio8: GPIO8,
        gpio10: GPIO10,
        gpio21: GPIO21,
        gpio4: GPIO4,
        gpio5: GPIO5,
        gpio6: GPIO6,
        #[cfg(feature = "diagnostic-console")]
        usb_device: USB_DEVICE,
        #[cfg(feature = "diagnostic-console")]
        flash: FLASH,
    };

    let mut executor = esp_rtos::embassy::Executor::new();
    let executor = unsafe { __make_static(&mut executor) };
    executor.run(move |spawner| {
        spawner
            .spawn(firmware_task(peripherals, spawner).expect("failed to create firmware task"));
    })
}

#[cfg(feature = "firmware-bin")]
unsafe fn __make_static<T>(value: &mut T) -> &'static mut T {
    core::mem::transmute(value)
}

#[cfg(not(feature = "firmware-bin"))]
#[esp_hal::main]
fn main() -> ! {
    let peripherals = unsafe { esp_hal::peripherals::Peripherals::steal() };
    let delay = Delay(EspDelay::new());

    let spi = EspSpi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(DISPLAY_SPI_FREQUENCY_MHZ))
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
    let _ = render_current_page(
        &mut display,
        &mut book,
        &delay,
        &mut refresh_state,
        &mut panel_mode,
        current_page,
    );

    let mut input_state = binbook_fw::input::InputState::new();
    let mut tick: u64 = 0;

    #[cfg(feature = "diagnostic-console")]
    let (mut usb_rx, mut usb_tx) = {
        use esp_hal::usb_serial_jtag::UsbSerialJtag;
        UsbSerialJtag::new(peripherals.USB_DEVICE).split()
    };
    #[cfg(feature = "diagnostic-console")]
    let mut diag_serial_state = binbook_fw::diag::SerialState::new();
    #[cfg(feature = "diagnostic-console")]
    let mut diag_log =
        binbook_fw::diag_log::DiagLog::<{ binbook_fw::diag_log::DEFAULT_LOG_CAPACITY }>::new();
    #[cfg(feature = "diagnostic-console")]
    let mut diag_deduper = binbook_fw::diag_log::DiagDeduper::new();
    #[cfg(feature = "diagnostic-console")]
    let mut crash_store = binbook_fw::diag_flash::CrashStore::new(X4Flash(RefCell::new(
        esp_storage::FlashStorage::new(peripherals.FLASH),
    )));
    #[cfg(feature = "diagnostic-console")]
    let mut diag_last_error = 0i32;
    #[cfg(feature = "diagnostic-console")]
    {
        dbgprintln!("[DIAG] diagnostic console enabled");
        use binbook_fw::diag_log::{DiagEvent, EVT_FIRMWARE_STARTED, LEVEL_INFO, SUB_SYSTEM};
        diag_log.push_event(
            DiagEvent {
                level: LEVEL_INFO,
                subsystem: SUB_SYSTEM,
                event: EVT_FIRMWARE_STARTED,
                arg0: page_count as i32,
                arg1: 0,
                arg2: 0,
            },
            tick as u32,
        );
    }

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
            dbgprintln!(
                "[ADC] ch1={} ch2={} decoded={:?} tick={}",
                ch1,
                ch2,
                decoded,
                tick
            );
            #[cfg(feature = "diagnostic-console")]
            {
                use binbook_fw::diag_log::{DiagEvent, EVT_ADC_SAMPLE, LEVEL_TRACE, SUB_INPUT};
                diag_log.push_event(
                    DiagEvent {
                        level: LEVEL_TRACE,
                        subsystem: SUB_INPUT,
                        event: EVT_ADC_SAMPLE,
                        arg0: ch1 as i32,
                        arg1: 0,
                        arg2: 0,
                    },
                    tick as u32,
                );
            }
        }

        if let Some(event) = input_state.poll_raw(ch1, ch2, tick) {
            dbgprintln!("[NAV] event={:?}", event);
            if let binbook_fw::input::ButtonEvent::Press(button) = event {
                #[cfg(feature = "diagnostic-console")]
                {
                    use binbook_fw::diag_log::{
                        DiagEvent, EVT_BUTTON_EVENT, LEVEL_INFO, SUB_INPUT,
                    };
                    diag_log.push_event(
                        DiagEvent {
                            level: LEVEL_INFO,
                            subsystem: SUB_INPUT,
                            event: EVT_BUTTON_EVENT,
                            arg0: button as i32,
                            arg1: 0,
                            arg2: 0,
                        },
                        tick as u32,
                    );
                }
                match button {
                    binbook_fw::input::Button::Select | binbook_fw::input::Button::Back => {
                        dbgprintln!("[NAV] {:?} pressed (no action yet)", button);
                    }
                    _ => {}
                }
                if let Some(turn) = binbook_fw::input::page_turn_for_button(button) {
                    let new_page =
                        binbook_fw::input::apply_page_turn(current_page, page_count, turn);
                    dbgprintln!(
                        "[NAV] turn={:?} current_page={} new_page={}",
                        turn,
                        current_page,
                        new_page
                    );
                    #[cfg(feature = "diagnostic-console")]
                    {
                        use binbook_fw::diag_log::{DiagEvent, EVT_PAGE_TURN, LEVEL_INFO, SUB_NAV};
                        diag_log.push_event(
                            DiagEvent {
                                level: LEVEL_INFO,
                                subsystem: SUB_NAV,
                                event: EVT_PAGE_TURN,
                                arg0: new_page as i32,
                                arg1: 0,
                                arg2: 0,
                            },
                            tick as u32,
                        );
                    }
                    if new_page != current_page {
                        dbgprintln!("[NAV] rendering page {}", new_page);
                        #[cfg(feature = "diagnostic-console")]
                        {
                            use binbook_fw::diag_log::{
                                DiagEvent, EVT_RENDER_START, LEVEL_INFO, SUB_DISPLAY,
                            };
                            diag_log.push_event(
                                DiagEvent {
                                    level: LEVEL_INFO,
                                    subsystem: SUB_DISPLAY,
                                    event: EVT_RENDER_START,
                                    arg0: new_page as i32,
                                    arg1: 0,
                                    arg2: 0,
                                },
                                tick as u32,
                            );
                        }
                        match render_current_page(
                            &mut display,
                            &mut book,
                            &delay,
                            &mut refresh_state,
                            &mut panel_mode,
                            new_page,
                        ) {
                            Ok(report) => {
                                current_page = new_page;
                                #[cfg(feature = "diagnostic-console")]
                                record_render_success(
                                    &mut diag_log,
                                    tick as u32,
                                    current_page,
                                    report,
                                );
                            }
                            Err(error) => {
                                #[cfg(feature = "diagnostic-console")]
                                {
                                    diag_last_error = hal_error_code(error);
                                    record_display_failure(
                                        &mut diag_log,
                                        &mut crash_store,
                                        tick as u32,
                                        current_page,
                                        panel_mode,
                                        diag_last_error,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        #[cfg(feature = "diagnostic-console")]
        {
            let mut usb_read_buf = [0u8; 64];
            let n = usb_rx.drain_rx_fifo(&mut usb_read_buf);
            if n > 0 {
                diag_serial_state.feed_rx(&usb_read_buf[..n]);
            }

            let pending = binbook_fw::diag::poll_pending_command(
                &mut diag_serial_state,
                current_page,
                page_count,
                diag_last_error,
                panel_mode_code(panel_mode),
                &mut diag_log,
                tick as u32,
            );

            if let Some(pending) = pending {
                use binbook_diagnostic_protocol::{
                    encode_crash_response, Status, CRASH_SUMMARY_BYTES,
                };
                use binbook_fw::diag::PendingAction;
                let mut action_payload = [0u8; 1 + CRASH_SUMMARY_BYTES];
                let mut action_payload_len = 0usize;
                let status = match pending.action {
                    PendingAction::RenderTurn { turn } => {
                        let target_page = binbook_fw::input::apply_page_turn(
                            current_page,
                            page_count,
                            turn,
                        );
                        diag_log.push(
                            tick as u32,
                            binbook_fw::diag_log::DiagEvent {
                                level: binbook_fw::diag_log::LEVEL_INFO,
                                subsystem: binbook_fw::diag_log::SUB_DISPLAY,
                                event: binbook_fw::diag_log::EVT_RENDER_START,
                                arg0: target_page as i32,
                                arg1: match turn {
                                    binbook_fw::input::PageTurn::Previous => 0,
                                    binbook_fw::input::PageTurn::Next => 1,
                                    binbook_fw::input::PageTurn::First => 2,
                                    binbook_fw::input::PageTurn::Last => 3,
                                },
                                arg2: 0,
                            },
                        );
                        match render_current_page(
                            &mut display,
                            &mut book,
                            &delay,
                            &mut refresh_state,
                            &mut panel_mode,
                            target_page,
                        ) {
                            Ok(report) => {
                                current_page = target_page;
                                diag_last_error = 0;
                                record_render_success(
                                    &mut diag_log,
                                    tick as u32,
                                    current_page,
                                    report,
                                );
                                Status::Ok
                            }
                            Err(error) => {
                                diag_last_error = hal_error_code(error);
                                record_display_failure(
                                    &mut diag_log,
                                    &mut crash_store,
                                    tick as u32,
                                    current_page,
                                    panel_mode,
                                    diag_last_error,
                                );
                                Status::InternalError
                            }
                        }
                    }
                    PendingAction::RenderPage { target_page } => {
                        diag_log.push(
                            tick as u32,
                            binbook_fw::diag_log::DiagEvent {
                                level: binbook_fw::diag_log::LEVEL_INFO,
                                subsystem: binbook_fw::diag_log::SUB_DISPLAY,
                                event: binbook_fw::diag_log::EVT_RENDER_START,
                                arg0: target_page as i32,
                                arg1: 0,
                                arg2: 0,
                            },
                        );
                        match render_current_page(
                            &mut display,
                            &mut book,
                            &delay,
                            &mut refresh_state,
                            &mut panel_mode,
                            target_page,
                        ) {
                            Ok(report) => {
                                current_page = target_page;
                                diag_last_error = 0;
                                record_render_success(
                                    &mut diag_log,
                                    tick as u32,
                                    current_page,
                                    report,
                                );
                                Status::Ok
                            }
                            Err(error) => {
                                diag_last_error = hal_error_code(error);
                                record_display_failure(
                                    &mut diag_log,
                                    &mut crash_store,
                                    tick as u32,
                                    current_page,
                                    panel_mode,
                                    diag_last_error,
                                );
                                Status::InternalError
                            }
                        }
                    }
                    PendingAction::DisplayProbe(probe) => {
                        use binbook_fw::diag::DisplayProbeKind;
                        let probe_result = match probe {
                            DisplayProbeKind::FullRefreshCurrent => {
                                binbook_fw::display::display_full_refresh_current(
                                    &mut display,
                                    &mut book,
                                    &PROBE_BOOK,
                                    &delay,
                                    &mut panel_mode,
                                    current_page,
                                )
                            }
                            DisplayProbeKind::ClearWhite => {
                                binbook_fw::display::display_clear_white_probe(
                                    &mut display,
                                    &delay,
                                    &mut panel_mode,
                                )
                            }
                            DisplayProbeKind::WindowCorners => {
                                binbook_fw::display::display_window_corners_probe(
                                    &mut display,
                                    &delay,
                                    &mut panel_mode,
                                )
                            }
                        };
                        match probe_result {
                            Ok(()) => {
                                refresh_state.invalidate();
                                diag_last_error = 0;
                                diag_log.push(
                                    tick as u32,
                                    binbook_fw::diag_log::DiagEvent {
                                        level: binbook_fw::diag_log::LEVEL_INFO,
                                        subsystem: binbook_fw::diag_log::SUB_DISPLAY,
                                        event: binbook_fw::diag_log::EVT_RENDER_SUCCESS,
                                        arg0: probe as i32,
                                        arg1: panel_mode_code(panel_mode) as i32,
                                        arg2: 0,
                                    },
                                );
                                Status::Ok
                            }
                            Err(error) => {
                                diag_last_error = hal_error_code(error);
                                record_display_failure(
                                    &mut diag_log,
                                    &mut crash_store,
                                    tick as u32,
                                    current_page,
                                    panel_mode,
                                    diag_last_error,
                                );
                                Status::InternalError
                            }
                        }
                    }
                    PendingAction::CrashGet => match crash_store.read() {
                        Ok(summary) => {
                            let mut encoded_summary = [0u8; CRASH_SUMMARY_BYTES];
                            action_payload_len = match summary {
                                Some(summary) => {
                                    summary.encode(&mut encoded_summary);
                                    encode_crash_response(
                                        Some(&encoded_summary),
                                        &mut action_payload,
                                    )
                                    .unwrap_or(0)
                                }
                                None => {
                                    encode_crash_response(None, &mut action_payload).unwrap_or(0)
                                }
                            };
                            Status::Ok
                        }
                        Err(error) => {
                            diag_last_error = hal_error_code(error);
                            Status::InternalError
                        }
                    },
                    PendingAction::CrashClear => match crash_store.clear().and_then(|_| {
                        if crash_store.read()?.is_none() {
                            Ok(())
                        } else {
                            Err(HalError::Flash)
                        }
                    }) {
                        Ok(()) => Status::Ok,
                        Err(error) => {
                            diag_last_error = hal_error_code(error);
                            Status::InternalError
                        }
                    },
                };
                let _ = binbook_fw::diag::complete_pending_command(
                    &mut diag_serial_state,
                    pending,
                    status,
                    current_page,
                    &action_payload[..action_payload_len],
                );
            }

            if !diag_serial_state.pending_tx().is_empty() {
                let pending_len = diag_serial_state.pending_tx().len();
                if usb_tx.write(diag_serial_state.pending_tx()).is_ok() {
                    diag_serial_state.consume_tx(pending_len);
                }
            }

            diag_deduper.push_idle_or_summary(&mut diag_log, tick as u32);
        }
    }
}

#[derive(Clone, Copy)]
struct RenderReport {
    decision: binbook_fw::refresh::RefreshDecision,
    panel_mode: binbook_fw::display::PanelMode,
}

fn render_current_page<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut binbook::BinBook<&[u8], &mut [u8; BINBOOK_SCRATCH_BYTES]>,
    delay: &dyn xteink_hal::Delay,
    refresh_state: &mut binbook_fw::refresh::RefreshState,
    panel_mode: &mut binbook_fw::display::PanelMode,
    page_index: u32,
) -> HalResult<RenderReport>
where
    SPI: xteink_hal::Spi,
    CS: xteink_hal::OutputPin,
    DC: xteink_hal::OutputPin,
    RST: xteink_hal::OutputPin,
    BUSY: xteink_hal::InputPin,
{
    #[cfg(not(feature = "chunk-dirty-probe"))]
    let decision = refresh_state.decide(
        page_index,
        binbook_fw::display::find_transition_mask(book, refresh_state.previous_page(), page_index),
    );
    #[cfg(feature = "chunk-dirty-probe")]
    let decision = refresh_state.decide_with_policy(
        page_index,
        binbook_fw::display::find_transition_mask(book, refresh_state.previous_page(), page_index),
        binbook_fw::refresh::RefreshPolicy::ChunkDirtyDifferentialDefault,
    );
    #[cfg(not(feature = "chunk-dirty-probe"))]
    {
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
            return Err(e);
        }
    }
    #[cfg(feature = "chunk-dirty-probe")]
    {
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
            return Err(e);
        }
    }
    Ok(RenderReport {
        decision,
        panel_mode: *panel_mode,
    })
}

#[cfg(feature = "diagnostic-console")]
fn refresh_decision_code(decision: binbook_fw::refresh::RefreshDecision) -> i32 {
    use binbook_fw::refresh::RefreshDecision;
    match decision {
        RefreshDecision::FullGrayscale => 1,
        RefreshDecision::FullBwSeed => 2,
        RefreshDecision::AdjacentDirtyPartial { .. } => 3,
        RefreshDecision::FullScreenDifferential => 4,
        RefreshDecision::Noop => 5,
    }
}

#[cfg(feature = "diagnostic-console")]
fn record_render_success<const N: usize>(
    log: &mut binbook_fw::diag_log::DiagLog<N>,
    tick_ms: u32,
    page: u32,
    report: RenderReport,
) {
    use binbook_fw::diag_log::{
        DiagEvent, EVT_PANEL_MODE, EVT_REFRESH_DECISION, EVT_RENDER_SUCCESS, LEVEL_INFO,
        SUB_DISPLAY,
    };
    log.push(
        tick_ms,
        DiagEvent {
            level: LEVEL_INFO,
            subsystem: SUB_DISPLAY,
            event: EVT_REFRESH_DECISION,
            arg0: refresh_decision_code(report.decision),
            arg1: page as i32,
            arg2: 0,
        },
    );
    log.push(
        tick_ms,
        DiagEvent {
            level: LEVEL_INFO,
            subsystem: SUB_DISPLAY,
            event: EVT_PANEL_MODE,
            arg0: panel_mode_code(report.panel_mode) as i32,
            arg1: page as i32,
            arg2: 0,
        },
    );
    log.push(
        tick_ms,
        DiagEvent {
            level: LEVEL_INFO,
            subsystem: SUB_DISPLAY,
            event: EVT_RENDER_SUCCESS,
            arg0: page as i32,
            arg1: panel_mode_code(report.panel_mode) as i32,
            arg2: 0,
        },
    );
}

#[cfg(feature = "diagnostic-console")]
fn record_display_failure<F: xteink_hal::Flash, const N: usize>(
    log: &mut binbook_fw::diag_log::DiagLog<N>,
    crash_store: &mut binbook_fw::diag_flash::CrashStore<F>,
    tick_ms: u32,
    current_page: u32,
    panel_mode: binbook_fw::display::PanelMode,
    error_code: i32,
) {
    use binbook_fw::diag_log::{
        CrashLogSlot, CrashSummary, DiagEvent, CRASH_LOG_RECORDS, EVT_DISPLAY_ERROR,
        EVT_RENDER_FAILURE, LEVEL_ERROR, SUB_DISPLAY,
    };
    log.push(
        tick_ms,
        DiagEvent {
            level: LEVEL_ERROR,
            subsystem: SUB_DISPLAY,
            event: EVT_RENDER_FAILURE,
            arg0: current_page as i32,
            arg1: error_code,
            arg2: panel_mode_code(panel_mode) as i32,
        },
    );
    log.push(
        tick_ms,
        DiagEvent {
            level: LEVEL_ERROR,
            subsystem: SUB_DISPLAY,
            event: EVT_DISPLAY_ERROR,
            arg0: error_code,
            arg1: current_page as i32,
            arg2: panel_mode_code(panel_mode) as i32,
        },
    );
    let mut records = [CrashLogSlot::default(); CRASH_LOG_RECORDS];
    let copied_log_count = log.copy_recent_crash_slots(&mut records) as u8;
    let summary = CrashSummary {
        flags: 1,
        copied_log_count,
        panel_mode: panel_mode_code(panel_mode),
        boot_counter: 0,
        last_error: error_code,
        last_page: current_page,
        last_log_sequence: log.newest_sequence().unwrap_or(0),
        records,
    };
    let _ = crash_store.write_fatal(&summary);
}
