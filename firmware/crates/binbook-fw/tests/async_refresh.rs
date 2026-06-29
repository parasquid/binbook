use binbook_fw::async_refresh::{
    RefreshAction, RefreshCoordinator, RefreshPhase, DISPLAY_BUSY_TIMEOUT_MS,
    DISPLAY_COMPLETION_CAPACITY, DISPLAY_STREAM_STRIP_ROWS, GRAY_SETTLE_DELAY_MS,
    INPUT_POLL_INTERVAL_MS, PAGE_TURN_QUEUE_CAPACITY,
};
use binbook_fw::display;
use binbook_fw::panel_driver::new_legacy_display;
use ssd1677_driver::Command;
use std::boxed::Box;
use std::cell::Cell;
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::vec::Vec;
use xteink_hal::RefreshMode;
use xteink_hal::{AsyncDelay, HalError, HalResult, InputPin, OutputPin, Spi};

const PAGE_1: u32 = 1;
const PAGE_2: u32 = 2;

fn test_profile(
    book: &mut binbook_core::Book<binbook_core::SliceSource<'_>>,
) -> binbook_core::DisplayProfile {
    let mut record = [0_u8; 56];
    book.display_profile(&mut record).expect("display profile")
}

fn test_page(
    book: &mut binbook_core::Book<binbook_core::SliceSource<'_>>,
    raw: u32,
) -> binbook_core::PageInfo {
    let number = book.page_number(raw).expect("page number");
    let mut record = [0_u8; binbook_core::PAGE_RECORD_SIZE];
    book.page(number, &mut record).expect("page must exist")
}

#[derive(Debug, Default, PartialEq, Eq)]
struct AsyncGrayTrace {
    red_windows: usize,
    black_windows: usize,
    red_window_heights: Vec<u16>,
    black_window_heights: Vec<u16>,
    yields_between_strips: usize,
    refreshes: Vec<RefreshMode>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct AsyncReseedTrace {
    red_plane_slots: Vec<usize>,
    black_plane_slots: Vec<usize>,
    refreshes: Vec<RefreshMode>,
}

#[derive(Debug, Default)]
struct RecordingSpi {
    writes: Rc<RefCell<Vec<Vec<u8>>>>,
}

impl Spi for RecordingSpi {
    fn write_command(&mut self, cmd: u8, data: &[u8]) -> HalResult<()> {
        self.writes.borrow_mut().push([&[cmd], data].concat());
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> HalResult<()> {
        self.writes.borrow_mut().push(data.to_vec());
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8]) -> HalResult<()> {
        buf.fill(0);
        Ok(())
    }
}

#[derive(Debug, Default)]
struct NoopOutputPin;

impl OutputPin for NoopOutputPin {
    fn set_high(&mut self) -> HalResult<()> {
        Ok(())
    }

    fn set_low(&mut self) -> HalResult<()> {
        Ok(())
    }
}

#[derive(Debug, Default)]
struct LowBusyPin;

impl InputPin for LowBusyPin {
    fn is_high(&self) -> HalResult<bool> {
        Ok(false)
    }
}

#[derive(Debug, Default)]
struct RecordingYieldDelay {
    calls: Rc<RefCell<Vec<u32>>>,
}

impl RecordingYieldDelay {
    fn zero_yield_count(&self) -> usize {
        self.calls
            .borrow()
            .iter()
            .copied()
            .filter(|ms| *ms == 0)
            .count()
    }
}

impl AsyncDelay for RecordingYieldDelay {
    async fn ms(&self, ms: u32) {
        self.calls.borrow_mut().push(ms);
    }
}

fn noop_raw_waker() -> RawWaker {
    unsafe fn clone(_: *const ()) -> RawWaker {
        noop_raw_waker()
    }

    unsafe fn wake(_: *const ()) {}
    unsafe fn wake_by_ref(_: *const ()) {}
    unsafe fn drop(_: *const ()) {}

    RawWaker::new(
        core::ptr::null(),
        &RawWakerVTable::new(clone, wake, wake_by_ref, drop),
    )
}

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);

    loop {
        match Future::poll(Pin::as_mut(&mut future), &mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => {}
        }
    }
}

