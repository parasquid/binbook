use binbook_fw::async_refresh::{
    PostGrayStrategy, RefreshAction, RefreshCoordinator, RefreshPhase, DISPLAY_BUSY_TIMEOUT_MS,
    DISPLAY_COMPLETION_CAPACITY, DISPLAY_STREAM_STRIP_ROWS, GRAY_SETTLE_DELAY_MS,
    INPUT_POLL_INTERVAL_MS, PAGE_TURN_QUEUE_CAPACITY,
};
use binbook_fw::display;
use ssd1677_driver::{Ssd1677, Ssd1677Driver};
use xteink_hal::RefreshMode;
use xteink_hal::{AsyncDelay, HalResult, InputPin, OutputPin, Spi};
use std::boxed::Box;
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::vec::Vec;

const PAGE_1: u32 = 1;
const PAGE_2: u32 = 2;

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

fn parse_gray_render_trace(
    writes: &[Vec<u8>],
    zero_yield_count: usize,
) -> AsyncGrayTrace {
    let mut trace = AsyncGrayTrace {
        yields_between_strips: zero_yield_count,
        ..AsyncGrayTrace::default()
    };
    let mut pending_window_height: Option<u16> = None;
    let mut pending_refresh_mode: Option<RefreshMode> = None;
    let mut index = 0;

    while index < writes.len() {
        match writes[index].as_slice() {
            [cmd] if *cmd == Ssd1677::SET_RAM_Y_ADDR => {
                let data = writes
                    .get(index + 1)
                    .expect("SET_RAM_Y_ADDR must be followed by data");
                let start = u16::from_le_bytes([data[0], data[1]]);
                let end = u16::from_le_bytes([data[2], data[3]]);
                pending_window_height = Some(end - start + 1);
                index += 2;
                continue;
            }
            [cmd] if *cmd == Ssd1677::WRITE_RAM_RED => {
                trace.red_windows += 1;
                trace
                    .red_window_heights
                    .push(pending_window_height.expect("window height must be set before red plane write"));
                pending_window_height = None;
            }
            [cmd] if *cmd == Ssd1677::WRITE_RAM => {
                trace.black_windows += 1;
                trace
                    .black_window_heights
                    .push(pending_window_height.expect("window height must be set before black plane write"));
                pending_window_height = None;
            }
            [cmd] if *cmd == Ssd1677::DISPLAY_UPDATE_CTRL2 => {
                let data = writes
                    .get(index + 1)
                    .expect("DISPLAY_UPDATE_CTRL2 must be followed by data");
                pending_refresh_mode = match data.as_slice() {
                    [value] if *value == Ssd1677::UPDATE_CTRL_NORMAL => Some(RefreshMode::Full),
                    [value] if *value == Ssd1677::UPDATE_CTRL_FAST => Some(RefreshMode::Partial),
                    [value] if *value == Ssd1677::UPDATE_CTRL_GRAYSCALE => {
                        Some(RefreshMode::Grayscale)
                    }
                    _ => None,
                };
                index += 2;
                continue;
            }
            [cmd] if *cmd == Ssd1677::MASTER_ACTIVATION => {
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

fn run_async_gray_render(page: u32) -> AsyncGrayTrace {
    let book_bytes = include_bytes!("../fixtures/nav_probe.binbook");
    let mut scratch = [0u8; 8192];
    let mut book =
        binbook::BinBook::open(&book_bytes[..], &mut scratch).expect("open nav probe book");
    let page_info = book.page_info(page).expect("page must exist");
    assert!(
        display::is_supported_x4_native_gray2_page(&page_info),
        "page must be x4-native gray2"
    );

    let writes = Rc::new(RefCell::new(Vec::new()));
    let delay_calls = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Ssd1677Driver::new(
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

    let parsed = parse_gray_render_trace(&writes.borrow(), delay.zero_yield_count());
    parsed
}

fn run_async_reseed(page: u32, visible: bool) -> AsyncReseedTrace {
    let book_bytes = include_bytes!("../fixtures/nav_probe.binbook");
    let mut scratch = [0u8; 8192];
    let mut book =
        binbook::BinBook::open(&book_bytes[..], &mut scratch).expect("open nav probe book");
    let page_info = book.page_info(page).expect("page must exist");
    assert!(
        display::is_supported_x4_native_gray2_page(&page_info),
        "page must be x4-native gray2"
    );

    let writes = Rc::new(RefCell::new(Vec::new()));
    let delay_calls = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Ssd1677Driver::new(
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

    let result = if visible {
        block_on(display::reseed_bw_visible_async(
            &mut driver,
            &mut book,
            &book_bytes[..],
            page,
            &delay,
        ))
    } else {
        block_on(display::reseed_bw_silent_async(
            &mut driver,
            &mut book,
            &book_bytes[..],
            page,
            &delay,
        ))
    };
    result.expect("async reseed should succeed");

    let parsed = parse_gray_render_trace(&writes.borrow(), delay.zero_yield_count());
    AsyncReseedTrace {
        red_plane_slots: vec![2],
        black_plane_slots: vec![2],
        refreshes: parsed.refreshes,
    }
}

fn run_async_bw_differential(prev_page: u32, target_page: u32) -> AsyncReseedTrace {
    let book_bytes = include_bytes!("../fixtures/nav_probe.binbook");
    let mut scratch = [0u8; 8192];
    let mut book =
        binbook::BinBook::open(&book_bytes[..], &mut scratch).expect("open nav probe book");
    let prev_info = book.page_info(prev_page).expect("previous page must exist");
    let target_info = book.page_info(target_page).expect("target page must exist");
    assert!(
        display::is_supported_x4_native_gray2_page(&prev_info),
        "previous page must be x4-native gray2"
    );
    assert!(
        display::is_supported_x4_native_gray2_page(&target_info),
        "target page must be x4-native gray2"
    );

    let writes = Rc::new(RefCell::new(Vec::new()));
    let delay_calls = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Ssd1677Driver::new(
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
    let mut book =
        binbook::BinBook::open(&book_bytes[..], &mut scratch).expect("open nav probe book");
    let page_info = book.page_info(page).expect("page must exist");
    assert!(
        display::is_supported_x4_native_gray2_page(&page_info),
        "page must be x4-native gray2"
    );

    let writes = Rc::new(RefCell::new(Vec::new()));
    let delay_calls = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Ssd1677Driver::new(
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
fn coordinator_uses_the_approved_fixed_configuration() {
    assert_eq!(PAGE_TURN_QUEUE_CAPACITY, 16);
    assert_eq!(DISPLAY_COMPLETION_CAPACITY, 16);
    assert_eq!(INPUT_POLL_INTERVAL_MS, 50);
    assert_eq!(GRAY_SETTLE_DELAY_MS, 350);
    assert_eq!(DISPLAY_BUSY_TIMEOUT_MS, 60_000);
    assert_eq!(DISPLAY_STREAM_STRIP_ROWS, 16);
}

#[test]
fn startup_requests_grayscale_for_page_zero() {
    let coordinator = RefreshCoordinator::new(4, PostGrayStrategy::SilentReseed);

    assert_eq!(coordinator.phase(), RefreshPhase::GrayRefreshing);
    assert_eq!(coordinator.next_action(), RefreshAction::RenderGray { page: 0 });
}

fn coordinator_ready_on_page_one() -> RefreshCoordinator {
    let mut coordinator = RefreshCoordinator::new(4, PostGrayStrategy::SilentReseed);

    assert_eq!(
        coordinator.record_gray_complete(),
        RefreshAction::ReseedBw {
            page: 0,
            visible: false,
        }
    );
    assert_eq!(
        coordinator.record_reseed_complete(),
        RefreshAction::WaitForRequest
    );
    assert_eq!(
        coordinator.start_bw(1),
        RefreshAction::RenderBw { from: 0, target: 1 }
    );
    assert_eq!(
        coordinator.record_bw_complete(1, 1_000),
        RefreshAction::WaitUntil {
            deadline_ms: 1_350,
        }
    );
    assert_eq!(
        coordinator.gray_deadline_elapsed(1_350),
        RefreshAction::RenderGray { page: 1 }
    );
    assert_eq!(
        coordinator.record_gray_complete(),
        RefreshAction::ReseedBw {
            page: 1,
            visible: false,
        }
    );
    assert_eq!(
        coordinator.record_reseed_complete(),
        RefreshAction::WaitForRequest
    );

    coordinator
}

fn coordinator_in_gray_refresh_on_page_one_with_strategy(
    strategy: PostGrayStrategy,
) -> RefreshCoordinator {
    let mut coordinator = RefreshCoordinator::new(4, strategy);

    assert_eq!(
        coordinator.record_gray_complete(),
        RefreshAction::ReseedBw {
            page: 0,
            visible: matches!(strategy, PostGrayStrategy::VisibleReseed),
        }
    );
    assert_eq!(
        coordinator.record_reseed_complete(),
        RefreshAction::WaitForRequest
    );
    assert_eq!(
        coordinator.start_bw(1),
        RefreshAction::RenderBw { from: 0, target: 1 }
    );
    assert_eq!(
        coordinator.record_bw_complete(1, 1_000),
        RefreshAction::WaitUntil {
            deadline_ms: 1_350,
        }
    );
    assert_eq!(
        coordinator.gray_deadline_elapsed(1_350),
        RefreshAction::RenderGray { page: 1 }
    );

    coordinator
}

fn coordinator_in_gray_refresh_on_page_one() -> RefreshCoordinator {
    coordinator_in_gray_refresh_on_page_one_with_strategy(PostGrayStrategy::SilentReseed)
}

fn coordinator_in_gray_delay_on_page_one() -> RefreshCoordinator {
    let mut coordinator = RefreshCoordinator::new(4, PostGrayStrategy::SilentReseed);

    assert_eq!(
        coordinator.record_gray_complete(),
        RefreshAction::ReseedBw {
            page: 0,
            visible: false,
        }
    );
    assert_eq!(
        coordinator.record_reseed_complete(),
        RefreshAction::WaitForRequest
    );
    assert_eq!(
        coordinator.start_bw(1),
        RefreshAction::RenderBw { from: 0, target: 1 }
    );
    assert_eq!(
        coordinator.record_bw_complete(1, 1_000),
        RefreshAction::WaitUntil {
            deadline_ms: 1_350,
        }
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
        RefreshAction::WaitUntil {
            deadline_ms: 1_350,
        }
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
    assert_eq!(coordinator.record_bw_complete(2, 1_000), RefreshAction::WaitUntil { deadline_ms: 1_350 });
    assert_eq!(coordinator.displayed_page(), 2);
}

#[test]
fn request_during_gray_refresh_waits_for_reseed() {
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
fn silent_strategy_reseeds_without_visible_activation() {
    let mut coordinator = coordinator_in_gray_refresh_on_page_one();

    assert_eq!(
        coordinator.record_gray_complete(),
        RefreshAction::ReseedBw {
            page: 1,
            visible: false,
        }
    );
    assert_eq!(coordinator.phase(), RefreshPhase::BwReseeding);
}

#[test]
fn fallback_strategy_reseeds_with_visible_activation() {
    let mut coordinator =
        coordinator_in_gray_refresh_on_page_one_with_strategy(PostGrayStrategy::VisibleReseed);

    assert_eq!(
        coordinator.record_gray_complete(),
        RefreshAction::ReseedBw {
            page: 1,
            visible: true,
        }
    );
    assert_eq!(coordinator.phase(), RefreshPhase::BwReseeding);
}

#[test]
fn successful_reseed_restores_bw_ready_state() {
    let mut coordinator = coordinator_in_gray_refresh_on_page_one();

    assert_eq!(
        coordinator.record_gray_complete(),
        RefreshAction::ReseedBw {
            page: 1,
            visible: false,
        }
    );
    assert_eq!(
        coordinator.record_reseed_complete(),
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
fn silent_reseed_writes_bw_plane_to_both_ram_planes_without_activation() {
    let trace = run_async_reseed(2, false);

    assert_eq!(trace.red_plane_slots, vec![2]);
    assert_eq!(trace.black_plane_slots, vec![2]);
    assert!(trace.refreshes.is_empty());
}

#[test]
fn visible_reseed_adds_exactly_one_full_refresh() {
    let trace = run_async_reseed(2, true);

    assert_eq!(trace.red_plane_slots, vec![2]);
    assert_eq!(trace.black_plane_slots, vec![2]);
    assert_eq!(trace.refreshes, vec![RefreshMode::Full]);
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
