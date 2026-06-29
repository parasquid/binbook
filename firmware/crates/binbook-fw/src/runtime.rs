#[cfg(feature = "diagnostic-console")]
use core::cell::RefCell;
use embassy_executor::Spawner;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Receiver, Sender},
};
#[cfg(feature = "diagnostic-console")]
use embedded_storage::ReadStorage;
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
use portable_atomic::{AtomicU32, Ordering};

use binbook_fw::panel_driver::{new_legacy_display, LegacyDisplayDriver};
use crate::{Delay, InputPin, OutputPin, Spi, BINBOOK_SCRATCH_BYTES, PROBE_BOOK};
#[cfg(feature = "diagnostic-console")]
use binbook_diagnostic_protocol::{encode_crash_response, CRASH_SUMMARY_BYTES};
#[cfg(feature = "diagnostic-console")]
use binbook_fw::async_refresh::DisplayProbeKind;
#[cfg(feature = "diagnostic-console")]
use binbook_fw::{
    async_refresh::DISPLAY_COMPLETION_CAPACITY, runtime_engine::RuntimeCompletionStatus,
};
use binbook_fw::{
    async_refresh::{DisplayRequest, INPUT_POLL_INTERVAL_MS, PAGE_TURN_QUEUE_CAPACITY},
    input::{self, InputState},
    runtime_engine::{DisplayBackend, DisplayEngine, EventSink, RuntimeEvent},
};
#[cfg(feature = "diagnostic-console")]
use binbook_fw::{
    diag::{
        complete_pending_command, DiagnosticSnapshot, PendingAction, PendingCommand, SerialState,
    },
    diag_flash::CrashStore,
};

type RequestSender =
    Sender<'static, CriticalSectionRawMutex, DisplayRequest, { PAGE_TURN_QUEUE_CAPACITY }>;
type RequestReceiver =
    Receiver<'static, CriticalSectionRawMutex, DisplayRequest, { PAGE_TURN_QUEUE_CAPACITY }>;

static REQUEST_CHANNEL: Channel<
    CriticalSectionRawMutex,
    DisplayRequest,
    { PAGE_TURN_QUEUE_CAPACITY },
> = Channel::new();
static RUNTIME_EVENT_CHANNEL: Channel<CriticalSectionRawMutex, RuntimeEvent, 32> = Channel::new();
static REQUEST_EPOCH: AtomicU32 = AtomicU32::new(0);

#[cfg(feature = "diagnostic-console")]
type CommittedCompletion = binbook_fw::runtime_aggregator::CommittedCompletion;

#[cfg(feature = "diagnostic-console")]
#[derive(Clone, Copy)]
enum AggregatorQuery {
    Enqueue {
        pending: PendingCommand,
        request: DisplayRequest,
    },
    Status,
    LogGet {
        cursor: u32,
        max_bytes: u16,
    },
    LogClear,
    ProtocolErrors(u32),
}

#[cfg(feature = "diagnostic-console")]
#[derive(Clone, Copy)]
enum AggregatorResponse {
    Reserve(Result<(), binbook_fw::runtime_aggregator::ReserveError>),
    Status(DiagnosticSnapshot),
    Log {
        payload: [u8; binbook_diagnostic_protocol::MAX_PAYLOAD_BYTES],
        len: usize,
    },
    Ack,
}

#[cfg(feature = "diagnostic-console")]
static AGGREGATOR_QUERY_CHANNEL: Channel<CriticalSectionRawMutex, AggregatorQuery, 4> =
    Channel::new();
#[cfg(feature = "diagnostic-console")]
static AGGREGATOR_RESPONSE_CHANNEL: Channel<CriticalSectionRawMutex, AggregatorResponse, 4> =
    Channel::new();
#[cfg(feature = "diagnostic-console")]
static AGGREGATOR_COMPLETION_CHANNEL: Channel<
    CriticalSectionRawMutex,
    CommittedCompletion,
    { DISPLAY_COMPLETION_CAPACITY },
> = Channel::new();