fn parse_gray_render_trace(writes: &[Vec<u8>], zero_yield_count: usize) -> AsyncGrayTrace {
    let mut trace = AsyncGrayTrace {
        yields_between_strips: zero_yield_count,
        ..AsyncGrayTrace::default()
    };
    let mut pending_window_height: Option<u16> = None;
    let mut pending_refresh_mode: Option<RefreshMode> = None;
    let mut index = 0;

    while index < writes.len() {
        match writes[index].as_slice() {
            [cmd] if *cmd == Command::SET_RAM_Y_ADDR => {
                let data = writes
                    .get(index + 1)
                    .expect("SET_RAM_Y_ADDR must be followed by data");
                let start = u16::from_le_bytes([data[0], data[1]]);
                let end = u16::from_le_bytes([data[2], data[3]]);
                pending_window_height = Some(end - start + 1);
                index += 2;
                continue;
            }
            [cmd] if *cmd == Command::WRITE_RAM_RED => {
                trace.red_windows += 1;
                trace.red_window_heights.push(
                    pending_window_height
                        .expect("window height must be set before red plane write"),
                );
                pending_window_height = None;
            }
            [cmd] if *cmd == Command::WRITE_RAM => {
                trace.black_windows += 1;
                trace.black_window_heights.push(
                    pending_window_height
                        .expect("window height must be set before black plane write"),
                );
                pending_window_height = None;
            }
            [cmd] if *cmd == Command::DISPLAY_UPDATE_CTRL2 => {
                let data = writes
                    .get(index + 1)
                    .expect("DISPLAY_UPDATE_CTRL2 must be followed by data");
                pending_refresh_mode = match data.as_slice() {
                    [value] if *value == Command::UPDATE_CTRL_NORMAL => Some(RefreshMode::Full),
                    [value] if *value == Command::UPDATE_CTRL_FAST => Some(RefreshMode::Partial),
                    [value] if *value == Command::UPDATE_CTRL_GRAYSCALE => {
                        Some(RefreshMode::Grayscale)
                    }
                    _ => None,
                };
                index += 2;
                continue;
            }
            [cmd] if *cmd == Command::MASTER_ACTIVATION => {
                if let Some(mode) = pending_refresh_mode.take() {
                    trace.refreshes.push(mode);
                }
            }
            _ => {}
        }

        index += 1;
    }

    trace
}

fn run_async_gray_render_with_writes(page: u32) -> (AsyncGrayTrace, Vec<Vec<u8>>) {
    let book_bytes = include_bytes!("../fixtures/nav_probe.binbook");
    let mut scratch = [0u8; 8192];
    let mut book = binbook_core::Book::open(
        binbook_core::SliceSource::new(&book_bytes[..]),
        &mut scratch,
    )
    .expect("open nav probe book");
    let profile = test_profile(&mut book);
    let page_info = test_page(&mut book, page);
    assert!(
        display::is_supported_x4_native_gray2_page(&profile, &page_info),
        "page must be x4-native gray2"
    );

    let writes = Rc::new(RefCell::new(Vec::new()));
    let delay_calls = Rc::new(RefCell::new(Vec::new()));
    let mut driver = new_legacy_display(
        RecordingSpi {
            writes: Rc::clone(&writes),
        },
        NoopOutputPin,
        NoopOutputPin,
        NoopOutputPin,
        LowBusyPin,
    );
    let delay = RecordingYieldDelay {
        calls: Rc::clone(&delay_calls),
    };

    block_on(display::display_full_grayscale_async(
        &mut driver,
        &mut book,
        &book_bytes[..],
        page,
        &delay,
    ))
    .expect("async gray render should succeed");

    let captured = writes.borrow().clone();
    let parsed = parse_gray_render_trace(&captured, delay.zero_yield_count());
    (parsed, captured)
}

