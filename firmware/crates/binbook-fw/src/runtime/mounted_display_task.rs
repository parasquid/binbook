use binbook_core::PlaneSlot;
use binbook_fw::{
    async_refresh::{
        activation_input_mode, DisplayProbeKind, DisplayRequest, GRAY_SETTLE_DELAY_MS, MODE_MENU,
        MODE_OPEN_ERROR, MODE_READING,
    },
    board::{DisplayDelay, FreqManagedSpiDevice, SharedSpi2},
    input::PageTurn,
    library::{LibraryStatus, LibraryViewport, MountFileToken},
    menu::{
        render_book_open_error, render_library_status, render_menu, render_sleep_cancelled,
        MenuAction, MenuState,
    },
    owner_book::{can_reuse_warm_open, OwnerBook},
    owner_reader::{
        mounted_diagnostic_action, mounted_request_needs_gray_refinement, MountedDiagnosticAction,
        OwnerGrayRefinement, OwnerReaderLifecycle, OwnerStateFlush,
    },
    power::{PowerAck, PowerRequest},
    reader_state::ReaderState,
    state_owner::OwnerStateSlots,
    storage_owner::StorageName,
};
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig};
use static_cell::StaticCell;
use xteink_x4_display::{
    framebuffer::Gray2Framebuffer,
    panel::{PanelRamTarget, X4Panel},
    ui_render::{
        activate_clean, activate_differential, activate_fast_differential, render_ui_bw,
        render_ui_transition, stage_ui_base_to_ram,
    },
};

use super::{
    display_backend::RuntimeBusyWaitObserver,
    mounted_reader_io::{
        load_page, load_state, load_viewport, load_viewport_window, open_book,
        prune_state_from_index, save_state, stage_plane, stage_plane_for_epoch, OwnerReaderBuffers,
        OwnerStateBuffer,
    },
    LibraryRefreshTrigger, PowerAckSender, PowerRequestReceiver, RequestReceiver, DISPLAY_MODE,
    LIBRARY_REFRESH_COMPLETION_CHANNEL, LIBRARY_REFRESH_TRIGGER_CHANNEL, REQUEST_EPOCH,
    RUNTIME_EVENT_CHANNEL,
};

const LATENCY_OPEN_BOOK: u8 = 50;
const LATENCY_OPEN_PAGE_METADATA: u8 = 51;
const LATENCY_OPEN_PREVIOUS_PLANE: u8 = 52;
const LATENCY_OPEN_TARGET_PLANE: u8 = 53;
const LATENCY_OPEN_ACTIVATION: u8 = 54;
const LATENCY_MENU_PREVIOUS_PLANE: u8 = 55;
const LATENCY_MENU_COMPOSE: u8 = 56;
const LATENCY_MENU_TARGET_PLANE: u8 = 57;
const LATENCY_MENU_ACTIVATION: u8 = 58;
const LATENCY_BACK_RED_PLANE: u8 = 60;
const LATENCY_BACK_TARGET_PLANE: u8 = 61;
const LATENCY_BACK_ACTIVATION: u8 = 62;
const LATENCY_BACK_STATE_SAVE: u8 = 63;
const STATE_IDLE_SAVE_DELAY_MS: u64 = 2_000;
const STATE_IDLE_SAVE_RETRY_MS: u64 = 5_000;
const MAINTENANCE_IDLE_WAIT_MS: u64 = 86_400_000;

