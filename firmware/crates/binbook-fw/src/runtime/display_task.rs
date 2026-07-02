use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Sender};
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig};

use binbook_fw::{
    async_refresh::{DisplayProbeKind, DisplayRequest},
    board::{DisplayDelay, FreqManagedSpiDevice, SharedSpi2},
    menu::{render_menu, MenuNames, MenuState, Mode},
    runtime_engine::{self, RuntimeEvent, RuntimeEventKind},
    xteink_x4_display::{
        framebuffer::Gray2Framebuffer,
        ui_render::{render_ui_bw, render_ui_gray_overlay},
    },
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

/// Convert a binbook-fw `DisplayRequest` to the xteink-x4-display engine
/// request. Menu intents are mapped to `Goto { page: 0 }` as a no-op
/// placeholder — they are consumed by the display task's own mode logic
/// (handled before reaching `engine.request()`).
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
        // Menu intents are handled by the display task before engine request;
        // the placeholder cast here satisfies exhaustive match.
        DisplayRequest::MenuNext
        | DisplayRequest::MenuPrev
        | DisplayRequest::MenuSelect
        | DisplayRequest::MenuBack => xteink_x4_display::events::DisplayRequest::Goto {
            page: 0,
            sequence: 0,
        },
    }
}

fn target_page_for_request(engine: &DisplayEngine, request: DisplayRequest) -> Option<u32> {
    match request {
        DisplayRequest::Turn { turn, .. } => {
            Some(engine.target_for(runtime_engine::to_display_turn(turn)))
        }
        DisplayRequest::Goto { page, .. } => Some(page),
        DisplayRequest::Probe { .. }
        | DisplayRequest::MenuNext
        | DisplayRequest::MenuPrev
        | DisplayRequest::MenuSelect
        | DisplayRequest::MenuBack => None,
    }
}