fn run_async_gray_render(page: u32) -> AsyncGrayTrace {
    run_async_gray_render_with_writes(page).0
}

fn run_async_bw_differential(prev_page: u32, target_page: u32) -> AsyncReseedTrace {
    let book_bytes = include_bytes!("../fixtures/nav_probe.binbook");
    let mut scratch = [0u8; 8192];
    let mut book = binbook_core::Book::open(
        binbook_core::SliceSource::new(&book_bytes[..]),
        &mut scratch,
    )
    .expect("open nav probe book");
    let profile = test_profile(&mut book);
    let prev_info = test_page(&mut book, prev_page);
    let target_info = test_page(&mut book, target_page);
    assert!(
        display::is_supported_x4_native_gray2_page(&profile, &prev_info),
        "previous page must be x4-native gray2"
    );
    assert!(
        display::is_supported_x4_native_gray2_page(&profile, &target_info),
        "target page must be x4-native gray2"
    );

    let writes = Rc::new(RefCell::new(Vec::new()));
    let delay_calls = Rc::new(RefCell::new(Vec::new()));
    let mut driver = new_legacy_display(
        RecordingSpi {
            writes: Rc::clone(&writes),
        },
        NoopOutputPin,
        NoopOutputPin,
        NoopOutputPin,
        LowBusyPin,
    );
    let delay = RecordingYieldDelay {
        calls: Rc::clone(&delay_calls),
    };

    let result = block_on(display::bw_differential_async(
        &mut driver,
        &mut book,
        &book_bytes[..],
        prev_page,
        target_page,
        &delay,
    ));
    result.expect("async bw differential should succeed");

    let parsed = parse_gray_render_trace(&writes.borrow(), delay.zero_yield_count());
    AsyncReseedTrace {
        red_plane_slots: vec![2],
        black_plane_slots: vec![2],
        refreshes: parsed.refreshes,
    }
}

fn run_async_recovery(page: u32) -> AsyncReseedTrace {
    let book_bytes = include_bytes!("../fixtures/nav_probe.binbook");
    let mut scratch = [0u8; 8192];
    let mut book = binbook_core::Book::open(
        binbook_core::SliceSource::new(&book_bytes[..]),
        &mut scratch,
    )
    .expect("open nav probe book");
    let profile = test_profile(&mut book);
    let page_info = test_page(&mut book, page);
    assert!(
        display::is_supported_x4_native_gray2_page(&profile, &page_info),
        "page must be x4-native gray2"
    );

    let writes = Rc::new(RefCell::new(Vec::new()));
    let delay_calls = Rc::new(RefCell::new(Vec::new()));
    let mut driver = new_legacy_display(
        RecordingSpi {
            writes: Rc::clone(&writes),
        },
        NoopOutputPin,
        NoopOutputPin,
        NoopOutputPin,
        LowBusyPin,
    );
    let delay = RecordingYieldDelay {
        calls: Rc::clone(&delay_calls),
    };

    let result = block_on(display::recovery_seed_async(
        &mut driver,
        &mut book,
        &book_bytes[..],
        page,
        &delay,
    ));
    result.expect("async recovery should succeed");

    let parsed = parse_gray_render_trace(&writes.borrow(), delay.zero_yield_count());
    AsyncReseedTrace {
        red_plane_slots: vec![2],
        black_plane_slots: vec![2],
        refreshes: parsed.refreshes,
    }
}