#[embassy_executor::task]
pub(super) async fn mounted_display_task(
    shared_spi2: &'static SharedSpi2,
    gpio21: esp_hal::peripherals::GPIO21<'static>,
    gpio4: esp_hal::peripherals::GPIO4<'static>,
    gpio5: esp_hal::peripherals::GPIO5<'static>,
    gpio6: esp_hal::peripherals::GPIO6<'static>,
    request_rx: RequestReceiver,
    power_rx: PowerRequestReceiver,
    power_ack_tx: PowerAckSender,
    viewport: &'static mut LibraryViewport,
) {
    use embassy_futures::select::{select4, Either4};

    static MENU_FB: StaticCell<Gray2Framebuffer> = StaticCell::new();
    static READER_STATE: StaticCell<ReaderState> = StaticCell::new();
    let menu_fb = MENU_FB.init(Gray2Framebuffer::new());
    let reader_state = READER_STATE.init(ReaderState::empty());
    let mut buffers = OwnerReaderBuffers::init();
    let mut state_buffer = OwnerStateBuffer::init();
    let mut state_slots = OwnerStateSlots::new();
    let mut sequence = 50_000u32;
    let mut state_sequence = 150_000u32;
    let status = load_viewport(viewport, 0, &mut buffers, &mut sequence)
        .await
        .unwrap_or(LibraryStatus::CardError);
    probe(30, status.code(), viewport.entries().len() as u32).await;
    let state_warning = load_state(
        &mut state_slots,
        reader_state,
        &mut state_buffer,
        &mut state_sequence,
    )
    .await
    .unwrap_or(true);
    probe(37, u8::from(state_warning), reader_state.generation).await;

    let restore_hash = if reader_state.saved_mode == binbook_fw::reader_state::SavedMode::Reading {
        reader_state.active_filename_hash
    } else {
        reader_state.selected_filename_hash
    };
    if status == LibraryStatus::Ready && restore_hash != 0 {
        let _ = load_selected_viewport(viewport, restore_hash, &mut buffers, &mut sequence).await;
    }

    let cs = Output::new(gpio21, Level::High, OutputConfig::default());
    let spi = FreqManagedSpiDevice::new(
        shared_spi2,
        cs,
        crate::DISPLAY_SPI_FREQUENCY_MHZ * 1_000_000,
    );
    let dc = Output::new(gpio4, Level::Low, OutputConfig::default());
    let rst = Output::new(gpio5, Level::High, OutputConfig::default());
    let busy = Input::new(gpio6, InputConfig::default());
    let mut panel = X4Panel::new(spi, dc, rst, busy);
    let mut delay = DisplayDelay;
    let _ = panel.init_absolute_gray_async(&mut delay).await;
    RUNTIME_EVENT_CHANNEL
        .send(binbook_fw::runtime_engine::RuntimeEvent {
            timestamp_ms: embassy_time::Instant::now().as_millis(),
            kind: binbook_fw::runtime_engine::RuntimeEventKind::PanelModeChanged(
                xteink_x4_display::events::PanelMode::Grayscale,
            ),
        })
        .await;

    let mut menu = MenuState::new(viewport.total());
    if let Some(selected) = viewport
        .entries()
        .iter()
        .position(|entry| entry.filename_hash == reader_state.selected_filename_hash)
        .map(|offset| viewport.start() + offset)
    {
        menu.restore_selected(selected);
    }
    compose_menu(menu_fb, status, &menu, viewport);
    let _ = render_ui_bw(&mut panel, menu_fb, &mut delay).await;
    probe(31, 0, 0).await;
    DISPLAY_MODE.store(MODE_MENU, portable_atomic::Ordering::Relaxed);

    let mut prepared = heapless::Vec::<
        (StorageName, MountFileToken),
        { binbook_fw::library::LIBRARY_VIEWPORT_CAPACITY },
    >::new();
    request_refresh(viewport).await;
    probe(32, 0, 0).await;
    let mut lifecycle = OwnerReaderLifecycle::new();
    let mut book: Option<OwnerBook> = None;
    let mut current_page_info: Option<binbook_core::PageInfo> = None;
    let mut open_error: Option<StorageName> = None;
    let mut restore_reading =
        reader_state.saved_mode == binbook_fw::reader_state::SavedMode::Reading;
    let mut gray = OwnerGrayRefinement::new();
    let mut state_flush = OwnerStateFlush::new();
    let mut refresh_mode = xteink_x4_display::events::PageRefreshMode::Balanced;

    loop {
        let now = embassy_time::Instant::now().as_millis();
        let maintenance_wait = gray
            .wait_ms(now, MAINTENANCE_IDLE_WAIT_MS)
            .min(state_flush.wait_ms(now, MAINTENANCE_IDLE_WAIT_MS));
        match select4(
            request_rx.receive(),
            power_rx.receive(),
            LIBRARY_REFRESH_COMPLETION_CHANNEL.receive(),
            embassy_time::Timer::after_millis(maintenance_wait),
        )
        .await
        {
            Either4::First(request) => {
                gray.cancel();
                state_flush.postpone(
                    embassy_time::Instant::now().as_millis(),
                    STATE_IDLE_SAVE_DELAY_MS,
                );
                probe(
                    33,
                    binbook_fw::runtime_engine::RuntimeRequestKind::from_request(&request)
                        .log_code() as u8,
                    0,
                )
                .await;
                let started = embassy_time::Instant::now().as_millis();
                let old_mode = DISPLAY_MODE.load(portable_atomic::Ordering::Relaxed);
                let page_before = match lifecycle.phase() {
                    binbook_fw::owner_reader::OwnerReaderPhase::Reading { page, .. } => page,
                    _ => 0,
                };
                let success = handle_request(
                    request,
                    viewport,
                    &mut menu,
                    &mut prepared,
                    &mut lifecycle,
                    &mut book,
                    &mut current_page_info,
                    &mut open_error,
                    menu_fb,
                    &mut panel,
                    &mut delay,
                    &mut buffers,
                    &mut sequence,
                    reader_state,
                    &mut refresh_mode,
                )
                .await;
                let new_mode = DISPLAY_MODE.load(portable_atomic::Ordering::Relaxed);
                let page = match lifecycle.phase() {
                    binbook_fw::owner_reader::OwnerReaderPhase::Reading { page, .. }
                    | binbook_fw::owner_reader::OwnerReaderPhase::RenderingTurn {
                        from: page,
                        ..
                    }
                    | binbook_fw::owner_reader::OwnerReaderPhase::RenderingOpen { target: page } => {
                        page
                    }
                    _ => 0,
                };
                let ended = embassy_time::Instant::now().as_millis();
                if success
                    && matches!(
                        request,
                        DisplayRequest::MenuNext { .. }
                            | DisplayRequest::MenuPrev { .. }
                            | DisplayRequest::MenuSelect { .. }
                            | DisplayRequest::MenuBack { .. }
                            | DisplayRequest::Turn { .. }
                            | DisplayRequest::Goto { .. }
                    )
                {
                    state_flush.mark_dirty(ended, STATE_IDLE_SAVE_DELAY_MS);
                }
                if let Some(kind) = binbook_fw::runtime_engine::committed_page_event(
                    &request,
                    success,
                    page_before,
                    page,
                ) {
                    RUNTIME_EVENT_CHANNEL
                        .send(binbook_fw::runtime_engine::RuntimeEvent {
                            timestamp_ms: ended,
                            kind,
                        })
                        .await;
                }
                if success
                    && new_mode == MODE_READING
                    && mounted_request_needs_gray_refinement(&request)
                {
                    gray.schedule(
                        page,
                        REQUEST_EPOCH.load(portable_atomic::Ordering::Acquire),
                        ended,
                        GRAY_SETTLE_DELAY_MS,
                    );
                }
                let kind = binbook_fw::runtime_engine::RuntimeRequestKind::from_request(&request);
                RUNTIME_EVENT_CHANNEL
                    .send(binbook_fw::runtime_engine::RuntimeEvent {
                        timestamp_ms: ended,
                        kind: binbook_fw::runtime_engine::RuntimeEventKind::DisplayRequestEnd {
                            kind,
                            duration_ms: ended.saturating_sub(started).min(u32::MAX as u64) as u32,
                            status: if success {
                                binbook_fw::runtime_engine::RuntimeCompletionStatus::Ok
                            } else {
                                binbook_fw::runtime_engine::RuntimeCompletionStatus::Error
                            },
                        },
                    })
                    .await;
                if old_mode != new_mode {
                    RUNTIME_EVENT_CHANNEL
                        .send(binbook_fw::runtime_engine::RuntimeEvent {
                            timestamp_ms: ended,
                            kind: binbook_fw::runtime_engine::RuntimeEventKind::ModeChange {
                                mode: if new_mode == MODE_READING {
                                    binbook_fw::menu::Mode::Reading
                                } else {
                                    binbook_fw::menu::Mode::Menu
                                },
                            },
                        })
                        .await;
                    if new_mode == MODE_READING {
                        if let (Some(entry), Some(opened)) =
                            (viewport_entry(viewport, menu.selected()), book.as_ref())
                        {
                            let hash = opened.rendition_identity().rendition_hash;
                            if let Some(kind) =
                                binbook_fw::runtime_engine::committed_book_open_event(
                                    true,
                                    entry.filename_hash,
                                    u32::from_le_bytes(hash[..4].try_into().unwrap()),
                                    opened.page_count(),
                                    page,
                                )
                            {
                                RUNTIME_EVENT_CHANNEL
                                    .send(binbook_fw::runtime_engine::RuntimeEvent {
                                        timestamp_ms: ended,
                                        kind,
                                    })
                                    .await;
                            }
                        }
                    }
                }
                if let Some(sequence) = binbook_fw::runtime_engine::request_sequence(&request) {
                    RUNTIME_EVENT_CHANNEL
                        .send(binbook_fw::runtime_engine::RuntimeEvent {
                            timestamp_ms: ended,
                            kind: binbook_fw::runtime_engine::RuntimeEventKind::Completion(
                                binbook_fw::runtime_engine::RuntimeCompletion {
                                    sequence: Some(sequence),
                                    status: if success {
                                        binbook_fw::runtime_engine::RuntimeCompletionStatus::Ok
                                    } else {
                                        binbook_fw::runtime_engine::RuntimeCompletionStatus::Error
                                    },
                                    page,
                                    error: None,
                                },
                            ),
                        })
                        .await;
                }
            }
            Either4::Second(power) => match power {
                PowerRequest::FullRefresh => {
                    gray.cancel();
                    let mode = DISPLAY_MODE.load(portable_atomic::Ordering::Relaxed);
                    let result = if mode == MODE_MENU || mode == MODE_OPEN_ERROR {
                        render_ui_bw(&mut panel, menu_fb, &mut delay)
                            .await
                            .map_err(|_| ())
                    } else if mode == MODE_READING {
                        full_refresh_current_page(
                            &mut panel,
                            &mut delay,
                            book.as_ref(),
                            current_page_info.as_ref(),
                            &mut buffers,
                            &mut sequence,
                        )
                        .await
                    } else {
                        Err(())
                    };
                    if result.is_ok() && mode == MODE_READING {
                        if let binbook_fw::owner_reader::OwnerReaderPhase::Reading {
                            page, ..
                        } = lifecycle.phase()
                        {
                            gray.schedule(
                                page,
                                REQUEST_EPOCH.load(portable_atomic::Ordering::Acquire),
                                embassy_time::Instant::now().as_millis(),
                                GRAY_SETTLE_DELAY_MS,
                            );
                        }
                    }
                    power_ack_tx
                        .send(if result.is_ok() {
                            PowerAck::Ready
                        } else {
                            PowerAck::Failed
                        })
                        .await;
                }
                PowerRequest::PrepareSleep => {
                    let saved = save_state(
                        &mut state_slots,
                        reader_state,
                        &mut state_buffer,
                        &mut state_sequence,
                    )
                    .await;
                    let now = embassy_time::Instant::now().as_millis();
                    let save_outcome =
                        binbook_fw::power::SleepPreparationOutcome::after_state_save(saved.is_ok());
                    if saved.is_ok() {
                        state_flush.record_success();
                    } else {
                        state_flush.record_failure(now, STATE_IDLE_SAVE_RETRY_MS);
                    }
                    if save_outcome.render_sleep_cancelled() {
                        RUNTIME_EVENT_CHANNEL
                            .send(binbook_fw::runtime_engine::RuntimeEvent {
                                timestamp_ms: now,
                                kind: binbook_fw::runtime_engine::RuntimeEventKind::ReaderStateSaveFailed {
                                    stage: 1,
                                },
                            })
                            .await;
                        RUNTIME_EVENT_CHANNEL
                            .send(binbook_fw::runtime_engine::RuntimeEvent {
                                timestamp_ms: now,
                                kind: binbook_fw::runtime_engine::RuntimeEventKind::SleepPrepared {
                                    status: save_outcome.status_code(),
                                },
                            })
                            .await;
                        show_sleep_cancelled(
                            &mut panel,
                            menu_fb,
                            book.as_ref(),
                            current_page_info.as_ref(),
                            &mut buffers,
                            &mut sequence,
                            &mut delay,
                        )
                        .await;
                        power_ack_tx.send(save_outcome.ack()).await;
                        continue;
                    }
                    debug_assert!(save_outcome.should_sleep_panel());
                    RUNTIME_EVENT_CHANNEL
                        .send(binbook_fw::runtime_engine::RuntimeEvent {
                            timestamp_ms: now,
                            kind: binbook_fw::runtime_engine::RuntimeEventKind::ReaderStateSaved {
                                generation: reader_state.generation,
                                record_count: reader_state.records().len() as u16,
                            },
                        })
                        .await;
                    let outcome = binbook_fw::power::SleepPreparationOutcome::after_panel_sleep(
                        panel.deep_sleep().is_ok(),
                    );
                    RUNTIME_EVENT_CHANNEL
                        .send(binbook_fw::runtime_engine::RuntimeEvent {
                            timestamp_ms: embassy_time::Instant::now().as_millis(),
                            kind: binbook_fw::runtime_engine::RuntimeEventKind::SleepPrepared {
                                status: outcome.status_code(),
                            },
                        })
                        .await;
                    power_ack_tx.send(outcome.ack()).await;
                }
            },
            Either4::Third(completion) => {
                probe(34, completion.status, completion.elapsed_ms).await;
                prepared = completion.prepared;
                let selected_hash = viewport_entry(viewport, menu.selected())
                    .map(|entry| entry.filename_hash)
                    .unwrap_or(reader_state.selected_filename_hash);
                let current_start = viewport.start();
                if load_viewport(viewport, current_start, &mut buffers, &mut sequence)
                    .await
                    .is_ok()
                {
                    if selected_hash != 0 {
                        let _ = load_selected_viewport(
                            viewport,
                            selected_hash,
                            &mut buffers,
                            &mut sequence,
                        )
                        .await;
                    }
                    probe(35, 0, viewport.entries().len() as u32).await;
                    menu.reset(viewport.total());
                    if let Some(selected) = viewport
                        .entries()
                        .iter()
                        .position(|entry| entry.filename_hash == selected_hash)
                        .map(|offset| viewport.start() + offset)
                    {
                        menu.restore_selected(selected);
                        reader_state.selected_filename_hash = selected_hash;
                    }
                    if DISPLAY_MODE.load(portable_atomic::Ordering::Relaxed) == MODE_MENU {
                        let _ = render_ui_transition(
                            &mut panel,
                            menu_fb,
                            |fb| compose_menu(fb, LibraryStatus::Ready, &menu, viewport),
                            &mut delay,
                        )
                        .await;
                        probe(36, 0, 0).await;
                    }
                }
                if completion.status == LibraryStatus::Ready.code() {
                    let before = reader_state.records().len();
                    if let Ok(removed) =
                        prune_state_from_index(reader_state, &mut buffers, &mut sequence).await
                    {
                        if removed > 0 {
                            let saved = save_state(
                                &mut state_slots,
                                reader_state,
                                &mut state_buffer,
                                &mut state_sequence,
                            )
                            .await;
                            if saved.is_ok() {
                                state_flush.record_success();
                            } else {
                                state_flush.record_failure(
                                    embassy_time::Instant::now().as_millis(),
                                    STATE_IDLE_SAVE_RETRY_MS,
                                );
                            }
                        }
                        RUNTIME_EVENT_CHANNEL
                            .send(binbook_fw::runtime_engine::RuntimeEvent {
                                timestamp_ms: embassy_time::Instant::now().as_millis(),
                                kind: binbook_fw::runtime_engine::RuntimeEventKind::ReaderStatePruned {
                                    removed_count: removed as u16,
                                    remaining_count: before.saturating_sub(removed) as u16,
                                },
                            })
                            .await;
                    }
                    if reader_state.saved_mode != binbook_fw::reader_state::SavedMode::Reading {
                        restore_reading = false;
                    }
                }
                if restore_reading {
                    restore_reading = false;
                    let restored = handle_request(
                        DisplayRequest::MenuSelect {
                            completion_sequence: None,
                        },
                        viewport,
                        &mut menu,
                        &mut prepared,
                        &mut lifecycle,
                        &mut book,
                        &mut current_page_info,
                        &mut open_error,
                        menu_fb,
                        &mut panel,
                        &mut delay,
                        &mut buffers,
                        &mut sequence,
                        reader_state,
                        &mut refresh_mode,
                    )
                    .await;
                    if !restored {
                        reader_state.set_menu();
                    } else if let binbook_fw::owner_reader::OwnerReaderPhase::Reading {
                        page, ..
                    } = lifecycle.phase()
                    {
                        if let (Some(entry), Some(opened)) =
                            (viewport_entry(viewport, menu.selected()), book.as_ref())
                        {
                            let hash = opened.rendition_identity().rendition_hash;
                            if let Some(kind) =
                                binbook_fw::runtime_engine::committed_book_open_event(
                                    true,
                                    entry.filename_hash,
                                    u32::from_le_bytes(hash[..4].try_into().unwrap()),
                                    opened.page_count(),
                                    page,
                                )
                            {
                                RUNTIME_EVENT_CHANNEL
                                    .send(binbook_fw::runtime_engine::RuntimeEvent {
                                        timestamp_ms: embassy_time::Instant::now().as_millis(),
                                        kind,
                                    })
                                    .await;
                            }
                        }
                        gray.schedule(
                            page,
                            REQUEST_EPOCH.load(portable_atomic::Ordering::Acquire),
                            embassy_time::Instant::now().as_millis(),
                            GRAY_SETTLE_DELAY_MS,
                        );
                    }
                    probe(38, u8::from(!restored), 0).await;
                }
                let _ = (
                    completion.status,
                    completion.book_count,
                    completion.elapsed_ms,
                );
            }
            Either4::Fourth(_) => {
                let now = embassy_time::Instant::now().as_millis();
                let epoch = REQUEST_EPOCH.load(portable_atomic::Ordering::Acquire);
                if let Some((page, expected_epoch)) = gray.take_due(now, epoch) {
                    let refined = refine_current_page(
                        &mut panel,
                        &mut delay,
                        book.as_ref(),
                        current_page_info.as_ref(),
                        &mut buffers,
                        &mut sequence,
                        expected_epoch,
                    )
                    .await;
                    probe(39, u8::from(!refined), page).await;
                    if refined {
                        let timestamp_ms = embassy_time::Instant::now().as_millis();
                        RUNTIME_EVENT_CHANNEL
                            .send(binbook_fw::runtime_engine::RuntimeEvent {
                                timestamp_ms,
                                kind: binbook_fw::runtime_engine::RuntimeEventKind::GrayActivated {
                                    page,
                                },
                            })
                            .await;
                    }
                }
                let now = embassy_time::Instant::now().as_millis();
                if state_flush.due(now) {
                    let started = now;
                    let saved = save_state(
                        &mut state_slots,
                        reader_state,
                        &mut state_buffer,
                        &mut state_sequence,
                    )
                    .await
                    .is_ok();
                    latency_probe(LATENCY_BACK_STATE_SAVE, saved, started).await;
                    if saved {
                        state_flush.record_success();
                        RUNTIME_EVENT_CHANNEL
                            .send(binbook_fw::runtime_engine::RuntimeEvent {
                                timestamp_ms: embassy_time::Instant::now().as_millis(),
                                kind:
                                    binbook_fw::runtime_engine::RuntimeEventKind::ReaderStateSaved {
                                        generation: reader_state.generation,
                                        record_count: reader_state.records().len() as u16,
                                    },
                            })
                            .await;
                    } else {
                        state_flush.record_failure(
                            embassy_time::Instant::now().as_millis(),
                            STATE_IDLE_SAVE_RETRY_MS,
                        );
                    }
                }
            }
        }
    }
}

