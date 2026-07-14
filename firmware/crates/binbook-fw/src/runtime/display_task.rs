use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Sender};
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig};

#[cfg(feature = "sd-storage")]
use binbook_fw::menu::{render_library_status, render_progress_not_saved, render_sleep_cancelled};
use binbook_fw::{
    async_refresh::{DisplayProbeKind, DisplayRequest},
    board::{DisplayDelay, FreqManagedSpiDevice, SharedSpi2},
    menu::{render_book_open_error, render_menu, MenuState, Mode},
    runtime_engine::{self, RuntimeEvent, RuntimeEventKind},
    xteink_x4_display::{
        framebuffer::Gray2Framebuffer,
        ui_render::{render_ui_bw, render_ui_transition},
    },
};
use xteink_x4_display::{engine::DisplayEngine, events::EventSink};

#[cfg(not(feature = "sd-storage"))]
use binbook_fw::menu::MenuNames;

#[cfg(any(feature = "sd-storage", feature = "diagnostic-console"))]
use crate::BINBOOK_SCRATCH_BYTES;
#[cfg(all(feature = "diagnostic-console", not(feature = "sd-storage")))]
use crate::PROBE_BOOK;
#[cfg(any(feature = "sd-storage", feature = "diagnostic-console"))]
use static_cell::StaticCell;

use super::{
    display_backend::HardwareDisplayBackend, PowerAckSender, PowerRequestReceiver, RequestReceiver,
    RUNTIME_EVENT_CHANNEL,
};

#[cfg(feature = "sd-storage")]
type DisplayFilesystem = super::RuntimeSdFilesystem;
#[cfg(not(feature = "sd-storage"))]
type DisplayFilesystem = binbook_fw::storage_access::NoFilesystem;

const GRAY_POLL_INTERVAL_MS: u64 = 10;
#[cfg(feature = "sd-storage")]
const RETURN_STAGE_SAVE: u8 = 1;
#[cfg(feature = "sd-storage")]
const RETURN_STAGE_SCAN: u8 = 2;
const RETURN_STAGE_COMPOSE: u8 = 3;
const RETURN_STAGE_PANEL: u8 = 4;
const RETURN_PHASE_START: u8 = 0;
const RETURN_PHASE_END: u8 = 1;

#[cfg(any(feature = "sd-storage", feature = "diagnostic-console"))]
static BINBOOK_SCRATCH: StaticCell<[u8; BINBOOK_SCRATCH_BYTES]> = StaticCell::new();

#[cfg(feature = "sd-storage")]
fn open_sd_book(
    sd_fs: binbook_fw::storage_access::SdHandle<'static, super::RuntimeSdFilesystem>,
    name: &str,
    scratch: &mut [u8],
) -> Result<
    (
        binbook_core::Book<
            binbook_fw::storage_access::RuntimeBookSource<'static, super::RuntimeSdFilesystem>,
        >,
        binbook_core::RenditionIdentity,
    ),
    u8,
> {
    let source = binbook_fw::storage_access::SdBookSource::new(sd_fs, name).map_err(|_| 1)?;
    let mut book = binbook_core::Book::open(
        binbook_fw::storage_access::RuntimeBookSource::Sd(source),
        scratch,
    )
    .map_err(|error| match error {
        binbook_core::Error::Source(_) => 2,
        binbook_core::Error::Format(format) => 10 + format_error_code(format),
        binbook_core::Error::BufferTooSmall { .. } => 5,
    })?;
    let identity = book
        .rendition_identity(scratch)
        .map_err(|error| match error {
            binbook_core::Error::Source(_) => 3,
            binbook_core::Error::Format(_) => 6,
            binbook_core::Error::BufferTooSmall { .. } => 7,
        })?;
    Ok((book, identity))
}

