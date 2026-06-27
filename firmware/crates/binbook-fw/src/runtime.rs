use embassy_executor::Spawner;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Receiver, Sender},
};
#[cfg(feature = "diagnostic-console")]
use core::cell::RefCell;
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
#[cfg(feature = "diagnostic-console")]
use embedded_storage::ReadStorage;

use crate::{
    BINBOOK_SCRATCH_BYTES, Delay, InputPin, OutputPin, PROBE_BOOK, Spi,
};
use binbook_fw::{
    async_refresh::{
        DisplayCompletion, DisplayCompletionStatus, DisplayRequest, PostGrayStrategy,
        RefreshAction, RefreshCoordinator, DISPLAY_COMPLETION_CAPACITY, INPUT_POLL_INTERVAL_MS,
        PAGE_TURN_QUEUE_CAPACITY,
    },
    display::PanelMode,
    input::{self, InputState},
};
#[cfg(feature = "diagnostic-console")]
use binbook_fw::{
    diag::{
        complete_pending_command, dispatch_command, DiagnosticLoopState, DiagnosticSnapshot,
        PendingAction, PendingCommand, SerialState, TransportError,
    },
    diag_flash::CrashStore,
    diag_log::{
        DiagDeduper, DiagEvent, DiagLog, LEVEL_ERROR, LEVEL_INFO, SUB_DISPLAY, SUB_INPUT,
        SUB_SERIAL, SUB_SYSTEM,
    },
};
#[cfg(feature = "diagnostic-console")]
use binbook_fw::async_refresh::DisplayProbeKind;
#[cfg(feature = "diagnostic-console")]
use binbook_diagnostic_protocol::{encode_crash_response, CRASH_SUMMARY_BYTES, Status};
use ssd1677_driver::Ssd1677Driver;

type RequestSender =
    Sender<'static, CriticalSectionRawMutex, DisplayRequest, { PAGE_TURN_QUEUE_CAPACITY }>;
type RequestReceiver =
    Receiver<'static, CriticalSectionRawMutex, DisplayRequest, { PAGE_TURN_QUEUE_CAPACITY }>;
type CompletionSender =
    Sender<'static, CriticalSectionRawMutex, DisplayCompletion, { DISPLAY_COMPLETION_CAPACITY }>;
type CompletionReceiver =
    Receiver<'static, CriticalSectionRawMutex, DisplayCompletion, { DISPLAY_COMPLETION_CAPACITY }>;

static REQUEST_CHANNEL: Channel<CriticalSectionRawMutex, DisplayRequest, { PAGE_TURN_QUEUE_CAPACITY }> =
    Channel::new();
static COMPLETION_CHANNEL: Channel<CriticalSectionRawMutex, DisplayCompletion, { DISPLAY_COMPLETION_CAPACITY }> =
    Channel::new();

const POST_GRAY_STRATEGY: PostGrayStrategy =
    binbook_fw::async_refresh::configured_post_gray_strategy();
const GRAY_POLL_INTERVAL_MS: u64 = 10;