async fn full_refresh_current_page<SPI, DC, RST, BUSY>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    delay: &mut DisplayDelay,
    book: Option<&OwnerBook>,
    page: Option<&binbook_core::PageInfo>,
    buffers: &mut OwnerReaderBuffers,
    sequence: &mut u32,
) -> Result<(), ()>
where
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
{
    let (Some(book), Some(page)) = (book, page) else {
        return Err(());
    };
    let plane = page.planes.get(PlaneSlot::FastBase).ok_or(())?;
    stage_plane(panel, book, plane, PanelRamTarget::Red, buffers, sequence).await?;
    stage_plane(panel, book, plane, PanelRamTarget::Black, buffers, sequence).await?;
    panel
        .refresh_observed(
            xteink_x4_display::panel::RefreshMode::Full,
            delay,
            &mut ssd1677_driver::NoopBusyWaitObserver,
        )
        .await
        .map_err(|_| ())
}

async fn refine_current_page<SPI, DC, RST, BUSY>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    delay: &mut DisplayDelay,
    book: Option<&OwnerBook>,
    page: Option<&binbook_core::PageInfo>,
    buffers: &mut OwnerReaderBuffers,
    sequence: &mut u32,
    expected_epoch: u32,
) -> bool
where
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
{
    let (Some(book), Some(page)) = (book, page) else {
        return false;
    };
    let (Some(lsb), Some(msb)) = (
        page.planes.get(PlaneSlot::OverlayLsb),
        page.planes.get(PlaneSlot::OverlayMsb),
    ) else {
        return true;
    };
    if stage_plane_for_epoch(
        panel,
        book,
        lsb,
        PanelRamTarget::Black,
        buffers,
        sequence,
        expected_epoch,
    )
    .await
        != Ok(true)
    {
        return false;
    }
    if stage_plane_for_epoch(
        panel,
        book,
        msb,
        PanelRamTarget::Red,
        buffers,
        sequence,
        expected_epoch,
    )
    .await
        != Ok(true)
        || REQUEST_EPOCH.load(portable_atomic::Ordering::Acquire) != expected_epoch
        || panel.load_staged_gray().is_err()
        || REQUEST_EPOCH.load(portable_atomic::Ordering::Acquire) != expected_epoch
    {
        return false;
    }
    panel.activate_staged_gray(delay).await.is_ok()
}

