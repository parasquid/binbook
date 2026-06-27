use embassy_executor::Spawner;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Receiver, Sender},
};
use esp_hal::{
    analog::adc::{Adc, AdcCalBasic, AdcConfig, Attenuation},
    delay::Delay as EspDelay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig},
    spi::{
        master::{Config as SpiConfig, Spi as EspSpi},
        Mode,
    },
    time::Rate,
};

use crate::{
    BINBOOK_SCRATCH_BYTES, Delay, InputPin, OutputPin, PROBE_BOOK, Spi,
};
use binbook_fw::{
    async_refresh::{
        DisplayCompletion, DisplayCompletionStatus, DisplayRequest, PostGrayStrategy,
        RefreshAction, RefreshCoordinator, RefreshPhase, DISPLAY_COMPLETION_CAPACITY,
        GRAY_SETTLE_DELAY_MS, INPUT_POLL_INTERVAL_MS, PAGE_TURN_QUEUE_CAPACITY,
    },
    display::PanelMode,
    input::{self, InputState},
};
#[cfg(feature = "diagnostic-console")]
use binbook_fw::async_refresh::DisplayProbeKind;
use ssd1677_driver::Ssd1677Driver;

type RequestSender =
    Sender<'static, CriticalSectionRawMutex, DisplayRequest, { PAGE_TURN_QUEUE_CAPACITY }>;
type RequestReceiver =
    Receiver<'static, CriticalSectionRawMutex, DisplayRequest, { PAGE_TURN_QUEUE_CAPACITY }>;
type CompletionSender =
    Sender<'static, CriticalSectionRawMutex, DisplayCompletion, { DISPLAY_COMPLETION_CAPACITY }>;

static REQUEST_CHANNEL: Channel<CriticalSectionRawMutex, DisplayRequest, { PAGE_TURN_QUEUE_CAPACITY }> =
    Channel::new();
static COMPLETION_CHANNEL: Channel<CriticalSectionRawMutex, DisplayCompletion, { DISPLAY_COMPLETION_CAPACITY }> =
    Channel::new();

const POST_GRAY_STRATEGY: PostGrayStrategy = PostGrayStrategy::SilentReseed;
const GRAY_POLL_INTERVAL_MS: u64 = 10;

#[embassy_executor::task]
async fn input_task(
    adc1: esp_hal::peripherals::ADC1<'static>,
    gpio1: esp_hal::peripherals::GPIO1<'static>,
    gpio2: esp_hal::peripherals::GPIO2<'static>,
    request_tx: RequestSender,
) {
    let mut adc_config = AdcConfig::new();
    let mut ch1_pin =
        adc_config.enable_pin_with_cal::<_, AdcCalBasic<_>>(gpio1, Attenuation::_11dB);
    let mut ch2_pin =
        adc_config.enable_pin_with_cal::<_, AdcCalBasic<_>>(gpio2, Attenuation::_11dB);
    let mut adc = Adc::new(adc1, adc_config);
    let mut input_state = InputState::new();
    let mut tick: u64 = 0;

    loop {
        embassy_time::Timer::after_millis(INPUT_POLL_INTERVAL_MS).await;
        tick = tick.saturating_add(INPUT_POLL_INTERVAL_MS);

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

        if let Some(event) = input_state.poll_raw(ch1, ch2, tick) {
            if let input::ButtonEvent::Press(button) = event {
                if let Some(turn) = input::page_turn_for_button(button) {
                    let _ = request_tx.try_send(DisplayRequest::Turn {
                        turn,
                        completion_sequence: None,
                    });
                }
            }
        }
    }
}