pub(crate) struct RuntimePeripherals {
    pub(crate) adc1: esp_hal::peripherals::ADC1<'static>,
    pub(crate) gpio1: esp_hal::peripherals::GPIO1<'static>,
    pub(crate) gpio2: esp_hal::peripherals::GPIO2<'static>,
    pub(crate) spi2: esp_hal::peripherals::SPI2<'static>,
    pub(crate) gpio8: esp_hal::peripherals::GPIO8<'static>,
    pub(crate) gpio10: esp_hal::peripherals::GPIO10<'static>,
    pub(crate) gpio21: esp_hal::peripherals::GPIO21<'static>,
    pub(crate) gpio4: esp_hal::peripherals::GPIO4<'static>,
    pub(crate) gpio5: esp_hal::peripherals::GPIO5<'static>,
    pub(crate) gpio6: esp_hal::peripherals::GPIO6<'static>,
    #[cfg(feature = "diagnostic-console")]
    pub(crate) usb_device: esp_hal::peripherals::USB_DEVICE<'static>,
    #[cfg(feature = "diagnostic-console")]
    pub(crate) flash: esp_hal::peripherals::FLASH<'static>,
}

const GRAY_POLL_INTERVAL_MS: u64 = 10;

struct RuntimeEventSink {
    sender: Sender<'static, CriticalSectionRawMutex, RuntimeEvent, 32>,
    pending: [Option<RuntimeEvent>; 32],
    head: usize,
    len: usize,
}

impl RuntimeEventSink {
    fn new(sender: Sender<'static, CriticalSectionRawMutex, RuntimeEvent, 32>) -> Self {
        Self {
            sender,
            pending: [None; 32],
            head: 0,
            len: 0,
        }
    }

    async fn flush(&mut self) {
        while self.len > 0 {
            let event = self.pending[self.head]
                .take()
                .expect("runtime event buffer entry must exist");
            self.head = (self.head + 1) % self.pending.len();
            self.len -= 1;
            self.sender.send(event).await;
        }
    }

    fn buffer(&mut self, event: RuntimeEvent) {
        let index = (self.head + self.len) % self.pending.len();
        assert!(
            self.len < self.pending.len(),
            "runtime event buffer overflow"
        );
        self.pending[index] = Some(event);
        self.len += 1;
    }
}

impl EventSink for RuntimeEventSink {
    fn emit(&mut self, event: RuntimeEvent) {
        if self.len > 0 {
            self.buffer(event);
            return;
        }
        if let Err(embassy_sync::channel::TrySendError::Full(event)) = self.sender.try_send(event) {
            self.buffer(event);
        }
    }
}

struct HardwareDisplayBackend<'a, SPI, CS, DC, RST, BUSY> {
    display: LegacyDisplayDriver<SPI, CS, DC, RST, BUSY>,
    book: binbook_core::Book<binbook_core::SliceSource<'a>>,
    delay: Delay,
}

impl<'a, SPI, CS, DC, RST, BUSY> DisplayBackend
    for HardwareDisplayBackend<'a, SPI, CS, DC, RST, BUSY>