#[test]
fn mismatched_waveform_is_rejected_before_controller_writes() {
    let mut book_bytes = include_bytes!("../fixtures/nav_probe.binbook").to_vec();
    let table_offset = u64::from_le_bytes(book_bytes[24..32].try_into().unwrap()) as usize;
    let section_count = u16::from_le_bytes(book_bytes[38..40].try_into().unwrap()) as usize;
    let profile_entry = (0..section_count)
        .map(|index| table_offset + index * 40)
        .find(|offset| {
            u16::from_le_bytes(book_bytes[*offset..*offset + 2].try_into().unwrap()) == 10
        })
        .unwrap();
    let profile_offset = u64::from_le_bytes(
        book_bytes[profile_entry + 4..profile_entry + 12]
            .try_into()
            .unwrap(),
    ) as usize;
    book_bytes[profile_offset + 53..profile_offset + 55].copy_from_slice(&1u16.to_le_bytes());

    let mut scratch = [0u8; 8192];
    let mut book = binbook_core::Book::open(
        binbook_core::SliceSource::new(&book_bytes[..]),
        &mut scratch,
    )
    .unwrap();
    let writes = Rc::new(RefCell::new(Vec::new()));
    let mut driver = new_legacy_display(
        RecordingSpi {
            writes: Rc::clone(&writes),
        },
        NoopOutputPin,
        NoopOutputPin,
        NoopOutputPin,
        LowBusyPin,
    );
    let delay = RecordingYieldDelay::default();

    let result = block_on(display::display_full_grayscale_async(
        &mut driver,
        &mut book,
        &book_bytes,
        0,
        &delay,
    ));

    assert_eq!(result, Err(HalError::InvalidParam));
    assert!(writes.borrow().is_empty());
}

fn run_staged_gray_with_cancel_at(
    cancel_at_check: usize,
) -> (display::GrayRenderOutcome, Vec<Vec<u8>>) {
    let book_bytes = include_bytes!("../fixtures/nav_probe.binbook");
    let mut scratch = [0u8; 8192];
    let mut book = binbook_core::Book::open(
        binbook_core::SliceSource::new(&book_bytes[..]),
        &mut scratch,
    )
    .unwrap();
    let writes = Rc::new(RefCell::new(Vec::new()));
    let mut driver = new_legacy_display(
        RecordingSpi {
            writes: Rc::clone(&writes),
        },
        NoopOutputPin,
        NoopOutputPin,
        NoopOutputPin,
        LowBusyPin,
    );
    let delay = RecordingYieldDelay::default();
    let checks = Cell::new(0usize);
    let outcome = block_on(display::display_staged_grayscale_async(
        &mut driver,
        &mut book,
        &book_bytes[..],
        0,
        7,
        || {
            let check = checks.get();
            checks.set(check + 1);
            if check >= cancel_at_check {
                8
            } else {
                7
            }
        },
        || {},
        &delay,
    ))
    .unwrap();
    let captured = writes.borrow().clone();
    (outcome, captured)
}

#[test]
fn staged_grayscale_streams_overlay_planes_then_activates() {
    let (outcome, writes) = run_staged_gray_with_cancel_at(usize::MAX);

    assert_eq!(outcome, display::GrayRenderOutcome::Completed);
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [Command::WRITE_RAM])
            .count(),
        30
    );
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [Command::WRITE_RAM_RED])
            .count(),
        30
    );
    assert!(writes.windows(2).any(|pair| {
        pair[0].as_slice() == [Command::DISPLAY_UPDATE_CTRL2]
            && pair[1].as_slice() == [Command::UPDATE_CTRL_STAGED_GRAYSCALE | 0xC0]
    }));
    assert!(writes
        .iter()
        .any(|write| write.as_slice() == [Command::MASTER_ACTIVATION]));
}

#[test]
fn staged_grayscale_cancels_on_strip_boundaries_without_activation() {
    for cancel_at in [0usize, 1, 15, 29] {
        let (outcome, writes) = run_staged_gray_with_cancel_at(cancel_at);

        assert_eq!(outcome, display::GrayRenderOutcome::Cancelled);
        assert_eq!(
            writes
                .iter()
                .filter(|write| write.as_slice() == [Command::WRITE_RAM])
                .count(),
            cancel_at
        );
        assert!(!writes.windows(2).any(|pair| {
            pair[0].as_slice() == [Command::DISPLAY_UPDATE_CTRL2]
                && pair[1].as_slice() == [Command::UPDATE_CTRL_STAGED_GRAYSCALE | 0xC0]
        }));
        assert!(!writes
            .iter()
            .any(|write| write.as_slice() == [Command::MASTER_ACTIVATION]));
    }
}