async fn probe(phase: u8, status: u8, elapsed_ms: u32) {
    RUNTIME_EVENT_CHANNEL
        .send(binbook_fw::runtime_engine::RuntimeEvent {
            timestamp_ms: embassy_time::Instant::now().as_millis(),
            kind: binbook_fw::runtime_engine::RuntimeEventKind::StorageOwnerProbe {
                phase,
                status,
                elapsed_ms,
            },
        })
        .await;
}

#[allow(clippy::too_many_arguments)]
async fn handle_request<SPI, DC, RST, BUSY>(
    request: DisplayRequest,
    viewport: &mut LibraryViewport,
    menu: &mut MenuState,
    prepared: &mut heapless::Vec<
        (StorageName, MountFileToken),
        { binbook_fw::library::LIBRARY_VIEWPORT_CAPACITY },
    >,
    lifecycle: &mut OwnerReaderLifecycle,
    book: &mut Option<OwnerBook>,
    current_page_info: &mut Option<binbook_core::PageInfo>,
    open_error: &mut Option<StorageName>,
    menu_fb: &mut Gray2Framebuffer,
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    delay: &mut DisplayDelay,
    buffers: &mut OwnerReaderBuffers,
    sequence: &mut u32,
    reader_state: &mut ReaderState,
    refresh_mode: &mut xteink_x4_display::events::PageRefreshMode,
) -> bool
where
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
{
    match request {
        DisplayRequest::MenuNext { .. } | DisplayRequest::MenuPrev { .. } => {
            let action = if matches!(request, DisplayRequest::MenuNext { .. }) {
                MenuAction::Next
            } else {
                MenuAction::Previous
            };
            let _ = menu.transition(action);
            if !viewport_contains(viewport, menu.selected()) {
                if let Some(cursor) = viewport.cursor() {
                    let start = menu.selected().saturating_sub(5);
                    if load_viewport_window(viewport, cursor, start, buffers, sequence)
                        .await
                        .is_ok()
                    {
                        prepared.clear();
                        request_refresh(viewport).await;
                    }
                }
            }
            let previous_started = embassy_time::Instant::now().as_millis();
            let previous_staged = stage_ui_base_to_ram(panel, menu_fb, PanelRamTarget::Red).is_ok();
            latency_probe(
                LATENCY_MENU_PREVIOUS_PLANE,
                previous_staged,
                previous_started,
            )
            .await;
            let compose_started = embassy_time::Instant::now().as_millis();
            compose_menu(menu_fb, LibraryStatus::Ready, menu, viewport);
            latency_probe(LATENCY_MENU_COMPOSE, true, compose_started).await;
            let target_started = embassy_time::Instant::now().as_millis();
            let target_staged = previous_staged
                && stage_ui_base_to_ram(panel, menu_fb, PanelRamTarget::Black).is_ok();
            latency_probe(LATENCY_MENU_TARGET_PLANE, target_staged, target_started).await;
            let activation_started = embassy_time::Instant::now().as_millis();
            let activated = target_staged && activate_fast_differential(panel, delay).await.is_ok();
            latency_probe(LATENCY_MENU_ACTIVATION, activated, activation_started).await;
            if let Some(entry) = viewport_entry(viewport, menu.selected()) {
                reader_state.selected_filename_hash = entry.filename_hash;
            }
            activated
        }
        request @ DisplayRequest::MenuSelect { .. } => {
            let Some(entry) = viewport_entry(viewport, menu.selected()) else {
                return false;
            };
            let name = entry.name.clone();
            let filename_hash = entry.filename_hash;
            let token = prepared
                .iter()
                .find(|(candidate, _)| candidate.as_str() == name.as_str())
                .map(|(_, token)| *token);
            let warm_open = book.as_ref().zip(current_page_info.as_ref()).is_some_and(
                |(cached_book, cached_page)| {
                    let cached_rendition = cached_book.rendition_identity().rendition_hash;
                    let saved_page = reader_state
                        .lookup_page(filename_hash, &cached_rendition)
                        .unwrap_or(0);
                    can_reuse_warm_open(
                        filename_hash,
                        binbook_fw::reader_state::filename_hash(
                            cached_book.name().as_str().as_bytes(),
                        ),
                        &cached_rendition,
                        &cached_rendition,
                        saved_page,
                        cached_page.page_number.get(),
                    )
                },
            );
            let open_started = embassy_time::Instant::now().as_millis();
            let opened = lifecycle.begin_open().is_ok()
                && (warm_open
                    || open_book(name.as_str(), token, book, buffers, sequence)
                        .await
                        .is_ok());
            latency_probe(LATENCY_OPEN_BOOK, opened, open_started).await;
            if !opened {
                let _ = lifecycle.fail_open();
                show_open_error(panel, menu_fb, name.as_str(), delay).await;
                *open_error = StorageName::new(name.as_str()).ok();
                DISPLAY_MODE.store(MODE_OPEN_ERROR, portable_atomic::Ordering::Relaxed);
                return false;
            }
            let page_count = book.as_ref().unwrap().page_count();
            let rendition_hash = book.as_ref().unwrap().rendition_identity().rendition_hash;
            let saved_page = reader_state
                .lookup_page(filename_hash, &rendition_hash)
                .unwrap_or(0);
            let Ok(target) = lifecycle.metadata_opened(page_count, saved_page) else {
                show_open_error(panel, menu_fb, name.as_str(), delay).await;
                *open_error = StorageName::new(name.as_str()).ok();
                DISPLAY_MODE.store(MODE_OPEN_ERROR, portable_atomic::Ordering::Relaxed);
                return false;
            };
            let page_started = embassy_time::Instant::now().as_millis();
            let page_loaded = warm_open
                && current_page_info
                    .as_ref()
                    .is_some_and(|page| page.page_number.get() == target)
                || load_page(
                    book.as_ref().unwrap(),
                    target,
                    current_page_info,
                    buffers,
                    sequence,
                )
                .await
                .is_ok();
            latency_probe(LATENCY_OPEN_PAGE_METADATA, page_loaded, page_started).await;
            if !page_loaded {
                let _ = lifecycle.fail_open();
                show_open_error(panel, menu_fb, name.as_str(), delay).await;
                *open_error = StorageName::new(name.as_str()).ok();
                DISPLAY_MODE.store(MODE_OPEN_ERROR, portable_atomic::Ordering::Relaxed);
                return false;
            }
            let plane = current_page_info
                .as_ref()
                .and_then(|page| page.planes.get(PlaneSlot::FastBase));
            let rendered = if let Some(plane) = plane {
                let previous_started = embassy_time::Instant::now().as_millis();
                let previous_staged =
                    stage_ui_base_to_ram(panel, menu_fb, PanelRamTarget::Red).is_ok();
                latency_probe(
                    LATENCY_OPEN_PREVIOUS_PLANE,
                    previous_staged,
                    previous_started,
                )
                .await;
                let target_started = embassy_time::Instant::now().as_millis();
                let target_staged = previous_staged
                    && stage_plane(
                        panel,
                        book.as_ref().unwrap(),
                        plane,
                        PanelRamTarget::Black,
                        buffers,
                        sequence,
                    )
                    .await
                    .is_ok();
                latency_probe(LATENCY_OPEN_TARGET_PLANE, target_staged, target_started).await;
                if target_staged {
                    if let Some(mode) = activation_input_mode(&request) {
                        DISPLAY_MODE.store(mode, portable_atomic::Ordering::Relaxed);
                    }
                }
                let activation_started = embassy_time::Instant::now().as_millis();
                let activated =
                    target_staged && activate_fast_differential(panel, delay).await.is_ok();
                latency_probe(LATENCY_OPEN_ACTIVATION, activated, activation_started).await;
                activated
            } else {
                false
            };
            if rendered && lifecycle.commit_open(page_count).is_ok() {
                DISPLAY_MODE.store(MODE_READING, portable_atomic::Ordering::Relaxed);
                *open_error = None;
                reader_state
                    .commit_visible_page(filename_hash, rendition_hash, target)
                    .is_ok()
            } else {
                let _ = lifecycle.fail_open();
                *book = None;
                *current_page_info = None;
                show_open_error(panel, menu_fb, name.as_str(), delay).await;
                *open_error = StorageName::new(name.as_str()).ok();
                DISPLAY_MODE.store(MODE_OPEN_ERROR, portable_atomic::Ordering::Relaxed);
                false
            }
        }
        request @ (DisplayRequest::Turn { .. } | DisplayRequest::Goto { .. }) => {
            let transition = match request {
                DisplayRequest::Turn { turn, .. } => {
                    let delta = if turn == PageTurn::Previous { -1 } else { 1 };
                    lifecycle.begin_turn(delta)
                }
                DisplayRequest::Goto { page, .. } => lifecycle.begin_goto(page),
                _ => unreachable!(),
            };
            let Ok((from, target)) = transition else {
                return false;
            };
            if from == target {
                let _ = lifecycle.finish_turn(true);
                return true;
            }
            let Some(previous) = current_page_info.as_ref() else {
                let _ = lifecycle.finish_turn(false);
                return false;
            };
            let Some(previous_plane) = previous.planes.get(PlaneSlot::FastBase) else {
                let _ = lifecycle.finish_turn(false);
                return false;
            };
            let mut next = None;
            let mut ok = stage_plane(
                panel,
                book.as_ref().unwrap(),
                previous_plane,
                PanelRamTarget::Red,
                buffers,
                sequence,
            )
            .await
            .is_ok();
            ok &= load_page(book.as_ref().unwrap(), target, &mut next, buffers, sequence)
                .await
                .is_ok();
            if let Some(plane) = next
                .as_ref()
                .and_then(|page| page.planes.get(PlaneSlot::FastBase))
            {
                ok &= stage_plane(
                    panel,
                    book.as_ref().unwrap(),
                    plane,
                    PanelRamTarget::Black,
                    buffers,
                    sequence,
                )
                .await
                .is_ok();
            } else {
                ok = false;
            }
            ok &= match refresh_mode {
                xteink_x4_display::events::PageRefreshMode::Balanced => {
                    activate_differential(panel, delay).await.is_ok()
                }
                xteink_x4_display::events::PageRefreshMode::Fast => {
                    activate_fast_differential(panel, delay).await.is_ok()
                }
            };
            if lifecycle.finish_turn(ok).is_ok() && ok {
                *current_page_info = next;
                if let Some(opened) = book.as_ref() {
                    let rendition_hash = opened.rendition_identity().rendition_hash;
                    if reader_state
                        .commit_visible_page(
                            reader_state.active_filename_hash,
                            rendition_hash,
                            target,
                        )
                        .is_err()
                    {
                        return false;
                    }
                }
            }
            ok
        }
        request @ DisplayRequest::MenuBack { .. } => {
            if open_error.is_some() {
                let result = render_ui_transition(
                    panel,
                    menu_fb,
                    |fb| compose_menu(fb, LibraryStatus::Ready, menu, viewport),
                    delay,
                )
                .await
                .is_ok();
                if result {
                    *open_error = None;
                    DISPLAY_MODE.store(MODE_MENU, portable_atomic::Ordering::Relaxed);
                }
                return result;
            }
            if current_page_info.is_none() {
                return false;
            }
            compose_menu(menu_fb, LibraryStatus::Ready, menu, viewport);
            let red_started = embassy_time::Instant::now().as_millis();
            let red_staged = stage_ui_base_to_ram(panel, menu_fb, PanelRamTarget::Red).is_ok();
            latency_probe(LATENCY_BACK_RED_PLANE, red_staged, red_started).await;
            let target_started = embassy_time::Instant::now().as_millis();
            let target_staged =
                red_staged && stage_ui_base_to_ram(panel, menu_fb, PanelRamTarget::Black).is_ok();
            latency_probe(LATENCY_BACK_TARGET_PLANE, target_staged, target_started).await;
            if target_staged {
                if let Some(mode) = activation_input_mode(&request) {
                    DISPLAY_MODE.store(mode, portable_atomic::Ordering::Relaxed);
                }
            }
            let activation_started = embassy_time::Instant::now().as_millis();
            let activated = target_staged && activate_clean(panel, delay).await.is_ok();
            latency_probe(LATENCY_BACK_ACTIVATION, activated, activation_started).await;
            if activated && lifecycle.close().is_ok() {
                reader_state.set_menu();
                DISPLAY_MODE.store(MODE_MENU, portable_atomic::Ordering::Relaxed);
                return true;
            }
            DISPLAY_MODE.store(MODE_READING, portable_atomic::Ordering::Relaxed);
            false
        }
        request @ (DisplayRequest::SetRefreshMode { .. } | DisplayRequest::Probe { .. }) => {
            match mounted_diagnostic_action(&request).unwrap() {
                MountedDiagnosticAction::SetRefreshMode(mode) => {
                    *refresh_mode = mode;
                    RUNTIME_EVENT_CHANNEL
                        .send(binbook_fw::runtime_engine::RuntimeEvent {
                            timestamp_ms: embassy_time::Instant::now().as_millis(),
                            kind: binbook_fw::runtime_engine::RuntimeEventKind::RefreshModeChanged(
                                mode,
                            ),
                        })
                        .await;
                    true
                }
                MountedDiagnosticAction::Probe(kind) => {
                    let completion_seq = match request {
                        DisplayRequest::Probe {
                            completion_sequence,
                            ..
                        } => completion_sequence,
                        _ => unreachable!(),
                    };
                    RUNTIME_EVENT_CHANNEL
                        .send(binbook_fw::runtime_engine::RuntimeEvent {
                            timestamp_ms: embassy_time::Instant::now().as_millis(),
                            kind: binbook_fw::runtime_engine::RuntimeEventKind::ProbeStarted {
                                sequence: completion_seq,
                                kind,
                            },
                        })
                        .await;
                    let result = match kind {
                        DisplayProbeKind::FullRefreshCurrent => {
                            let mode = DISPLAY_MODE.load(portable_atomic::Ordering::Relaxed);
                            if mode == MODE_MENU || mode == MODE_OPEN_ERROR {
                                render_ui_bw(panel, menu_fb, delay).await.map_err(|_| ())
                            } else if mode == MODE_READING {
                                full_refresh_current_page(
                                    panel,
                                    delay,
                                    book.as_ref(),
                                    current_page_info.as_ref(),
                                    buffers,
                                    sequence,
                                )
                                .await
                            } else {
                                Err(())
                            }
                        }
                        DisplayProbeKind::ClearWhite => {
                            xteink_x4_display::probes::clear_white_observed(
                                panel,
                                delay,
                                &mut RuntimeBusyWaitObserver::new(
                                    binbook_fw::runtime_engine::BusyWaitSite::Probe,
                                ),
                            )
                            .await
                            .map_err(|_| ())
                        }
                        DisplayProbeKind::WindowCorners => {
                            xteink_x4_display::probes::window_corners_observed(
                                panel,
                                delay,
                                &mut RuntimeBusyWaitObserver::new(
                                    binbook_fw::runtime_engine::BusyWaitSite::Probe,
                                ),
                            )
                            .await
                            .map_err(|_| ())
                        }
                        DisplayProbeKind::PartialWindowBusyMatrix => {
                            #[cfg(feature = "diagnostic-console")]
                            {
                                use super::display_backend::RuntimePartialWindowBusyObserver;

                                xteink_x4_display::probes::partial_window_busy_matrix_observed(
                                    panel,
                                    delay,
                                    1,
                                    &mut RuntimePartialWindowBusyObserver,
                                )
                                .await
                                .map_err(|_| ())
                            }
                            #[cfg(not(feature = "diagnostic-console"))]
                            xteink_x4_display::probes::partial_window_busy_matrix_observed(
                                panel,
                                delay,
                                1,
                                &mut (),
                            )
                            .await
                            .map_err(|_| ())
                        }
                        DisplayProbeKind::CustomLutFastA => {
                            xteink_x4_display::probes::custom_lut_probe_observed(
                                panel,
                                xteink_x4_display::probes::CustomLutCandidate::FastA,
                                delay,
                                &mut RuntimeBusyWaitObserver::new(
                                    binbook_fw::runtime_engine::BusyWaitSite::Probe,
                                ),
                            )
                            .await
                            .map_err(|_| ())
                        }
                        DisplayProbeKind::CustomLutFastB => {
                            xteink_x4_display::probes::custom_lut_probe_observed(
                                panel,
                                xteink_x4_display::probes::CustomLutCandidate::FastB,
                                delay,
                                &mut RuntimeBusyWaitObserver::new(
                                    binbook_fw::runtime_engine::BusyWaitSite::Probe,
                                ),
                            )
                            .await
                            .map_err(|_| ())
                        }
                    };
                    if result.is_ok() {
                        RUNTIME_EVENT_CHANNEL
                            .send(binbook_fw::runtime_engine::RuntimeEvent {
                                timestamp_ms: embassy_time::Instant::now().as_millis(),
                                kind:
                                    binbook_fw::runtime_engine::RuntimeEventKind::ProbeCompleted {
                                        sequence: completion_seq,
                                        kind,
                                    },
                            })
                            .await;
                    }
                    result.is_ok()
                }
            }
        }
    }
}

