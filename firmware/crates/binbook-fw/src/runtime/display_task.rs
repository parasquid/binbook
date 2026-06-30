use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Sender};
use esp_hal::{
    gpio::{Input, InputConfig, Level, Output, OutputConfig},
    spi::{
        master::{Config as SpiConfig, Spi},
        Mode,
    },
    time::Rate,
};

use binbook_fw::{
    async_refresh::{DisplayProbeKind, DisplayRequest},
    board::{BoardSpiDevice, DisplayDelay},
    runtime_engine::{self, RuntimeEvent, RuntimeEventKind},
};
use xteink_x4_display::{engine::DisplayEngine, events::EventSink};

use crate::{BINBOOK_SCRATCH_BYTES, PROBE_BOOK};

use super::{display_backend::HardwareDisplayBackend, RequestReceiver, RUNTIME_EVENT_CHANNEL};

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
        assert!(
            self.len < self.pending.len(),
            "runtime event buffer overflow"
        );
        let index = (self.head + self.len) % self.pending.len();
        self.pending[index] = Some(event);
        self.len += 1;
    }

    fn push(&mut self, event: RuntimeEvent) {
        if self.len > 0 {
            self.buffer(event);
        } else if let Err(embassy_sync::channel::TrySendError::Full(event)) =
            self.sender.try_send(event)
        {
            self.buffer(event);
        }
    }
}

impl EventSink for RuntimeEventSink {
    fn emit(&mut self, event: xteink_x4_display::events::DisplayEvent) {
        self.push(runtime_engine::map_display_event(event));
    }
}

fn display_request(request: DisplayRequest) -> xteink_x4_display::events::DisplayRequest {
    match request {
        DisplayRequest::Turn {
            turn,
            completion_sequence,
        } => xteink_x4_display::events::DisplayRequest::Turn {
            turn: runtime_engine::to_display_turn(turn),
            sequence: completion_sequence,
        },
        DisplayRequest::Goto {
            page,
            completion_sequence,
        } => xteink_x4_display::events::DisplayRequest::Goto {
            page,
            sequence: completion_sequence,
        },
        DisplayRequest::Probe {
            kind,
            completion_sequence,
        } => xteink_x4_display::events::DisplayRequest::Probe {
            kind: match kind {
                DisplayProbeKind::FullRefreshCurrent => {
                    xteink_x4_display::probes::ProbeKind::FullRefreshCurrent
                }
                DisplayProbeKind::ClearWhite => xteink_x4_display::probes::ProbeKind::ClearWhite,
                DisplayProbeKind::WindowCorners => {
                    xteink_x4_display::probes::ProbeKind::WindowCorners
                }
            },
            sequence: completion_sequence,
        },
    }
}

#[embassy_executor::task]
pub(super) async fn display_task(
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

    let spi = Spi::new(
        spi2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(crate::DISPLAY_SPI_FREQUENCY_MHZ))
            .with_mode(Mode::_0),
    )
    .expect("failed to configure SPI2")
    .with_sck(gpio8)
    .with_mosi(gpio10);

    let cs = Output::new(gpio21, Level::High, OutputConfig::default());
    let dc = Output::new(gpio4, Level::Low, OutputConfig::default());
    let rst = Output::new(gpio5, Level::High, OutputConfig::default());
    let busy = Input::new(gpio6, InputConfig::default());
    let mut scratch = [0u8; BINBOOK_SCRATCH_BYTES];
    let book = binbook_core::Book::open(binbook_core::SliceSource::new(PROBE_BOOK), &mut scratch)
        .expect("failed to open embedded BinBook");
    let page_count = book.page_count();
    let mut backend = HardwareDisplayBackend {
        display: xteink_x4_display::panel::X4Panel::new(
            BoardSpiDevice::new(spi, cs),
            dc,
            rst,
            busy,
        ),
        book,
        delay: DisplayDelay,
        compressed: [0; 768],
        decoded: [0; 300],
        black: [0; 100],
        red: [0; 100],
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
                if let DisplayRequest::Turn {
                    turn,
                    completion_sequence,
                } = request
                {
                    events.push(RuntimeEvent {
                        timestamp_ms: now_ms,
                        kind: RuntimeEventKind::TurnQueued {
                            sequence: completion_sequence,
                            turn,
                        },
                    });
                }
                let _ = engine
                    .request(display_request(request), &mut backend, &mut events, now_ms)
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