#[cfg(feature = "sd-storage")]
fn format_error_code(error: binbook_core::FormatError) -> u8 {
    use binbook_core::FormatError;
    match error {
        FormatError::InvalidMagic => 0,
        FormatError::UnsupportedVersion => 1,
        FormatError::InvalidHeader => 2,
        FormatError::FileOutOfBounds => 3,
        FormatError::MissingSection(_) => 4,
        FormatError::DuplicateSection(_) => 5,
        FormatError::InvalidSection => 6,
        _ => 31,
    }
}

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
        DisplayRequest::SetRefreshMode {
            mode,
            completion_sequence,
        } => xteink_x4_display::events::DisplayRequest::SetRefreshMode {
            mode,
            sequence: Some(completion_sequence),
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
                DisplayProbeKind::PartialWindowBusyMatrix => {
                    xteink_x4_display::probes::ProbeKind::PartialWindowBusyMatrix
                }
                DisplayProbeKind::CustomLutFastA => {
                    xteink_x4_display::probes::ProbeKind::CustomLutFastA
                }
                DisplayProbeKind::CustomLutFastB => {
                    xteink_x4_display::probes::ProbeKind::CustomLutFastB
                }
            },
            sequence: completion_sequence,
        },
        // Menu intents are handled by the display task before engine request;
        // the placeholder cast here satisfies exhaustive match.
        DisplayRequest::MenuNext {
            completion_sequence,
        }
        | DisplayRequest::MenuPrev {
            completion_sequence,
        }
        | DisplayRequest::MenuSelect {
            completion_sequence,
        }
        | DisplayRequest::MenuBack {
            completion_sequence,
        } => xteink_x4_display::events::DisplayRequest::Goto {
            page: 0,
            sequence: completion_sequence.unwrap_or(0),
        },
    }
}