#[embassy_executor::task]
pub(super) async fn display_task(
    shared_spi2: &'static SharedSpi2,
    gpio21: esp_hal::peripherals::GPIO21<'static>,
    gpio4: esp_hal::peripherals::GPIO4<'static>,
    gpio5: esp_hal::peripherals::GPIO5<'static>,
    gpio6: esp_hal::peripherals::GPIO6<'static>,
    request_rx: RequestReceiver,
) {
    use embassy_futures::select::{select, Either};

    let cs = Output::new(gpio21, Level::High, OutputConfig::default());
    let spi_device = FreqManagedSpiDevice::new(
        shared_spi2,
        cs,
        crate::DISPLAY_SPI_FREQUENCY_MHZ * 1_000_000,
    );
    let dc = Output::new(gpio4, Level::Low, OutputConfig::default());
    let rst = Output::new(gpio5, Level::High, OutputConfig::default());
    let busy = Input::new(gpio6, InputConfig::default());
    let mut scratch = [0u8; BINBOOK_SCRATCH_BYTES];
    let book = binbook_core::Book::open(binbook_core::SliceSource::new(PROBE_BOOK), &mut scratch)
        .expect("failed to open embedded BinBook");
    let page_count = book.page_count();
    let mut backend = HardwareDisplayBackend {
        display: xteink_x4_display::panel::X4Panel::new(spi_device, dc, rst, busy),
        book,
        delay: DisplayDelay,
        compressed: [0; 768],
        decoded: [0; 300],
        black: [0; 100],
        red: [0; 100],
    };
    let mut engine = DisplayEngine::new(page_count);
    let mut events = RuntimeEventSink::new(RUNTIME_EVENT_CHANNEL.sender());

    let mut menu_state = MenuState::new(0);
    let mut menu_fb = Gray2Framebuffer::new();
    #[cfg(feature = "sd-storage")]
    let menu_names: MenuNames = super::MENU_BOOK_NAMES.lock().await.clone();
    #[cfg(not(feature = "sd-storage"))]
    let menu_names: MenuNames = binbook_fw::heapless::Vec::new();

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
                let request_kind = runtime_engine::RuntimeRequestKind::from_request(&request);
                let request_sequence = runtime_engine::request_sequence(&request);
                events.push(RuntimeEvent {
                    timestamp_ms: now_ms,
                    kind: RuntimeEventKind::RequestReceive {
                        kind: request_kind,
                        sequence: request_sequence,
                        queue_age_ms: runtime_engine::queue_age_ms(now_ms, None),
                    },
                });
                match request {
                    DisplayRequest::MenuNext | DisplayRequest::MenuPrev => {
                        engine.cancel_overlay();
                        let action = if matches!(request, DisplayRequest::MenuPrev) {
                            binbook_fw::menu::MenuAction::Previous
                        } else {
                            binbook_fw::menu::MenuAction::Next
                        };
                        let _ = menu_state.transition(action);
                        render_menu(&mut menu_fb, &menu_state, &menu_names);
                        let _ =
                            render_ui_bw(&mut backend.display, &menu_fb, &mut backend.delay).await;
                        let _ = render_ui_gray_overlay(
                            &mut backend.display,
                            &menu_fb,
                            &mut backend.delay,
                        )
                        .await;
                        events.push(RuntimeEvent {
                            timestamp_ms: now_ms,
                            kind: RuntimeEventKind::MenuTransition,
                        });
                    }
                    DisplayRequest::MenuBack => {
                        engine.cancel_overlay();
                        render_menu(&mut menu_fb, &menu_state, &menu_names);
                        let _ =
                            render_ui_bw(&mut backend.display, &menu_fb, &mut backend.delay).await;
                        let _ = render_ui_gray_overlay(
                            &mut backend.display,
                            &menu_fb,
                            &mut backend.delay,
                        )
                        .await;
                        super::DISPLAY_MODE.store(
                            binbook_fw::async_refresh::MODE_MENU,
                            portable_atomic::Ordering::Relaxed,
                        );
                        events.push(RuntimeEvent {
                            timestamp_ms: now_ms,
                            kind: RuntimeEventKind::ModeChange { mode: Mode::Menu },
                        });
                    }
                    DisplayRequest::MenuSelect => {
                        let selected_index = menu_state.selected();
                        if selected_index < menu_names.len() {
                            engine.cancel_overlay();
                            super::DISPLAY_MODE.store(
                                binbook_fw::async_refresh::MODE_READING,
                                portable_atomic::Ordering::Relaxed,
                            );
                            events.push(RuntimeEvent {
                                timestamp_ms: now_ms,
                                kind: RuntimeEventKind::ModeChange {
                                    mode: Mode::Reading,
                                },
                            });
                            events.push(RuntimeEvent {
                                timestamp_ms: now_ms,
                                kind: RuntimeEventKind::BookOpen {
                                    name_index: selected_index,
                                },
                            });
                        }
                    }
                    DisplayRequest::Turn {
                        turn,
                        completion_sequence,
                    } => {
                        events.push(RuntimeEvent {
                            timestamp_ms: now_ms,
                            kind: RuntimeEventKind::TurnQueued {
                                sequence: completion_sequence,
                                turn,
                            },
                        });
                    }
                    DisplayRequest::Goto { .. } | DisplayRequest::Probe { .. } => {}
                }
                let current_page = engine.current_page();
                let target_page = target_page_for_request(&engine, request);
                let start_ms = embassy_time::Instant::now().as_millis();
                events.push(RuntimeEvent {
                    timestamp_ms: start_ms,
                    kind: RuntimeEventKind::DisplayRequestStart {
                        kind: request_kind,
                        current_page,
                        target_page,
                    },
                });
                let result = engine
                    .request(display_request(request), &mut backend, &mut events, now_ms)
                    .await;
                let end_ms = embassy_time::Instant::now().as_millis();
                events.push(RuntimeEvent {
                    timestamp_ms: end_ms,
                    kind: RuntimeEventKind::DisplayRequestEnd {
                        kind: request_kind,
                        duration_ms: end_ms.saturating_sub(start_ms).min(u32::MAX as u64) as u32,
                        status: match result.status {
                            xteink_x4_display::events::CompletionStatus::Ok => {
                                runtime_engine::RuntimeCompletionStatus::Ok
                            }
                            xteink_x4_display::events::CompletionStatus::Error => {
                                runtime_engine::RuntimeCompletionStatus::Error
                            }
                        },
                    },
                });
                events.flush().await;
            }
            Either::Second(_) => {
                let _ = engine.advance(&mut backend, &mut events, now_ms).await;
                events.flush().await;
            }
        }
    }
}