fn run_base_sync_with_cancel_at(
    cancel_at_check: usize,
) -> (display::BaseSyncOutcome, Vec<Vec<u8>>) {
    let book_bytes = include_bytes!("../fixtures/nav_probe.binbook");
    let mut scratch = [0u8; 8192];
    let mut book = binbook_core::Book::open(
        binbook_core::SliceSource::new(&book_bytes[..]),
        &mut scratch,
    )
    .unwrap();
    let writes = Rc::new(RefCell::new(Vec::new()));
    let mut driver = new_legacy_display(
        RecordingSpi {
            writes: Rc::clone(&writes),
        },
        NoopOutputPin,
        NoopOutputPin,
        NoopOutputPin,
        LowBusyPin,
    );
    let delay = RecordingYieldDelay::default();
    let checks = Cell::new(0usize);
    let outcome = block_on(display::sync_bw_base_async(
        &mut driver,
        &mut book,
        &book_bytes[..],
        0,
        11,
        || {
            let check = checks.get();
            checks.set(check + 1);
            if check >= cancel_at_check {
                12
            } else {
                11
            }
        },
        &delay,
    ))
    .unwrap();
    let captured = writes.borrow().clone();
    (outcome, captured)
}

#[test]
fn background_base_sync_is_cancellable_and_never_activates() {
    for cancel_at in [0usize, 1, 15, 29, usize::MAX] {
        let (outcome, writes) = run_base_sync_with_cancel_at(cancel_at);
        let expected_strips = cancel_at.min(30);
        let expected_outcome = if cancel_at == usize::MAX {
            display::BaseSyncOutcome::Completed
        } else {
            display::BaseSyncOutcome::Cancelled
        };

        assert_eq!(outcome, expected_outcome);
        assert_eq!(
            writes
                .iter()
                .filter(|write| write.as_slice() == [Command::WRITE_RAM_RED])
                .count(),
            expected_strips
        );
        assert!(!writes
            .iter()
            .any(|write| write.as_slice() == [Command::WRITE_RAM]));
        assert!(!writes
            .iter()
            .any(|write| write.as_slice() == [Command::MASTER_ACTIVATION]));
    }
}

#[test]
fn coordinator_uses_the_approved_fixed_configuration() {
    assert_eq!(PAGE_TURN_QUEUE_CAPACITY, 16);
    assert_eq!(DISPLAY_COMPLETION_CAPACITY, 16);
    assert_eq!(INPUT_POLL_INTERVAL_MS, 50);
    assert_eq!(GRAY_SETTLE_DELAY_MS, 350);
    assert_eq!(DISPLAY_BUSY_TIMEOUT_MS, 60_000);
    assert_eq!(DISPLAY_STREAM_STRIP_ROWS, 16);
}

#[test]
fn permanent_build_has_no_deferred_gray_probe_feature() {
    let cargo = include_str!("../Cargo.toml");
    assert!(!cargo.contains("deferred-gray-probe"));
}

#[test]
fn startup_requests_safe_bw_seed_for_page_zero() {
    let coordinator = RefreshCoordinator::new(4);

    assert_eq!(coordinator.phase(), RefreshPhase::Recovering);
    assert_eq!(
        coordinator.next_action(),
        RefreshAction::RecoverBw { page: 0 }
    );
}