#[embassy_executor::task]
async fn display_task(
    spi2: esp_hal::peripherals::SPI2<'static>,
    gpio8: esp_hal::peripherals::GPIO8<'static>,
    gpio10: esp_hal::peripherals::GPIO10<'static>,
    gpio21: esp_hal::peripherals::GPIO21<'static>,
    gpio4: esp_hal::peripherals::GPIO4<'static>,
    gpio5: esp_hal::peripherals::GPIO5<'static>,
    gpio6: esp_hal::peripherals::GPIO6<'static>,
    request_rx: RequestReceiver,
    completion_tx: CompletionSender,
) {
    let spi = EspSpi::new(
        spi2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(crate::DISPLAY_SPI_FREQUENCY_MHZ))
            .with_mode(Mode::_0),
    )
    .expect("failed to configure SPI2")
    .with_sck(gpio8)
    .with_mosi(gpio10);

    let cs = OutputPin(Output::new(gpio21, Level::High, OutputConfig::default()));
    let dc = OutputPin(Output::new(gpio4, Level::Low, OutputConfig::default()));
    let rst = OutputPin(Output::new(gpio5, Level::High, OutputConfig::default()));
    let busy = InputPin(Input::new(gpio6, InputConfig::default()));
    let mut display = Ssd1677Driver::new(Spi(spi), cs, dc, rst, busy);
    let delay = Delay(EspDelay::new());

    let mut scratch = [0u8; BINBOOK_SCRATCH_BYTES];
    let mut book =
        binbook::BinBook::open(PROBE_BOOK, &mut scratch).expect("failed to open embedded BinBook");
    let page_count = book.page_count();
    let mut current_page: u32 = 0;
    let mut panel_mode = PanelMode::Unknown;
    let mut coordinator = RefreshCoordinator::new(page_count, POST_GRAY_STRATEGY);

    if let RefreshAction::RenderGray { page } = coordinator.next_action() {
        let _ = binbook_fw::display::display_full_grayscale_async(
            &mut display,
            &mut book,
            PROBE_BOOK,
            page,
            &delay,
        )
        .await;
        let _ = coordinator.record_gray_complete();
        let _ = binbook_fw::display::reseed_bw_silent_async(
            &mut display,
            &mut book,
            PROBE_BOOK,
            page,
            &delay,
        )
        .await;
        let _ = coordinator.record_reseed_complete();
    }

    loop {
        let request = request_rx.receive().await;
        let (target_page, completion_sequence) = match request {
            DisplayRequest::Turn {
                turn,
                completion_sequence,
            } => (
                input::apply_page_turn(current_page, page_count, turn),
                completion_sequence,
            ),
            DisplayRequest::Goto {
                page,
                completion_sequence,
            } => (page, Some(completion_sequence)),
            DisplayRequest::Probe { .. } => continue,
        };

        if target_page == current_page {
            if let Some(sequence) = completion_sequence {
                let _ = completion_tx.try_send(DisplayCompletion {
                    sequence,
                    status: DisplayCompletionStatus::Ok,
                    page: current_page,
                });
            }
            continue;
        }

        let _ = coordinator.start_bw(target_page);
        let prev_page = current_page;

        if let RefreshAction::RenderBw { from, target } = coordinator.next_action() {
            let _ = binbook_fw::display::bw_differential_async(
                &mut display,
                &mut book,
                PROBE_BOOK,
                from,
                target,
                &delay,
            )
            .await;
            current_page = target;
            let now_ms = embassy_time::Instant::now().as_millis();
            let _ = coordinator.record_bw_complete(target, now_ms);
        } else {
            continue;
        }

        if let Some(sequence) = completion_sequence {
            let _ = completion_tx.try_send(DisplayCompletion {
                sequence,
                status: DisplayCompletionStatus::Ok,
                page: current_page,
            });
        }

        loop {
            match coordinator.next_action() {
                RefreshAction::WaitUntil { deadline_ms } => {
                    let now_ms = embassy_time::Instant::now().as_millis();
                    if now_ms >= deadline_ms {
                        let _ = coordinator.gray_deadline_elapsed(now_ms);
                        break;
                    }

                    if let Ok(pending_request) = request_rx.try_receive() {
                        let _ = coordinator.request_arrived();
                        let request = pending_request;
                        if matches!(request, DisplayRequest::Probe { .. }) {
                            continue;
                        }
                        let (next_target, next_completion) = match request {
                            DisplayRequest::Turn {
                                turn,
                                completion_sequence,
                            } => (
                                input::apply_page_turn(current_page, page_count, turn),
                                completion_sequence,
                            ),
                            DisplayRequest::Goto {
                                page,
                                completion_sequence,
                            } => (page, Some(completion_sequence)),
                            DisplayRequest::Probe { .. } => continue,
                        };

                        if next_target == current_page {
                            if let Some(sequence) = next_completion {
                                let _ = completion_tx.try_send(DisplayCompletion {
                                    sequence,
                                    status: DisplayCompletionStatus::Ok,
                                    page: current_page,
                                });
                            }
                            break;
                        }

                        let _ = coordinator.start_bw(next_target);
                        if let RefreshAction::RenderBw { from, target } = coordinator.next_action()
                        {
                            let _ = binbook_fw::display::bw_differential_async(
                                &mut display,
                                &mut book,
                                PROBE_BOOK,
                                from,
                                target,
                                &delay,
                            )
                            .await;
                            current_page = target;
                            let now_ms = embassy_time::Instant::now().as_millis();
                            let _ = coordinator.record_bw_complete(target, now_ms);
                            if let Some(sequence) = next_completion {
                                let _ = completion_tx.try_send(DisplayCompletion {
                                    sequence,
                                    status: DisplayCompletionStatus::Ok,
                                    page: current_page,
                                });
                            }
                            continue;
                        }
                    }

                    embassy_time::Timer::after_millis(GRAY_POLL_INTERVAL_MS).await;
                }
                RefreshAction::RenderGray { page } => {
                    let _ = binbook_fw::display::display_full_grayscale_async(
                        &mut display,
                        &mut book,
                        PROBE_BOOK,
                        page,
                        &delay,
                    )
                    .await;
                    let _ = coordinator.record_gray_complete();
                    let _ = binbook_fw::display::reseed_bw_silent_async(
                        &mut display,
                        &mut book,
                        PROBE_BOOK,
                        page,
                        &delay,
                    )
                    .await;
                    let _ = coordinator.record_reseed_complete();
                    break;
                }
                RefreshAction::ReseedBw { page, visible } => {
                    if visible {
                        let _ = binbook_fw::display::reseed_bw_visible_async(
                            &mut display,
                            &mut book,
                            PROBE_BOOK,
                            page,
                            &delay,
                        )
                        .await;
                    } else {
                        let _ = binbook_fw::display::reseed_bw_silent_async(
                            &mut display,
                            &mut book,
                            PROBE_BOOK,
                            page,
                            &delay,
                        )
                        .await;
                    }
                    let _ = coordinator.record_reseed_complete();
                    break;
                }
                RefreshAction::WaitForRequest | RefreshAction::None => break,
                RefreshAction::RenderBw { .. } => break,
                RefreshAction::RecoverBw { page } => {
                    let _ = binbook_fw::display::recovery_seed_async(
                        &mut display,
                        &mut book,
                        PROBE_BOOK,
                        page,
                        &delay,
                    )
                    .await;
                    let _ = coordinator.record_recovery_complete(page, embassy_time::Instant::now().as_millis());
                    break;
                }
            }
        }
    }
}