#[cfg(feature = "diagnostic-console")]
struct X4Flash(RefCell<esp_storage::FlashStorage<'static>>);

#[cfg(feature = "diagnostic-console")]
impl xteink_hal::Flash for X4Flash {
    fn read(&self, offset: u32, buf: &mut [u8]) -> xteink_hal::HalResult<()> {
        embedded_storage::nor_flash::ReadNorFlash::read(&mut *self.0.borrow_mut(), offset, buf)
            .map_err(|_| xteink_hal::HalError::Flash)
    }

    fn write(&mut self, offset: u32, data: &[u8]) -> xteink_hal::HalResult<()> {
        embedded_storage::nor_flash::NorFlash::write(&mut *self.0.borrow_mut(), offset, data)
            .map_err(|_| xteink_hal::HalError::Flash)
    }

    fn erase_sector(&mut self, offset: u32) -> xteink_hal::HalResult<()> {
        embedded_storage::nor_flash::NorFlash::erase(
            &mut *self.0.borrow_mut(),
            offset,
            offset + binbook_fw::flash::CRASH_SECTOR_SIZE,
        )
        .map_err(|_| xteink_hal::HalError::Flash)
    }

    fn size(&self) -> u32 {
        self.0.borrow().capacity() as u32
    }
}

#[cfg(feature = "diagnostic-console")]
fn panel_mode_code(mode: binbook_diagnostic_protocol::PanelModeCode) -> u8 {
    mode as u8
}

#[cfg(feature = "diagnostic-console")]
fn hal_error_code(error: xteink_hal::HalError) -> i32 {
    match error {
        xteink_hal::HalError::Spi => -1,
        xteink_hal::HalError::Gpio => -2,
        xteink_hal::HalError::Flash => -3,
        xteink_hal::HalError::Timeout => -4,
        xteink_hal::HalError::InvalidParam => -5,
    }
}

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
async fn diagnostic_task(
    usb_device: esp_hal::peripherals::USB_DEVICE<'static>,
    flash: esp_hal::peripherals::FLASH<'static>,
    request_tx: RequestSender,
    completion_rx: CompletionReceiver,
) {
    use binbook_diagnostic_protocol::{
        decode_frame, decode_key_payload, decode_log_get_payload, decode_page_payload,
        decode_probe_payload, encode_log_response_header, encode_page_response, encode_status_payload,
        FrameKind, Opcode, RawFrameHeader, Status, StatusPayload, LOG_RESPONSE_HEADER_BYTES,
        MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES,
    };
    use binbook_fw::diag::{resolve_log_clear, TransportError};
    use binbook_fw::input::{apply_page_turn, page_turn_for_button, Button};
    use esp_hal::usb_serial_jtag::UsbSerialJtag;

    let mut usb = UsbSerialJtag::new(usb_device);
    let (mut usb_rx, mut usb_tx) = usb.split();
    let mut diag_serial_state = SerialState::new();
    let mut diag_log = DiagLog::<{ binbook_fw::diag_log::DEFAULT_LOG_CAPACITY }>::new();
    let mut diag_deduper = DiagDeduper::new();
    let mut crash_store = CrashStore::new(X4Flash(RefCell::new(esp_storage::FlashStorage::new(
        flash,
    ))));

    let mut book_scratch = [0u8; BINBOOK_SCRATCH_BYTES];
    let book = binbook::BinBook::open(PROBE_BOOK, &mut book_scratch)
        .expect("failed to open embedded BinBook");
    let page_count = book.page_count();
    let mut snapshot = DiagnosticSnapshot {
        current_page: 0,
        page_count,
        panel_mode: binbook_diagnostic_protocol::PanelModeCode::Unknown,
        dropped_log_count: 0,
        protocol_error_count: 0,
        last_error: 0,
    };
    let mut loop_state =
        DiagnosticLoopState::<{ PAGE_TURN_QUEUE_CAPACITY }, { binbook_fw::diag_log::DEFAULT_LOG_CAPACITY }>::new(
            snapshot,
            diag_log,
        );
    loop_state.log_mut().push_event(
        DiagEvent {
            level: LEVEL_INFO,
            subsystem: SUB_SYSTEM,
            event: binbook_fw::diag_log::EVT_FIRMWARE_STARTED,
            arg0: page_count as i32,
            arg1: 0,
            arg2: 0,
        },
        0,
    );

    let mut pending_display: binbook_fw::diag::DiagnosticPendingQueue<{ PAGE_TURN_QUEUE_CAPACITY }> =
        binbook_fw::diag::DiagnosticPendingQueue::new();
    let mut tick: u64 = 0;

    loop {
        let mut usb_read_buf = [0u8; 64];
        let n = usb_rx.drain_rx_fifo(&mut usb_read_buf);
        if n > 0 {
            diag_serial_state.feed_rx(&usb_read_buf[..n]);
        }

        while let Ok(completion) = completion_rx.try_receive() {
            if let Some(pending) = pending_display.pop() {
                snapshot.current_page = completion.page;
                snapshot.panel_mode = binbook_diagnostic_protocol::PanelModeCode::Bw;
                loop_state.update_snapshot(snapshot);
                loop_state.log_mut().push(
                    tick as u32,
                    DiagEvent {
                        level: LEVEL_INFO,
                        subsystem: SUB_DISPLAY,
                        event: binbook_fw::diag_log::EVT_TURN_DEQUEUED,
                        arg0: completion.sequence as i32,
                        arg1: completion.page as i32,
                        arg2: 0,
                    },
                );
                let status = match completion.status {
                    DisplayCompletionStatus::Ok => Status::Ok,
                    DisplayCompletionStatus::Error => {
                        snapshot.last_error = -4;
                        loop_state.update_snapshot(snapshot);
                        Status::Error
                    }
                };
                let _ = complete_pending_command(
                    &mut diag_serial_state,
                    pending,
                    status,
                    completion.page,
                    &[],
                );
            }
        }

        let pending = binbook_fw::diag::poll_pending_command(
            &mut diag_serial_state,
            snapshot.current_page,
            snapshot.page_count,
            snapshot.last_error,
            panel_mode_code(snapshot.panel_mode),
            loop_state.log_mut(),
            tick as u32,
        );

        if let Some(pending) = pending {
            let mut response_status = Status::Ok;
            let mut response_payload = [0u8; 1 + CRASH_SUMMARY_BYTES];
            let mut response_payload_len = 0usize;
            match pending.action {
                PendingAction::RenderTurn { turn } => {
                    let _ = request_tx.try_send(DisplayRequest::Turn {
                        turn,
                        completion_sequence: Some(pending.header.sequence),
                    })
                    .map_err(|_| {
                        response_status = Status::Error;
                    });
                    if response_status == Status::Ok {
                        let _ = pending_display.try_push(pending);
                        loop_state.log_mut().push(
                            tick as u32,
                            DiagEvent {
                                level: LEVEL_INFO,
                                subsystem: SUB_DISPLAY,
                                event: binbook_fw::diag_log::EVT_TURN_QUEUED,
                                arg0: turn as i32,
                                arg1: pending.header.sequence as i32,
                                arg2: 0,
                            },
                        );
                    } else {
                        loop_state.log_mut().push(
                            tick as u32,
                            DiagEvent {
                                level: LEVEL_ERROR,
                                subsystem: SUB_DISPLAY,
                                event: binbook_fw::diag_log::EVT_TURN_DROPPED,
                                arg0: turn as i32,
                                arg1: pending.header.sequence as i32,
                                arg2: 0,
                            },
                        );
                        let _ = complete_pending_command(
                            &mut diag_serial_state,
                            pending,
                            Status::Error,
                            snapshot.current_page,
                            &[],
                        );
                    }
                }
                PendingAction::RenderPage { target_page } => {
                    let _ = request_tx.try_send(DisplayRequest::Goto {
                        page: target_page,
                        completion_sequence: pending.header.sequence,
                    })
                    .map_err(|_| {
                        response_status = Status::Error;
                    });
                    if response_status == Status::Ok {
                        let _ = pending_display.try_push(pending);
                    } else {
                        let _ = complete_pending_command(
                            &mut diag_serial_state,
                            pending,
                            Status::Error,
                            snapshot.current_page,
                            &[],
                        );
                    }
                }
                PendingAction::DisplayProbe(probe) => {
                    let kind = match probe {
                        binbook_fw::diag::DisplayProbeKind::FullRefreshCurrent => {
                            DisplayProbeKind::FullRefreshCurrent
                        }
                        binbook_fw::diag::DisplayProbeKind::ClearWhite => {
                            DisplayProbeKind::ClearWhite
                        }
                        binbook_fw::diag::DisplayProbeKind::WindowCorners => {
                            DisplayProbeKind::WindowCorners
                        }
                    };
                    let _ = request_tx.try_send(DisplayRequest::Probe {
                        kind,
                        completion_sequence: pending.header.sequence,
                    })
                    .map_err(|_| {
                        response_status = Status::Error;
                    });
                    if response_status == Status::Ok {
                        let _ = pending_display.try_push(pending);
                    } else {
                        let _ = complete_pending_command(
                            &mut diag_serial_state,
                            pending,
                            Status::Error,
                            snapshot.current_page,
                            &[],
                        );
                    }
                }
                PendingAction::CrashGet => match crash_store.read() {
                    Ok(summary) => {
                        response_payload_len = match summary {
                            Some(summary) => {
                                let mut encoded_summary = [0u8; CRASH_SUMMARY_BYTES];
                                summary.encode(&mut encoded_summary);
                                encode_crash_response(Some(&encoded_summary), &mut response_payload)
                                    .unwrap_or(0)
                            }
                            None => encode_crash_response(None, &mut response_payload).unwrap_or(0),
                        };
                    }
                    Err(_) => {
                        response_status = Status::InternalError;
                    }
                },
                PendingAction::CrashClear => match crash_store.clear().and_then(|_| {
                    if crash_store.read()?.is_none() {
                        Ok(())
                    } else {
                        Err(xteink_hal::HalError::Flash)
                    }
                }) {
                    Ok(()) => {}
                    Err(error) => {
                        response_status = Status::InternalError;
                        snapshot.last_error = hal_error_code(error);
                        loop_state.update_snapshot(snapshot);
                    }
                },
            }

            if matches!(pending.action, PendingAction::CrashGet | PendingAction::CrashClear) {
                let _ = complete_pending_command(
                    &mut diag_serial_state,
                    pending,
                    response_status,
                    snapshot.current_page,
                    &response_payload[..response_payload_len],
                );
            }
        }

        if !diag_serial_state.pending_tx().is_empty() {
            let pending_len = diag_serial_state.pending_tx().len();
            if usb_tx.write(diag_serial_state.pending_tx()).is_ok() {
                diag_serial_state.consume_tx(pending_len);
            }
        }

        diag_deduper.push_idle_or_summary(loop_state.log_mut(), tick as u32);
        loop_state.update_snapshot(snapshot);
        snapshot.dropped_log_count = loop_state.log_mut().dropped_records();
        snapshot.protocol_error_count = diag_serial_state.protocol_error_count();
        loop_state.update_snapshot(snapshot);
        tick = tick.saturating_add(1);
        embassy_time::Timer::after_millis(GRAY_POLL_INTERVAL_MS).await;
    }
}

pub(crate) async fn run(spawner: Spawner) {
    let peripherals = esp_hal::init(esp_hal::Config::default());

    let request_tx_input = REQUEST_CHANNEL.sender();
    let request_tx_diag = REQUEST_CHANNEL.sender();
    let request_rx = REQUEST_CHANNEL.receiver();
    let completion_tx = COMPLETION_CHANNEL.sender();
    let completion_rx = COMPLETION_CHANNEL.receiver();

    spawner.spawn(
        input_task(
            peripherals.ADC1,
            peripherals.GPIO1,
            peripherals.GPIO2,
            request_tx_input,
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
    spawner.spawn(
        diagnostic_task(
            peripherals.USB_DEVICE,
            peripherals.FLASH,
            request_tx_diag,
            completion_rx,
        )
        .unwrap(),
    );
}