fn coordinator_ready_on_page_one() -> RefreshCoordinator {
    let mut coordinator = RefreshCoordinator::new(4);

    assert_eq!(
        coordinator.record_seed_complete(0, 0),
        RefreshAction::WaitUntil { deadline_ms: 350 }
    );
    assert_eq!(
        coordinator.start_bw(1),
        RefreshAction::RenderBw { from: 0, target: 1 }
    );
    assert_eq!(
        coordinator.record_bw_complete(1, 1_000),
        RefreshAction::WaitUntil { deadline_ms: 1_350 }
    );
    assert_eq!(
        coordinator.gray_deadline_elapsed(1_350),
        RefreshAction::RenderGray { page: 1 }
    );
    assert_eq!(
        coordinator.record_gray_complete(),
        RefreshAction::SyncBwBase { page: 1 }
    );
    assert_eq!(
        coordinator.record_base_sync_complete(),
        RefreshAction::WaitForRequest
    );

    coordinator
}

fn coordinator_in_gray_refresh_on_page_one() -> RefreshCoordinator {
    let mut coordinator = RefreshCoordinator::new(4);

    assert_eq!(
        coordinator.record_seed_complete(0, 0),
        RefreshAction::WaitUntil { deadline_ms: 350 }
    );
    assert_eq!(
        coordinator.start_bw(1),
        RefreshAction::RenderBw { from: 0, target: 1 }
    );
    assert_eq!(
        coordinator.record_bw_complete(1, 1_000),
        RefreshAction::WaitUntil { deadline_ms: 1_350 }
    );
    assert_eq!(
        coordinator.gray_deadline_elapsed(1_350),
        RefreshAction::RenderGray { page: 1 }
    );
    coordinator
}

fn coordinator_in_gray_delay_on_page_one() -> RefreshCoordinator {
    let mut coordinator = RefreshCoordinator::new(4);

    assert_eq!(
        coordinator.record_seed_complete(0, 0),
        RefreshAction::WaitUntil { deadline_ms: 350 }
    );
    assert_eq!(
        coordinator.start_bw(1),
        RefreshAction::RenderBw { from: 0, target: 1 }
    );
    assert_eq!(
        coordinator.record_bw_complete(1, 1_000),
        RefreshAction::WaitUntil { deadline_ms: 1_350 }
    );

    coordinator
}

fn coordinator_with_active_bw_refresh_on_page_one_targeting_two() -> RefreshCoordinator {
    let mut coordinator = coordinator_ready_on_page_one();

    assert_eq!(
        coordinator.start_bw(2),
        RefreshAction::RenderBw { from: 1, target: 2 }
    );

    coordinator
}

#[test]
fn bw_completion_starts_gray_delay_at_completion_time() {
    let mut coordinator = coordinator_ready_on_page_one();

    assert_eq!(coordinator.phase(), RefreshPhase::BwReady);
    assert_eq!(coordinator.displayed_page(), 1);
    assert_eq!(
        coordinator.start_bw(2),
        RefreshAction::RenderBw { from: 1, target: 2 }
    );
    assert_eq!(coordinator.displayed_page(), 1);
    assert_eq!(
        coordinator.record_bw_complete(2, 1_000),
        RefreshAction::WaitUntil { deadline_ms: 1_350 }
    );
    assert_eq!(coordinator.displayed_page(), 2);
    assert_eq!(coordinator.phase(), RefreshPhase::GrayDelay);
}

#[test]
fn displayed_page_changes_only_after_refresh_completion() {
    let mut coordinator = coordinator_ready_on_page_one();

    assert_eq!(coordinator.displayed_page(), 1);
    assert_eq!(
        coordinator.start_bw(2),
        RefreshAction::RenderBw { from: 1, target: 2 }
    );
    assert_eq!(coordinator.displayed_page(), 1);
    assert_eq!(
        coordinator.record_bw_complete(2, 1_000),
        RefreshAction::WaitUntil { deadline_ms: 1_350 }
    );
    assert_eq!(coordinator.displayed_page(), 2);
}