#[cfg(feature = "diagnostic-console")]
#[embassy_executor::task]
async fn diagnostic_task(usb_device: esp_hal::peripherals::USB_DEVICE) {
    let _usb = esp_hal::usb_serial_jtag::UsbSerialJtag::new(usb_device);
    loop {
        embassy_time::Timer::after_millis(1_000).await;
    }
}

pub(crate) async fn run(spawner: Spawner) {
    let peripherals = esp_hal::init(esp_hal::Config::default());

    let request_tx = REQUEST_CHANNEL.sender();
    let request_rx = REQUEST_CHANNEL.receiver();
    let completion_tx = COMPLETION_CHANNEL.sender();

    spawner.spawn(
        input_task(
            peripherals.ADC1,
            peripherals.GPIO1,
            peripherals.GPIO2,
            request_tx,
        )
        .unwrap(),
    );
    spawner.spawn(
        display_task(
            peripherals.SPI2,
            peripherals.GPIO8,
            peripherals.GPIO10,
            peripherals.GPIO21,
            peripherals.GPIO4,
            peripherals.GPIO5,
            peripherals.GPIO6,
            request_rx,
            completion_tx,
        )
        .unwrap(),
    );

    #[cfg(feature = "diagnostic-console")]
    spawner.spawn(diagnostic_task(peripherals.USB_DEVICE).unwrap());
}