where
    SPI: xteink_hal::Spi,
    CS: xteink_hal::OutputPin,
    DC: xteink_hal::OutputPin,
    RST: xteink_hal::OutputPin,
    BUSY: xteink_hal::InputPin,
{
    fn timestamp_ms(&self) -> Option<u64> {
        Some(embassy_time::Instant::now().as_millis())
    }

    fn request_epoch(&self) -> u32 {
        REQUEST_EPOCH.load(Ordering::Acquire)
    }

    async fn init_grayscale(&mut self) -> xteink_hal::HalResult<()> {
        self.display.init_grayscale_async(&self.delay).await
    }

    async fn render_grayscale(
        &mut self,
        page: u32,
        expected_epoch: u32,
    ) -> xteink_hal::HalResult<binbook_fw::display::GrayRenderOutcome> {
        binbook_fw::display::display_staged_grayscale_async(
            &mut self.display,
            &mut self.book,
            PROBE_BOOK,
            page,
            expected_epoch,
            || REQUEST_EPOCH.load(Ordering::Acquire),
            || {
                let timestamp_ms = embassy_time::Instant::now().as_millis();
                let sender = RUNTIME_EVENT_CHANNEL.sender();
                let _ = sender.try_send(RuntimeEvent {
                    timestamp_ms,
                    kind: binbook_fw::runtime_engine::RuntimeEventKind::WaveformSelected {
                        waveform_hint: binbook_core::WAVEFORM_SSD1677_STAGED_GRAY2,
                        lut_revision: binbook_fw::panel_driver::STAGED_GRAY_LUT_REVISION,
                    },
                });
                let _ = sender.try_send(RuntimeEvent {
                    timestamp_ms,
                    kind: binbook_fw::runtime_engine::RuntimeEventKind::GrayActivated { page },
                });
            },
            &self.delay,
        )
        .await
    }

    async fn init_bw(&mut self) -> xteink_hal::HalResult<()> {
        self.display.init_async(&self.delay).await
    }

    async fn render_bw(&mut self, from: u32, target: u32) -> xteink_hal::HalResult<()> {
        binbook_fw::display::bw_differential_async(
            &mut self.display,
            &mut self.book,
            PROBE_BOOK,
            from,
            target,
            &self.delay,
        )
        .await
    }

    async fn sync_bw_base(
        &mut self,
        page: u32,
        expected_epoch: u32,
    ) -> xteink_hal::HalResult<binbook_fw::display::BaseSyncOutcome> {
        binbook_fw::display::sync_bw_base_async(
            &mut self.display,
            &mut self.book,
            PROBE_BOOK,
            page,
            expected_epoch,
            || REQUEST_EPOCH.load(Ordering::Acquire),
            &self.delay,
        )
        .await
    }

    async fn recover_bw(&mut self, page: u32) -> xteink_hal::HalResult<()> {
        binbook_fw::display::recovery_seed_async(
            &mut self.display,
            &mut self.book,
            PROBE_BOOK,
            page,
            &self.delay,
        )
        .await
    }

    async fn run_probe(
        &mut self,
        kind: binbook_fw::async_refresh::DisplayProbeKind,
        page: u32,
    ) -> xteink_hal::HalResult<()> {
        #[cfg(feature = "diagnostic-console")]
        {
            match kind {
                binbook_fw::async_refresh::DisplayProbeKind::FullRefreshCurrent => {
                    binbook_fw::display::display_full_grayscale_async(
                        &mut self.display,
                        &mut self.book,
                        PROBE_BOOK,
                        page,
                        &self.delay,
                    )
                    .await
                }
                binbook_fw::async_refresh::DisplayProbeKind::ClearWhite => {
                    binbook_fw::display::clear_white_probe_async(&mut self.display, &self.delay)
                        .await
                }
                binbook_fw::async_refresh::DisplayProbeKind::WindowCorners => {
                    binbook_fw::display::window_corners_probe_async(&mut self.display, &self.delay)
                        .await
                }
            }
        }
        #[cfg(not(feature = "diagnostic-console"))]
        {
            let _ = (kind, page);
            Err(xteink_hal::HalError::InvalidParam)
        }
    }
}

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

        let outcome = input_state.poll_raw_detailed(ch1, ch2, tick);
        let timestamp_ms = embassy_time::Instant::now().as_millis();
        if outcome.previous != outcome.observed {
            RUNTIME_EVENT_CHANNEL
                .sender()
                .send(RuntimeEvent {
                    timestamp_ms,
                    kind: binbook_fw::runtime_engine::RuntimeEventKind::InputTransition {
                        ch1,
                        ch2,
                        observed: outcome.observed,
                    },
                })
                .await;
        }
        if outcome.decision != input::InputDecision::Unchanged {
            RUNTIME_EVENT_CHANNEL
                .sender()
                .send(RuntimeEvent {
                    timestamp_ms,
                    kind: binbook_fw::runtime_engine::RuntimeEventKind::InputDecision {
                        observed: outcome.observed,
                        decision: outcome.decision,
                        elapsed_ms: outcome.elapsed_since_last_press_ms,
                    },
                })
                .await;
        }
        if let input::InputDecision::Press(button) = outcome.decision {
            if let Some(turn) = input::page_turn_for_button(button) {
                if request_tx
                    .try_send(DisplayRequest::Turn {
                        turn,
                        completion_sequence: None,
                    })
                    .is_ok()
                {
                    REQUEST_EPOCH.fetch_add(1, Ordering::AcqRel);
                } else {
                    RUNTIME_EVENT_CHANNEL
                        .sender()
                        .send(RuntimeEvent {
                            timestamp_ms: embassy_time::Instant::now().as_millis(),
                            kind: binbook_fw::runtime_engine::RuntimeEventKind::TurnDropped {
                                turn,
                            },
                        })
                        .await;
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
) {
    use embassy_futures::select::{select, Either};

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
    let mut scratch = [0u8; BINBOOK_SCRATCH_BYTES];
    let book = binbook_core::Book::open(binbook_core::SliceSource::new(PROBE_BOOK), &mut scratch)
        .expect("failed to open embedded BinBook");
    let page_count = book.page_count();
    let mut backend = HardwareDisplayBackend {
        display: new_legacy_display(Spi(spi), cs, dc, rst, busy),
        book,
        delay: Delay(EspDelay::new()),
    };
    let mut engine = DisplayEngine::new(page_count);
    let mut events = RuntimeEventSink::new(RUNTIME_EVENT_CHANNEL.sender());

    let _ = engine
        .initialize(
            &mut backend,
            &mut events,
            embassy_time::Instant::now().as_millis(),
        )
        .await;
    events.flush().await;

    loop {
        let now_ms = embassy_time::Instant::now().as_millis();
        match select(
            request_rx.receive(),
            embassy_time::Timer::after_millis(GRAY_POLL_INTERVAL_MS),
        )
        .await
        {
            Either::First(request) => {
                let _ = engine
                    .request(request, &mut backend, &mut events, now_ms)
                    .await;
                events.flush().await;
            }
            Either::Second(_) => {
                let _ = engine.advance(&mut backend, &mut events, now_ms).await;
                events.flush().await;
            }
        }
    }
}

#[cfg(feature = "diagnostic-console")]
#[embassy_executor::task]
async fn runtime_event_aggregator_task() {
    use binbook_diagnostic_protocol::{
        encode_log_response_header, LogResponseHeader, MAX_PAYLOAD_BYTES,
    };
    use binbook_fw::{diag::resolve_log_get, runtime_aggregator::RuntimeAggregator};
    use embassy_futures::select::{select, Either};

    let mut scratch = [0u8; BINBOOK_SCRATCH_BYTES];
    let book = binbook_core::Book::open(binbook_core::SliceSource::new(PROBE_BOOK), &mut scratch)
        .expect("failed to open embedded BinBook for diagnostics");
    let mut aggregator = RuntimeAggregator::<
        { PAGE_TURN_QUEUE_CAPACITY },
        { binbook_fw::diag_log::DEFAULT_LOG_CAPACITY },
    >::new(DiagnosticSnapshot {
        current_page: 0,
        page_count: book.page_count(),
        panel_mode: binbook_diagnostic_protocol::PanelModeCode::Unknown,
        dropped_log_count: 0,
        protocol_error_count: 0,
        last_error: 0,
    });
    aggregator.commit(RuntimeEvent {
        timestamp_ms: embassy_time::Instant::now().as_millis(),
        kind: binbook_fw::runtime_engine::RuntimeEventKind::FirmwareStarted {
            page_count: book.page_count(),
        },
    });
    let event_rx = RUNTIME_EVENT_CHANNEL.receiver();
    let query_rx = AGGREGATOR_QUERY_CHANNEL.receiver();
    let response_tx = AGGREGATOR_RESPONSE_CHANNEL.sender();
    let completion_tx = AGGREGATOR_COMPLETION_CHANNEL.sender();

    loop {
        match select(event_rx.receive(), query_rx.receive()).await {
            Either::First(event) => {
                if let Some(completion) = aggregator.commit(event) {
                    completion_tx.send(completion).await;
                }
            }
            Either::Second(query) => {
                let response = match query {
                    AggregatorQuery::Enqueue { pending, request } => {
                        AggregatorResponse::Reserve(aggregator.reserve_and_enqueue(pending, || {
                            if REQUEST_CHANNEL.sender().try_send(request).is_ok() {
                                REQUEST_EPOCH.fetch_add(1, Ordering::AcqRel);
                                true
                            } else {
                                false
                            }
                        }))
                    }
                    AggregatorQuery::Status => AggregatorResponse::Status(aggregator.snapshot()),
                    AggregatorQuery::LogGet { cursor, max_bytes } => {
                        let mut payload = [0u8; MAX_PAYLOAD_BYTES];
                        let len =
                            resolve_log_get(aggregator.log(), cursor, max_bytes, &mut payload);
                        AggregatorResponse::Log { payload, len }
                    }
                    AggregatorQuery::LogClear => {
                        let next_cursor = aggregator.clear_log();
                        let mut payload = [0u8; MAX_PAYLOAD_BYTES];
                        let len = encode_log_response_header(
                            LogResponseHeader {
                                next_cursor,
                                dropped_log_count: 0,
                                record_count: 0,
                            },
                            &mut payload,
                        )
                        .unwrap_or(0);
                        AggregatorResponse::Log { payload, len }
                    }
                    AggregatorQuery::ProtocolErrors(count) => {
                        aggregator.set_protocol_error_count(count);
                        AggregatorResponse::Ack
                    }
                };
                response_tx.send(response).await;
            }
        }
    }
}

#[cfg(feature = "diagnostic-console")]
async fn query_aggregator(query: AggregatorQuery) -> AggregatorResponse {
    AGGREGATOR_QUERY_CHANNEL.sender().send(query).await;
    AGGREGATOR_RESPONSE_CHANNEL.receiver().receive().await
}

#[cfg(feature = "diagnostic-console")]
#[embassy_executor::task]
async fn diagnostic_task(
    usb_device: esp_hal::peripherals::USB_DEVICE<'static>,
    flash: esp_hal::peripherals::FLASH<'static>,
) {
    use binbook_diagnostic_protocol::Status;
    use binbook_fw::diag::{poll_runtime_command, queue_runtime_response, RuntimeCommand};
    use esp_hal::usb_serial_jtag::UsbSerialJtag;

    let mut usb = UsbSerialJtag::new(usb_device);
    let (mut usb_rx, mut usb_tx) = usb.split();
    let mut diag_serial_state = SerialState::new();
    let mut crash_store =
        CrashStore::new(X4Flash(RefCell::new(esp_storage::FlashStorage::new(flash))));

    loop {
        let mut usb_read_buf = [0u8; 64];
        let n = usb_rx.drain_rx_fifo(&mut usb_read_buf);
        if n > 0 {
            diag_serial_state.feed_rx(&usb_read_buf[..n]);
        }

        while let Ok(committed) = AGGREGATOR_COMPLETION_CHANNEL.receiver().try_receive() {
            let status = match committed.completion.status {
                RuntimeCompletionStatus::Ok => Status::Ok,
                RuntimeCompletionStatus::Error => Status::Error,
            };
            let _ = complete_pending_command(
                &mut diag_serial_state,
                committed.pending,
                status,
                committed.completion.page,
                &[],
            );
        }

        let snapshot = match query_aggregator(AggregatorQuery::Status).await {
            AggregatorResponse::Status(snapshot) => snapshot,
            _ => continue,
        };
        if let Some(command) = poll_runtime_command(&mut diag_serial_state, snapshot) {
            let header = command.header();
            RUNTIME_EVENT_CHANNEL
                .sender()
                .send(RuntimeEvent {
                    timestamp_ms: embassy_time::Instant::now().as_millis(),
                    kind: binbook_fw::runtime_engine::RuntimeEventKind::ProtocolCommand {
                        opcode: header.opcode as u8,
                        sequence: header.sequence,
                    },
                })
                .await;
            match command {
                RuntimeCommand::Immediate {
                    header,
                    status,
                    payload,
                    payload_len,
                } => queue_runtime_response(
                    &mut diag_serial_state,
                    header,
                    status,
                    &payload[..payload_len],
                ),
                RuntimeCommand::LogGet {
                    header,
                    cursor,
                    max_bytes,
                } => {
                    if let AggregatorResponse::Log { payload, len } =
                        query_aggregator(AggregatorQuery::LogGet { cursor, max_bytes }).await
                    {
                        queue_runtime_response(
                            &mut diag_serial_state,
                            header,
                            Status::Ok,
                            &payload[..len],
                        );
                    }
                }
                RuntimeCommand::LogClear { header } => {
                    if let AggregatorResponse::Log { payload, len } =
                        query_aggregator(AggregatorQuery::LogClear).await
                    {
                        queue_runtime_response(
                            &mut diag_serial_state,
                            header,
                            Status::Ok,
                            &payload[..len],
                        );
                    }
                }
                RuntimeCommand::Hardware(pending)
                    if matches!(
                        pending.action,
                        PendingAction::CrashGet | PendingAction::CrashClear
                    ) =>
                {
                    let mut response_status = Status::Ok;
                    let mut response_payload = [0u8; 1 + CRASH_SUMMARY_BYTES];
                    let mut response_payload_len = 0usize;
                    match pending.action {
                        PendingAction::CrashGet => match crash_store.read() {
                            Ok(summary) => {
                                response_payload_len = match summary {
                                    Some(summary) => {
                                        let mut encoded_summary = [0u8; CRASH_SUMMARY_BYTES];
                                        summary.encode(&mut encoded_summary);
                                        encode_crash_response(
                                            Some(&encoded_summary),
                                            &mut response_payload,
                                        )
                                        .unwrap_or(0)
                                    }
                                    None => encode_crash_response(None, &mut response_payload)
                                        .unwrap_or(0),
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
                                RUNTIME_EVENT_CHANNEL.sender().send(RuntimeEvent {
                                    timestamp_ms: embassy_time::Instant::now().as_millis(),
                                    kind: binbook_fw::runtime_engine::RuntimeEventKind::DisplayFailure {
                                        error,
                                        page: snapshot.current_page,
                                    },
                                }).await;
                            }
                        },
                        _ => unreachable!(),
                    }
                    let _ = complete_pending_command(
                        &mut diag_serial_state,
                        pending,
                        response_status,
                        snapshot.current_page,
                        &response_payload[..response_payload_len],
                    );
                }
                RuntimeCommand::Hardware(pending) => {
                    let request = match pending.action {
                        PendingAction::RenderTurn { turn } => DisplayRequest::Turn {
                            turn,
                            completion_sequence: Some(pending.header.sequence),
                        },
                        PendingAction::RenderPage { target_page } => DisplayRequest::Goto {
                            page: target_page,
                            completion_sequence: pending.header.sequence,
                        },
                        PendingAction::DisplayProbe(probe) => DisplayRequest::Probe {
                            kind: match probe {
                                binbook_fw::diag::DisplayProbeKind::FullRefreshCurrent => {
                                    DisplayProbeKind::FullRefreshCurrent
                                }
                                binbook_fw::diag::DisplayProbeKind::ClearWhite => {
                                    DisplayProbeKind::ClearWhite
                                }
                                binbook_fw::diag::DisplayProbeKind::WindowCorners => {
                                    DisplayProbeKind::WindowCorners
                                }
                            },
                            completion_sequence: pending.header.sequence,
                        },
                        _ => unreachable!(),
                    };
                    let enqueued = matches!(
                        query_aggregator(AggregatorQuery::Enqueue { pending, request }).await,
                        AggregatorResponse::Reserve(Ok(()))
                    );
                    if !enqueued {
                        let _ = complete_pending_command(
                            &mut diag_serial_state,
                            pending,
                            Status::Error,
                            snapshot.current_page,
                            &[],
                        );
                    }
                }
            }
        }

        if !diag_serial_state.pending_tx().is_empty() {
            let pending_len = diag_serial_state.pending_tx().len();
            if usb_tx.write(diag_serial_state.pending_tx()).is_ok() {
                diag_serial_state.consume_tx(pending_len);
            }
        }

        let _ = query_aggregator(AggregatorQuery::ProtocolErrors(
            diag_serial_state.protocol_error_count(),
        ))
        .await;
        embassy_time::Timer::after_millis(GRAY_POLL_INTERVAL_MS).await;
    }
}

pub(crate) async fn run(spawner: Spawner, peripherals: RuntimePeripherals) {
    let request_tx_input = REQUEST_CHANNEL.sender();
    let request_rx = REQUEST_CHANNEL.receiver();

    spawner.spawn(
        input_task(
            peripherals.adc1,
            peripherals.gpio1,
            peripherals.gpio2,
            request_tx_input,
        )
        .unwrap(),
    );
    spawner.spawn(
        display_task(
            peripherals.spi2,
            peripherals.gpio8,
            peripherals.gpio10,
            peripherals.gpio21,
            peripherals.gpio4,
            peripherals.gpio5,
            peripherals.gpio6,
            request_rx,
        )
        .unwrap(),
    );

    #[cfg(feature = "diagnostic-console")]
    spawner.spawn(runtime_event_aggregator_task().unwrap());

    #[cfg(feature = "diagnostic-console")]
    spawner.spawn(diagnostic_task(peripherals.usb_device, peripherals.flash).unwrap());
}