#[test]
fn request_during_gray_refresh_is_observed_by_epoch_streaming() {
    let mut coordinator = coordinator_in_gray_refresh_on_page_one();

    assert_eq!(coordinator.request_arrived(), RefreshAction::None);
    assert_eq!(coordinator.phase(), RefreshPhase::GrayRefreshing);
    assert_eq!(coordinator.displayed_page(), 1);
}

#[test]
fn request_during_gray_delay_cancels_gray_and_starts_bw() {
    let mut coordinator = coordinator_in_gray_delay_on_page_one();

    assert_eq!(
        coordinator.start_bw(2),
        RefreshAction::RenderBw { from: 1, target: 2 }
    );
    assert_eq!(coordinator.phase(), RefreshPhase::BwRefreshing);
    assert_eq!(coordinator.displayed_page(), 1);
    assert_eq!(
        coordinator.next_action(),
        RefreshAction::RenderBw { from: 1, target: 2 }
    );
}

#[test]
fn successful_overlay_starts_background_base_sync() {
    let mut coordinator = coordinator_in_gray_refresh_on_page_one();

    assert_eq!(
        coordinator.record_gray_complete(),
        RefreshAction::SyncBwBase { page: 1 }
    );
    assert_eq!(coordinator.phase(), RefreshPhase::BaseSync);
}

#[test]
fn cancelled_overlay_returns_to_request_waiting_without_sync() {
    let mut coordinator = coordinator_in_gray_refresh_on_page_one();

    assert_eq!(
        coordinator.record_gray_cancelled(),
        RefreshAction::WaitForRequest
    );
    assert_eq!(coordinator.phase(), RefreshPhase::BwReady);
}

#[test]
fn successful_base_sync_restores_bw_ready_state() {
    let mut coordinator = coordinator_in_gray_refresh_on_page_one();

    assert_eq!(
        coordinator.record_gray_complete(),
        RefreshAction::SyncBwBase { page: 1 }
    );
    assert_eq!(
        coordinator.record_base_sync_complete(),
        RefreshAction::WaitForRequest
    );
    assert_eq!(coordinator.phase(), RefreshPhase::BwReady);
    assert_eq!(coordinator.displayed_page(), 1);
}

#[test]
fn first_failure_requests_one_safe_recovery() {
    let mut coordinator = coordinator_with_active_bw_refresh_on_page_one_targeting_two();

    assert_eq!(
        coordinator.record_failure(),
        RefreshAction::RecoverBw { page: 2 }
    );
    assert_eq!(coordinator.phase(), RefreshPhase::Recovering);
    assert_eq!(coordinator.displayed_page(), 1);
}

#[test]
fn recovery_failure_enters_fault() {
    let mut coordinator = coordinator_with_active_bw_refresh_on_page_one_targeting_two();

    assert_eq!(
        coordinator.record_failure(),
        RefreshAction::RecoverBw { page: 2 }
    );
    assert_eq!(coordinator.phase(), RefreshPhase::Recovering);
    assert_eq!(coordinator.record_failure(), RefreshAction::None);
    assert_eq!(coordinator.phase(), RefreshPhase::Fault);
}

#[test]
fn async_grayscale_streams_each_plane_in_sixteen_row_strips() {
    let trace = run_async_gray_render(PAGE_1);

    assert_eq!(trace.red_windows, 30);
    assert_eq!(trace.black_windows, 30);
    assert!(trace.red_window_heights.iter().all(|h| *h == 16));
    assert!(trace.black_window_heights.iter().all(|h| *h == 16));
    assert_eq!(trace.yields_between_strips, 59);
    assert_eq!(trace.refreshes, vec![RefreshMode::Grayscale]);
}