async fn latency_probe(phase: u8, succeeded: bool, started_ms: u64) {
    #[cfg(feature = "diagnostic-console")]
    probe(
        phase,
        u8::from(!succeeded),
        embassy_time::Instant::now()
            .as_millis()
            .saturating_sub(started_ms)
            .min(u32::MAX as u64) as u32,
    )
    .await;
    #[cfg(not(feature = "diagnostic-console"))]
    let _ = (phase, succeeded, started_ms);
}

#[allow(clippy::too_many_arguments)]
async fn show_sleep_cancelled<SPI, DC, RST, BUSY>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    menu_fb: &mut Gray2Framebuffer,
    book: Option<&OwnerBook>,
    page: Option<&binbook_core::PageInfo>,
    buffers: &mut OwnerReaderBuffers,
    sequence: &mut u32,
    delay: &mut DisplayDelay,
) where
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
{
    if DISPLAY_MODE.load(portable_atomic::Ordering::Relaxed) == MODE_READING {
        if let (Some(book), Some(plane)) = (
            book,
            page.and_then(|page| page.planes.get(PlaneSlot::FastBase)),
        ) {
            let staged_previous =
                stage_plane(panel, book, plane, PanelRamTarget::Red, buffers, sequence)
                    .await
                    .is_ok();
            if staged_previous {
                render_sleep_cancelled(menu_fb);
                let staged_notice =
                    stage_ui_base_to_ram(panel, menu_fb, PanelRamTarget::Black).is_ok();
                if staged_notice {
                    let _ = activate_differential(panel, delay).await;
                }
            }
            if staged_previous {
                return;
            }
        }
    }
    let _ = render_ui_transition(panel, menu_fb, |fb| render_sleep_cancelled(fb), delay).await;
}