fn target_page_for_request(engine: &DisplayEngine, request: DisplayRequest) -> Option<u32> {
    match request {
        DisplayRequest::Turn { turn, .. } => {
            Some(engine.target_for(runtime_engine::to_display_turn(turn)))
        }
        DisplayRequest::Goto { page, .. } => Some(page),
        DisplayRequest::SetRefreshMode { .. } => Some(engine.current_page()),
        DisplayRequest::Probe { .. }
        | DisplayRequest::MenuNext { .. }
        | DisplayRequest::MenuPrev { .. }
        | DisplayRequest::MenuSelect { .. }
        | DisplayRequest::MenuBack { .. } => None,
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
    power_rx: PowerRequestReceiver,
    power_ack_tx: PowerAckSender,
    #[cfg(feature = "sd-storage")] sd_fs: binbook_fw::storage_access::SdHandle<
        'static,
        super::RuntimeSdFilesystem,
    >,
    #[cfg(feature = "sd-storage")] library: &'static mut binbook_fw::library::Library,
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
    #[cfg(any(feature = "sd-storage", feature = "diagnostic-console"))]
    let scratch = BINBOOK_SCRATCH.init([0u8; BINBOOK_SCRATCH_BYTES]);
    #[cfg(all(feature = "diagnostic-console", not(feature = "sd-storage")))]
    let initial_book = Some(
        binbook_core::Book::open(
            binbook_fw::storage_access::RuntimeBookSource::<DisplayFilesystem>::Embedded(
                binbook_core::SliceSource::new(PROBE_BOOK),
            ),
            &mut scratch[..],
        )
        .expect("failed to open embedded diagnostic BinBook"),
    );
    #[cfg(not(all(feature = "diagnostic-console", not(feature = "sd-storage"))))]
    let initial_book = None;
    let page_count = initial_book
        .as_ref()
        .map_or(1, binbook_core::Book::page_count);
    let mut backend: HardwareDisplayBackend<'_, DisplayFilesystem, _, _, _, _> =
        HardwareDisplayBackend {
            display: xteink_x4_display::panel::X4Panel::new(spi_device, dc, rst, busy),
            book: initial_book,
            delay: DisplayDelay,
            compressed: [0; 4096],
            decoded: [0; 300],
            black: [0; 100],
            red: [0; 100],
        };
    let mut engine = DisplayEngine::new_with_refresh_mode(
        page_count,
        binbook_fw::async_refresh::STARTUP_PAGE_REFRESH_MODE,
    );
    let mut events = RuntimeEventSink::new(RUNTIME_EVENT_CHANNEL.sender());

    let mut menu_fb = Gray2Framebuffer::new();
    #[cfg(not(feature = "sd-storage"))]
    let menu_names: MenuNames = binbook_fw::heapless::Vec::new();
    #[cfg(feature = "sd-storage")]
    let mut menu_state = MenuState::new(library.entries().len());
    #[cfg(not(feature = "sd-storage"))]
    let mut menu_state = MenuState::new(menu_names.len());

    #[cfg(feature = "sd-storage")]
    events.push(RuntimeEvent {
        timestamp_ms: embassy_time::Instant::now().as_millis(),
        kind: RuntimeEventKind::LibraryScan {
            status: library.status().code(),
            book_count: library.entries().len() as u16,
        },
    });

    #[cfg(feature = "sd-storage")]
    let (reader_state, state_buffer, mut state_slots) = {
        use static_cell::StaticCell;
        static STATE: StaticCell<binbook_fw::reader_state::ReaderState> = StaticCell::new();
        static BUFFER: StaticCell<[u8; binbook_fw::reader_state::MAX_STATE_BYTES]> =
            StaticCell::new();
        let buffer = BUFFER.init([0; binbook_fw::reader_state::MAX_STATE_BYTES]);
        let mut slots = binbook_fw::storage_access::StateSlots::new(sd_fs);
        let loaded = slots.load(buffer);
        (STATE.init(loaded.state), buffer, slots)
    };
    #[cfg(feature = "sd-storage")]
    let refresh_library = {
        use static_cell::StaticCell;
        static REFRESH_LIBRARY: StaticCell<binbook_fw::library::Library> = StaticCell::new();
        REFRESH_LIBRARY.init(binbook_fw::library::Library::empty())
    };

    #[cfg(feature = "sd-storage")]
    {
        let before_prune = reader_state.clone();
        if library.prune_state(reader_state) && *reader_state != before_prune {
            let removed_count = before_prune
                .records()
                .len()
                .saturating_sub(reader_state.records().len())
                as u16;
            if state_slots.save(reader_state, state_buffer).is_ok() {
                events.push(RuntimeEvent {
                    timestamp_ms: embassy_time::Instant::now().as_millis(),
                    kind: RuntimeEventKind::ReaderStatePruned {
                        removed_count,
                        remaining_count: reader_state.records().len() as u16,
                    },
                });
                events.push(RuntimeEvent {
                    timestamp_ms: embassy_time::Instant::now().as_millis(),
                    kind: RuntimeEventKind::ReaderStateSaved {
                        generation: reader_state.generation,
                        record_count: reader_state.records().len() as u16,
                    },
                });
            }
        }
    }

    #[cfg(feature = "sd-storage")]
    if let Some(index) = library.index_for_filename_hash(reader_state.selected_filename_hash) {
        menu_state.restore_selected(index);
    }

    #[cfg(feature = "sd-storage")]
    let mut restored_reading = false;
    #[cfg(feature = "sd-storage")]
    if reader_state.saved_mode == binbook_fw::reader_state::SavedMode::Reading {
        if let Some((index, entry)) = library
            .entries()
            .iter()
            .enumerate()
            .find(|(_, entry)| entry.filename_hash == reader_state.active_filename_hash)
        {
            if let Ok((book, identity)) = open_sd_book(sd_fs, entry.name.as_str(), &mut scratch[..])
            {
                if identity.rendition_hash == reader_state.active_rendition_hash {
                    let page_count = book.page_count();
                    let page = reader_state
                        .lookup_page(
                            reader_state.active_filename_hash,
                            &reader_state.active_rendition_hash,
                        )
                        .unwrap_or(0)
                        .min(page_count.saturating_sub(1));
                    backend.book = Some(book);
                    engine = DisplayEngine::new_with_refresh_mode(
                        page_count,
                        binbook_fw::async_refresh::STARTUP_PAGE_REFRESH_MODE,
                    );
                    if engine
                        .initialize_at(
                            page,
                            &mut backend,
                            &mut events,
                            embassy_time::Instant::now().as_millis(),
                        )
                        .await
                        .is_none()
                    {
                        menu_state.restore_selected(index);
                        restored_reading = true;
                        super::DISPLAY_MODE.store(
                            binbook_fw::async_refresh::MODE_READING,
                            portable_atomic::Ordering::Relaxed,
                        );
                        events.push(RuntimeEvent {
                            timestamp_ms: embassy_time::Instant::now().as_millis(),
                            kind: RuntimeEventKind::BookOpen {
                                filename_hash: reader_state.active_filename_hash,
                                rendition_hash_prefix: u32::from_le_bytes(
                                    reader_state.active_rendition_hash[..4]
                                        .try_into()
                                        .expect("four-byte rendition prefix"),
                                ),
                                page_count,
                                page,
                            },
                        });
                    }
                }
            }
        }
    }

    #[cfg(not(feature = "sd-storage"))]
    if backend.book.is_some() {
        let _ = engine
            .initialize(
                &mut backend,
                &mut events,
                embassy_time::Instant::now().as_millis(),
            )
            .await;
        events.flush().await;
    }

    #[cfg(feature = "sd-storage")]
    let show_menu = !restored_reading;
    #[cfg(not(feature = "sd-storage"))]
    let show_menu = super::DISPLAY_MODE.load(portable_atomic::Ordering::Relaxed)
        == binbook_fw::async_refresh::MODE_MENU;
    if show_menu {
        super::DISPLAY_MODE.store(
            binbook_fw::async_refresh::MODE_MENU,
            portable_atomic::Ordering::Relaxed,
        );
        engine.cancel_overlay();
        #[cfg(feature = "sd-storage")]
        let status = binbook_fw::library::LibraryStatus::from_code(
            super::LIBRARY_STATUS.load(portable_atomic::Ordering::Relaxed),
        );
        #[cfg(feature = "sd-storage")]
        if status != binbook_fw::library::LibraryStatus::Ready {
            render_library_status(&mut menu_fb, status);
        } else {
            render_menu(&mut menu_fb, &menu_state, library);
        }
        #[cfg(not(feature = "sd-storage"))]
        render_menu(&mut menu_fb, &menu_state, &menu_names);
        let _ = render_ui_bw(&mut backend.display, &menu_fb, &mut backend.delay).await;
    }

    loop {
        let now_ms = embassy_time::Instant::now().as_millis();
        match select(
            power_rx.receive(),
            select(
                request_rx.receive(),
                embassy_time::Timer::after_millis(GRAY_POLL_INTERVAL_MS),
            ),
        )
        .await
        {
            Either::First(power_request) => {
                if power_request == binbook_fw::power::PowerRequest::FullRefresh {
                    if super::DISPLAY_MODE.load(portable_atomic::Ordering::Relaxed)
                        == binbook_fw::async_refresh::MODE_MENU
                    {
                        let _ =
                            render_ui_bw(&mut backend.display, &menu_fb, &mut backend.delay).await;
                    } else {
                        let _ = engine
                            .request(
                                xteink_x4_display::events::DisplayRequest::Probe {
                                    kind: xteink_x4_display::probes::ProbeKind::FullRefreshCurrent,
                                    sequence: u16::MAX,
                                },
                                &mut backend,
                                &mut events,
                                now_ms,
                            )
                            .await;
                    }
                    events.flush().await;
                    continue;
                }
                #[cfg(feature = "sd-storage")]
                {
                    if reader_state.saved_mode == binbook_fw::reader_state::SavedMode::Reading {
                        let _ =
                            reader_state.set_progress(binbook_fw::reader_state::ProgressRecord {
                                filename_hash: reader_state.active_filename_hash,
                                rendition_hash: reader_state.active_rendition_hash,
                                page: engine.current_page(),
                            });
                    }
                    if let Err(error) = state_slots.save(reader_state, state_buffer) {
                        let outcome =
                            binbook_fw::power::SleepPreparationOutcome::after_state_save(false);
                        events.push(RuntimeEvent {
                            timestamp_ms: now_ms,
                            kind: RuntimeEventKind::ReaderStateSaveFailed {
                                stage: error.stage_code(),
                            },
                        });
                        events.push(RuntimeEvent {
                            timestamp_ms: now_ms,
                            kind: RuntimeEventKind::SleepPrepared {
                                status: outcome.status_code(),
                            },
                        });
                        events.flush().await;
                        render_sleep_cancelled(&mut menu_fb);
                        let _ =
                            render_ui_bw(&mut backend.display, &menu_fb, &mut backend.delay).await;
                        power_ack_tx.send(outcome.ack()).await;
                        continue;
                    }
                    events.push(RuntimeEvent {
                        timestamp_ms: now_ms,
                        kind: RuntimeEventKind::ReaderStateSaved {
                            generation: reader_state.generation,
                            record_count: reader_state.records().len() as u16,
                        },
                    });
                }
                let outcome = binbook_fw::power::SleepPreparationOutcome::after_panel_sleep(
                    backend.display.deep_sleep().is_ok(),
                );
                events.push(RuntimeEvent {
                    timestamp_ms: now_ms,
                    kind: RuntimeEventKind::SleepPrepared {
                        status: outcome.status_code(),
                    },
                });
                events.flush().await;
                power_ack_tx.send(outcome.ack()).await;
            }
            Either::Second(Either::First(request)) => {
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
                match request {
                    DisplayRequest::MenuNext { .. } | DisplayRequest::MenuPrev { .. } => {
                        engine.cancel_overlay();
                        let action = if matches!(request, DisplayRequest::MenuPrev { .. }) {
                            binbook_fw::menu::MenuAction::Previous
                        } else {
                            binbook_fw::menu::MenuAction::Next
                        };
                        let transition = render_ui_transition(
                            &mut backend.display,
                            &mut menu_fb,
                            |target| {
                                let _ = menu_state.transition(action);
                                #[cfg(feature = "sd-storage")]
                                {
                                    let status = binbook_fw::library::LibraryStatus::from_code(
                                        super::LIBRARY_STATUS
                                            .load(portable_atomic::Ordering::Relaxed),
                                    );
                                    if status != binbook_fw::library::LibraryStatus::Ready {
                                        render_library_status(target, status);
                                    } else {
                                        render_menu(target, &menu_state, library);
                                    }
                                }
                                #[cfg(not(feature = "sd-storage"))]
                                render_menu(target, &menu_state, &menu_names);
                            },
                            &mut backend.delay,
                        )
                        .await;
                        if transition.is_err() {
                            let _ =
                                render_ui_bw(&mut backend.display, &menu_fb, &mut backend.delay)
                                    .await;
                        }
                        events.push(RuntimeEvent {
                            timestamp_ms: now_ms,
                            kind: RuntimeEventKind::MenuTransition {
                                selected: menu_state.selected(),
                            },
                        });
                    }
                    DisplayRequest::MenuBack { .. } => {
                        engine.cancel_overlay();
                        #[cfg(feature = "sd-storage")]
                        {
                            reader_state.set_menu();
                        }
                        let compose_start = embassy_time::Instant::now().as_millis();
                        events.push(RuntimeEvent {
                            timestamp_ms: compose_start,
                            kind: RuntimeEventKind::ReaderReturnStage {
                                stage: RETURN_STAGE_COMPOSE,
                                phase: RETURN_PHASE_START,
                                elapsed_ms: 0,
                            },
                        });
                        #[cfg(feature = "sd-storage")]
                        if library.status() != binbook_fw::library::LibraryStatus::Ready {
                            render_library_status(&mut menu_fb, library.status());
                        } else {
                            render_menu(&mut menu_fb, &menu_state, library);
                        }
                        #[cfg(not(feature = "sd-storage"))]
                        render_menu(&mut menu_fb, &menu_state, &menu_names);
                        let compose_end = embassy_time::Instant::now().as_millis();
                        events.push(RuntimeEvent {
                            timestamp_ms: compose_end,
                            kind: RuntimeEventKind::ReaderReturnStage {
                                stage: RETURN_STAGE_COMPOSE,
                                phase: RETURN_PHASE_END,
                                elapsed_ms: compose_end
                                    .saturating_sub(compose_start)
                                    .min(u32::MAX as u64)
                                    as u32,
                            },
                        });
                        let panel_start = embassy_time::Instant::now().as_millis();
                        events.push(RuntimeEvent {
                            timestamp_ms: panel_start,
                            kind: RuntimeEventKind::ReaderReturnStage {
                                stage: RETURN_STAGE_PANEL,
                                phase: RETURN_PHASE_START,
                                elapsed_ms: 0,
                            },
                        });
                        let transition = backend
                            .render_book_to_ui(engine.current_page(), &menu_fb)
                            .await;
                        if transition.is_err() {
                            let _ =
                                render_ui_bw(&mut backend.display, &menu_fb, &mut backend.delay)
                                    .await;
                        }
                        #[cfg(feature = "sd-storage")]
                        {
                            backend.book = None;
                        }
                        let panel_end = embassy_time::Instant::now().as_millis();
                        events.push(RuntimeEvent {
                            timestamp_ms: panel_end,
                            kind: RuntimeEventKind::ReaderReturnStage {
                                stage: RETURN_STAGE_PANEL,
                                phase: RETURN_PHASE_END,
                                elapsed_ms: panel_end
                                    .saturating_sub(panel_start)
                                    .min(u32::MAX as u64)
                                    as u32,
                            },
                        });
                        super::DISPLAY_MODE.store(
                            binbook_fw::async_refresh::MODE_MENU,
                            portable_atomic::Ordering::Relaxed,
                        );
                        events.push(RuntimeEvent {
                            timestamp_ms: now_ms,
                            kind: RuntimeEventKind::ModeChange { mode: Mode::Menu },
                        });
                        #[cfg(feature = "sd-storage")]
                        {
                            let stage_start = embassy_time::Instant::now().as_millis();
                            events.push(RuntimeEvent {
                                timestamp_ms: stage_start,
                                kind: RuntimeEventKind::ReaderReturnStage {
                                    stage: RETURN_STAGE_SAVE,
                                    phase: RETURN_PHASE_START,
                                    elapsed_ms: 0,
                                },
                            });
                            let save_failed = match state_slots.save(reader_state, state_buffer) {
                                Ok(()) => {
                                    events.push(RuntimeEvent {
                                        timestamp_ms: embassy_time::Instant::now().as_millis(),
                                        kind: RuntimeEventKind::ReaderStateSaved {
                                            generation: reader_state.generation,
                                            record_count: reader_state.records().len() as u16,
                                        },
                                    });
                                    false
                                }
                                Err(error) => {
                                    events.push(RuntimeEvent {
                                        timestamp_ms: embassy_time::Instant::now().as_millis(),
                                        kind: RuntimeEventKind::ReaderStateSaveFailed {
                                            stage: error.stage_code(),
                                        },
                                    });
                                    true
                                }
                            };
                            let stage_end = embassy_time::Instant::now().as_millis();
                            events.push(RuntimeEvent {
                                timestamp_ms: stage_end,
                                kind: RuntimeEventKind::ReaderReturnStage {
                                    stage: RETURN_STAGE_SAVE,
                                    phase: RETURN_PHASE_END,
                                    elapsed_ms: stage_end
                                        .saturating_sub(stage_start)
                                        .min(u32::MAX as u64)
                                        as u32,
                                },
                            });
                            if save_failed {
                                render_progress_not_saved(&mut menu_fb);
                                let _ = render_ui_bw(
                                    &mut backend.display,
                                    &menu_fb,
                                    &mut backend.delay,
                                )
                                .await;
                            }

                            let selected_hash = reader_state.selected_filename_hash;
                            let mut scan_fs = sd_fs;
                            let stage_start = embassy_time::Instant::now().as_millis();
                            events.push(RuntimeEvent {
                                timestamp_ms: stage_start,
                                kind: RuntimeEventKind::ReaderReturnStage {
                                    stage: RETURN_STAGE_SCAN,
                                    phase: RETURN_PHASE_START,
                                    elapsed_ms: 0,
                                },
                            });
                            refresh_library.scan_in_place(&mut scan_fs, |error| match error {
                                binbook_fw::storage::SdError::NoCard => {
                                    binbook_fw::library::ScanErrorKind::NoCard
                                }
                                _ => binbook_fw::library::ScanErrorKind::CardError,
                            });
                            let scanned_count = refresh_library.entries().len() as u16;
                            let outcome = library.apply_refresh_in_place(refresh_library);
                            events.push(RuntimeEvent {
                                timestamp_ms: embassy_time::Instant::now().as_millis(),
                                kind: RuntimeEventKind::LibraryScan {
                                    status: outcome.scan_status.code(),
                                    book_count: scanned_count,
                                },
                            });
                            let stage_end = embassy_time::Instant::now().as_millis();
                            events.push(RuntimeEvent {
                                timestamp_ms: stage_end,
                                kind: RuntimeEventKind::ReaderReturnStage {
                                    stage: RETURN_STAGE_SCAN,
                                    phase: RETURN_PHASE_END,
                                    elapsed_ms: stage_end
                                        .saturating_sub(stage_start)
                                        .min(u32::MAX as u64)
                                        as u32,
                                },
                            });

                            if outcome.adopted {
                                if outcome.changed {
                                    let mut index_store =
                                        binbook_fw::storage_access::LibraryIndexStore::new(sd_fs);
                                    let _ = index_store.save(library);
                                }
                                menu_state.reset(library.entries().len());
                                if let Some(index) = library.index_for_filename_hash(selected_hash)
                                {
                                    menu_state.restore_selected(index);
                                }
                                super::LIBRARY_STATUS.store(
                                    library.status().code(),
                                    portable_atomic::Ordering::Relaxed,
                                );
                                let before_prune = reader_state.clone();
                                if library.prune_state(reader_state)
                                    && *reader_state != before_prune
                                {
                                    let removed_count = before_prune
                                        .records()
                                        .len()
                                        .saturating_sub(reader_state.records().len())
                                        as u16;
                                    if state_slots.save(reader_state, state_buffer).is_ok() {
                                        events.push(RuntimeEvent {
                                            timestamp_ms: embassy_time::Instant::now().as_millis(),
                                            kind: RuntimeEventKind::ReaderStatePruned {
                                                removed_count,
                                                remaining_count: reader_state.records().len()
                                                    as u16,
                                            },
                                        });
                                    }
                                }
                                if outcome.changed {
                                    if library.status() == binbook_fw::library::LibraryStatus::Ready
                                    {
                                        render_menu(&mut menu_fb, &menu_state, library);
                                    } else {
                                        render_library_status(&mut menu_fb, library.status());
                                    }
                                    let _ = render_ui_bw(
                                        &mut backend.display,
                                        &menu_fb,
                                        &mut backend.delay,
                                    )
                                    .await;
                                }
                            }
                        }
                    }
                    DisplayRequest::MenuSelect { .. } => {
                        #[cfg(feature = "sd-storage")]
                        let can_open = {
                            let status = binbook_fw::library::LibraryStatus::from_code(
                                super::LIBRARY_STATUS.load(portable_atomic::Ordering::Relaxed),
                            );
                            if status != binbook_fw::library::LibraryStatus::Ready {
                                let mut scan_fs = sd_fs;
                                library.scan_in_place(&mut scan_fs, |error| match error {
                                    binbook_fw::storage::SdError::NoCard => {
                                        binbook_fw::library::ScanErrorKind::NoCard
                                    }
                                    _ => binbook_fw::library::ScanErrorKind::CardError,
                                });
                                events.push(RuntimeEvent {
                                    timestamp_ms: now_ms,
                                    kind: RuntimeEventKind::LibraryScan {
                                        status: library.status().code(),
                                        book_count: library.entries().len() as u16,
                                    },
                                });
                                menu_state.reset(library.entries().len());
                                super::LIBRARY_STATUS.store(
                                    library.status().code(),
                                    portable_atomic::Ordering::Relaxed,
                                );
                                if library.status() == binbook_fw::library::LibraryStatus::Ready {
                                    render_menu(&mut menu_fb, &menu_state, library);
                                } else {
                                    render_library_status(&mut menu_fb, library.status());
                                }
                                let _ = render_ui_bw(
                                    &mut backend.display,
                                    &menu_fb,
                                    &mut backend.delay,
                                )
                                .await;
                                false
                            } else {
                                true
                            }
                        };
                        #[cfg(not(feature = "sd-storage"))]
                        let can_open = true;
                        let selected_index = menu_state.selected();
                        #[cfg(feature = "sd-storage")]
                        let selected_count = library.entries().len();
                        #[cfg(not(feature = "sd-storage"))]
                        let selected_count = menu_names.len();
                        if can_open && selected_index < selected_count {
                            #[cfg(feature = "sd-storage")]
                            let opened = {
                                let name = library.entries()[selected_index].name.clone();
                                open_sd_book(sd_fs, name.as_str(), &mut scratch[..])
                            };
                            #[cfg(not(feature = "sd-storage"))]
                            let opened: Result<
                                (
                                    binbook_core::Book<
                                        binbook_fw::storage_access::RuntimeBookSource<
                                            '_,
                                            DisplayFilesystem,
                                        >,
                                    >,
                                    binbook_core::RenditionIdentity,
                                ),
                                u8,
                            > = Err(0);
                            if let Ok((book, identity)) = opened {
                                #[cfg(not(feature = "sd-storage"))]
                                let _ = identity;
                                let page_count = book.page_count();
                                #[cfg(feature = "sd-storage")]
                                let filename_hash = library.entries()[selected_index].filename_hash;
                                #[cfg(not(feature = "sd-storage"))]
                                let filename_hash = 0;
                                #[cfg(feature = "sd-storage")]
                                let resume_page = reader_state
                                    .lookup_page(filename_hash, &identity.rendition_hash)
                                    .unwrap_or(0)
                                    .min(page_count.saturating_sub(1));
                                #[cfg(not(feature = "sd-storage"))]
                                let resume_page = 0;
                                backend.book = Some(book);
                                engine = DisplayEngine::new_with_refresh_mode(
                                    page_count,
                                    binbook_fw::async_refresh::STARTUP_PAGE_REFRESH_MODE,
                                );
                                let opened_ok =
                                    match backend.render_ui_to_book(&menu_fb, resume_page).await {
                                        Ok(()) => engine
                                            .commit_differential_page(
                                                resume_page,
                                                embassy_time::Instant::now().as_millis(),
                                            )
                                            .is_ok(),
                                        Err(_) => engine
                                            .initialize_at(
                                                resume_page,
                                                &mut backend,
                                                &mut events,
                                                now_ms,
                                            )
                                            .await
                                            .is_none(),
                                    };
                                if !opened_ok {
                                    backend.book = None;
                                    continue;
                                }
                                #[cfg(feature = "sd-storage")]
                                {
                                    let _ = reader_state.set_progress(
                                        binbook_fw::reader_state::ProgressRecord {
                                            filename_hash,
                                            rendition_hash: identity.rendition_hash,
                                            page: resume_page,
                                        },
                                    );
                                    reader_state.selected_filename_hash = filename_hash;
                                    let _ = reader_state
                                        .set_reading(filename_hash, identity.rendition_hash);
                                }
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
                                        filename_hash,
                                        rendition_hash_prefix: u32::from_le_bytes(
                                            identity.rendition_hash[..4]
                                                .try_into()
                                                .expect("four-byte rendition prefix"),
                                        ),
                                        page_count,
                                        page: resume_page,
                                    },
                                });
                            } else {
                                let stage = opened.err().unwrap_or(0);
                                #[cfg(feature = "sd-storage")]
                                let failed_name = library.entries()[selected_index].name.as_str();
                                #[cfg(not(feature = "sd-storage"))]
                                let failed_name = menu_names[selected_index].as_str();
                                render_book_open_error(&mut menu_fb, failed_name);
                                let _ = render_ui_bw(
                                    &mut backend.display,
                                    &menu_fb,
                                    &mut backend.delay,
                                )
                                .await;
                                events.push(RuntimeEvent {
                                    timestamp_ms: now_ms,
                                    kind: RuntimeEventKind::BookOpenFailed {
                                        name_index: selected_index,
                                        stage,
                                    },
                                });
                            }
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
                    DisplayRequest::Goto { .. }
                    | DisplayRequest::SetRefreshMode { .. }
                    | DisplayRequest::Probe { .. } => {}
                }
                if runtime_engine::is_menu_request(&request) {
                    let end_ms = embassy_time::Instant::now().as_millis();
                    events.push(RuntimeEvent {
                        timestamp_ms: end_ms,
                        kind: RuntimeEventKind::DisplayRequestEnd {
                            kind: request_kind,
                            duration_ms: end_ms.saturating_sub(start_ms).min(u32::MAX as u64)
                                as u32,
                            status: runtime_engine::RuntimeCompletionStatus::Ok,
                        },
                    });
                    if let Some(completion) =
                        runtime_engine::menu_completion(&request, engine.current_page())
                    {
                        events.push(RuntimeEvent {
                            timestamp_ms: end_ms,
                            kind: RuntimeEventKind::Completion(completion),
                        });
                    }
                    events.flush().await;
                    continue;
                }
                let result = engine
                    .request(display_request(request), &mut backend, &mut events, now_ms)
                    .await;
                #[cfg(feature = "sd-storage")]
                if result.status == xteink_x4_display::events::CompletionStatus::Ok
                    && reader_state.saved_mode == binbook_fw::reader_state::SavedMode::Reading
                {
                    let _ = reader_state.set_progress(binbook_fw::reader_state::ProgressRecord {
                        filename_hash: reader_state.active_filename_hash,
                        rendition_hash: reader_state.active_rendition_hash,
                        page: engine.current_page(),
                    });
                }
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
            Either::Second(Either::Second(_)) => {
                if super::DISPLAY_MODE.load(portable_atomic::Ordering::Relaxed)
                    == binbook_fw::async_refresh::MODE_READING
                {
                    let _ = engine.advance(&mut backend, &mut events, now_ms).await;
                }
                events.flush().await;
            }
        }
    }
}