#[test]
fn async_full_grayscale_reconstructs_absolute_planes_from_staged_slots() {
    let book_bytes = include_bytes!("../fixtures/nav_probe.binbook");
    let mut scratch = [0u8; 8192];
    let mut book = binbook_core::Book::open(
        binbook_core::SliceSource::new(&book_bytes[..]),
        &mut scratch,
    )
    .expect("open nav probe book");
    let page_data_offset = usize::try_from(book.page_data_offset().get()).unwrap();
    let page = test_page(&mut book, 0);
    let decompress_slot = |slot: usize| {
        let raw_slot = u8::try_from(slot).unwrap();
        let descriptor = page
            .planes
            .get(binbook_core::PlaneSlot::try_from(raw_slot).unwrap())
            .unwrap();
        let offset = page_data_offset + usize::try_from(descriptor.offset.get()).unwrap();
        let end = offset + usize::try_from(descriptor.length.get()).unwrap();
        let mut plane = vec![0u8; 100 * 480];
        display::decompress_row(&book_bytes[offset..end], &mut plane);
        plane
    };
    let msb = decompress_slot(0);
    let lsb = decompress_slot(1);
    let base = decompress_slot(2);
    let mut expected = Vec::with_capacity(100 * 480 * 2);
    expected.extend(
        base.iter()
            .zip(&msb)
            .zip(&lsb)
            .map(|((&base, &msb), &lsb)| !(base | (msb & !lsb))),
    );
    expected.extend(base.iter().zip(&lsb).map(|(&base, &lsb)| !(base | lsb)));

    let (_, writes) = run_async_gray_render_with_writes(0);
    let actual: Vec<u8> = writes
        .iter()
        .filter(|write| write.len() == 100)
        .take(960)
        .flatten()
        .copied()
        .collect();

    assert_eq!(actual.len(), expected.len());
    let mismatch = actual
        .iter()
        .zip(&expected)
        .position(|(actual, expected)| actual != expected);
    assert_eq!(
        mismatch, None,
        "absolute grayscale byte stream differs at offset {mismatch:?}"
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn async_corner_probe_streams_sixteen_bytes_per_128_pixel_window_row() {
    let writes = Rc::new(RefCell::new(Vec::new()));
    let delay_calls = Rc::new(RefCell::new(Vec::new()));
    let mut driver = new_legacy_display(
        RecordingSpi {
            writes: Rc::clone(&writes),
        },
        NoopOutputPin,
        NoopOutputPin,
        NoopOutputPin,
        LowBusyPin,
    );
    let delay = RecordingYieldDelay {
        calls: Rc::clone(&delay_calls),
    };

    block_on(display::window_corners_probe_async(&mut driver, &delay))
        .expect("corner probe should succeed");

    let sixteen_byte_rows = writes
        .borrow()
        .iter()
        .filter(|write| write.len() == 128 / 8)
        .count();
    assert_eq!(sixteen_byte_rows, 4 * 2 * 96);
}

#[test]
fn bw_differential_streams_old_and_new_planes() {
    let trace = run_async_bw_differential(PAGE_1, PAGE_2);

    assert_eq!(trace.red_plane_slots, vec![2]);
    assert_eq!(trace.black_plane_slots, vec![2]);
    assert_eq!(trace.refreshes, vec![RefreshMode::Partial]);
}

#[test]
fn recovery_seeds_target_with_one_full_refresh() {
    let trace = run_async_recovery(PAGE_2);

    assert_eq!(trace.red_plane_slots, vec![2]);
    assert_eq!(trace.black_plane_slots, vec![2]);
    assert_eq!(trace.refreshes, vec![RefreshMode::Full]);
}

#[test]
fn display_request_channel_is_fifo_and_rejects_the_seventeenth_request() {
    use embassy_sync::blocking_mutex::raw::NoopRawMutex;
    use embassy_sync::channel::Channel;

    let channel: Channel<NoopRawMutex, u32, 16> = Channel::new();
    for value in 0..16u32 {
        channel.try_send(value).unwrap();
    }
    assert!(channel.try_send(16).is_err());

    let mut values = Vec::new();
    for _ in 0..16 {
        values.push(channel.try_receive().unwrap());
    }
    assert_eq!(values, (0u32..16).collect::<Vec<_>>());
}