async fn show_open_error<SPI, DC, RST, BUSY>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    menu_fb: &mut Gray2Framebuffer,
    name: &str,
    delay: &mut DisplayDelay,
) where
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
{
    let _ =
        render_ui_transition(panel, menu_fb, |fb| render_book_open_error(fb, name), delay).await;
}

async fn load_selected_viewport(
    viewport: &mut LibraryViewport,
    filename_hash: u64,
    buffers: &mut OwnerReaderBuffers,
    sequence: &mut u32,
) -> Result<(), ()> {
    if viewport
        .entries()
        .iter()
        .any(|entry| entry.filename_hash == filename_hash)
    {
        return Ok(());
    }
    let cursor = viewport.cursor().ok_or(())?;
    let mut start = viewport.entries().len();
    while start < cursor.entry_count() {
        load_viewport_window(viewport, cursor, start, buffers, sequence).await?;
        if viewport
            .entries()
            .iter()
            .any(|entry| entry.filename_hash == filename_hash)
        {
            return Ok(());
        }
        start = start.saturating_add(binbook_fw::library::LIBRARY_VIEWPORT_CAPACITY);
    }
    load_viewport_window(viewport, cursor, 0, buffers, sequence).await?;
    Ok(())
}

fn viewport_contains(viewport: &LibraryViewport, index: usize) -> bool {
    index >= viewport.start() && index < viewport.start() + viewport.entries().len()
}

fn viewport_entry(
    viewport: &LibraryViewport,
    index: usize,
) -> Option<&binbook_fw::library::LibraryEntry> {
    viewport.entries().get(index.checked_sub(viewport.start())?)
}

fn compose_menu(
    fb: &mut Gray2Framebuffer,
    status: LibraryStatus,
    menu: &MenuState,
    viewport: &LibraryViewport,
) {
    if status == LibraryStatus::Ready {
        render_menu(fb, menu, viewport);
    } else {
        render_library_status(fb, status);
    }
}

async fn request_refresh(viewport: &LibraryViewport) {
    let mut prefetch = heapless::Vec::new();
    for entry in viewport.entries() {
        if let Ok(name) = StorageName::new(entry.name.as_str()) {
            let _ = prefetch.push(name);
        }
    }
    LIBRARY_REFRESH_TRIGGER_CHANNEL
        .send(LibraryRefreshTrigger { prefetch })
        .await;
}
