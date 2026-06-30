# Xteink X4 Async Deferred-Grayscale Refresh Implementation Plan

> Historical implementation plan. Its pre-refactor crate paths are retained as
> milestone context; current boundaries are in
> [`2026-06-30-rust-modular-foundation-refactor.md`](2026-06-30-rust-modular-foundation-refactor.md).

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` and execute this plan sequentially without subagents. Keep this checkbox list current. Do not begin a production-code step until its preceding test has been observed failing for the expected reason.

**Goal:** Make every page turn appear quickly in black and white, render grayscale after 350 ms without further turns, preserve every queued intermediate turn, and keep button and diagnostic handling responsive throughout display work.

**Architecture:** Migrate the firmware binary to the `esp-rtos` Embassy executor with separate input, display, and diagnostic tasks. A fixed-capacity request channel feeds a host-testable refresh coordinator; the display task owns the SSD1677 and streams bounded strips while yielding. Both post-grayscale strategies are implemented before hardware work: silent BW RAM reseeding is the primary hardware experiment, and an immediate visible BW reseed is the permanent fallback.

**Tech Stack:** Rust `no_std`, pinned nightly Rust, `esp-hal 1.1.1`, `esp-rtos 0.3`, Embassy executor/sync/time crates compatible with `esp-rtos 0.3`, SSD1677, existing BinBook GRAY2 streaming, COBS diagnostic protocol, Cargo host tests, Xteink X4 hardware.

## Review Repair Status (2026-06-28)

The original implementation was rejected because the device runtime bypassed
coordinator behavior, discarded display errors, ignored asynchronous probes,
matched completions FIFO, and emitted synthetic phase/reseed records. The repair
is implemented as follows:

- `runtime_engine.rs` is the shared no-allocation display engine with an
  injectable backend and behavioral failure/recovery tests.
- `runtime_aggregator.rs` owns pending sequence reservations, diagnostic state,
  and logs; the firmware spawns it with a 32-entry `RuntimeEvent` channel.
- `runtime.rs` uses the engine for the real SSD1677 path and routes STATUS,
  LOG_GET, LOG_CLEAR, and sequence-matched completions through the aggregator.
- all probes execute asynchronously and invalidate differential readiness;
- `diag exercise deferred-gray` validates structured event content, timestamps,
  page order, sequences, reseed boundaries, and error counters.

Host implementation, the clean host matrix, and host-side adversarial review
are complete. Permanent strategy selection, final flash, serial runbook,
five-point webcam verdict, clear-white crop calibration, and device-side
adversarial acceptance review remain completion gates. The repaired probe flash
was attempted but rejected before execution because the approval service's
account escalation limit was exhausted. Earlier hardware output from the
rejected runtime is not evidence for this repaired implementation.

---

## Non-Negotiable Execution Rules

- Use strict red-green-refactor for Tasks 1 through 7: write one behavior test, run it and confirm the expected failure, add only enough production code to pass, rerun the focused suite, then refactor while green.
- If a new test passes before production code changes, strengthen or correct the test until it fails because the required behavior is absent.
- Do not add production code and retroactively add tests. Delete any prematurely written production code and restart that behavior from RED.
- Run no flash, USB, serial, webcam, SD-card, or other hardware command before Task 9.
- Keep the reusable `xteink-hal` and `ssd1677-driver` crates independent of Embassy. Embassy belongs in the firmware application integration.
- Keep display memory bounded: no 48,000-byte plane buffers, no 96,000-byte dual-plane buffers, and no full framebuffer.
- Keep the diagnostic protocol binary. Do not add JSON, CBOR, protobuf, or text control messages.
- Preserve `debug-log` compilation behavior and diagnostic-console ownership of USB Serial/JTAG.
- Use the SSD1677's documented 20 MHz maximum write clock, not the 40 MHz shared-bus metadata ceiling.
- Treat the webcam verdict as required evidence. A successful command response does not prove visible correctness.

## Fixed Decisions And Constants

```rust
pub const PAGE_TURN_QUEUE_CAPACITY: usize = 16;
pub const DISPLAY_COMPLETION_CAPACITY: usize = 16;
pub const INPUT_POLL_INTERVAL_MS: u64 = 50;
pub const INPUT_COOLDOWN_MS: u32 = 100;
pub const GRAY_SETTLE_DELAY_MS: u64 = 350;
pub const DISPLAY_BUSY_TIMEOUT_MS: u64 = 60_000;
pub const DISPLAY_STREAM_STRIP_ROWS: u16 = 16;
pub const DISPLAY_SPI_FREQUENCY_MHZ: u32 = 20;
```

- Queue overflow rejects and logs the newest request; existing queued requests remain ordered.
- Directional button presses and diagnostic KEY presses queue `PageTurn` values, not pre-resolved page numbers.
- Every queued turn is applied when dequeued to the last successfully displayed page, so `Next, Next, Previous` visibly renders each intermediate page.
- Boundary turns are consumed without a panel refresh and complete successfully at the unchanged page.
- Grayscale starts only after the queue is empty, the last BW refresh has completed, and 350 ms has elapsed without another request.
- An active e-ink waveform is never reset or aborted. Requests received during it remain queued.
- The primary post-gray strategy writes the current page's BW plane into both controller RAM planes without master activation.
- The fallback performs the same writes and immediately triggers a visible full BW refresh.
- The firmware cannot detect visual failure of silent reseeding. The user webcam verdict selects one permanent compile-time strategy.

## Planned File Boundaries

- Modify `firmware/crates/xteink-hal/src/lib.rs`: add an Embassy-independent asynchronous delay contract.
- Modify `firmware/crates/ssd1677-driver/src/lib.rs`: split refresh activation from BUSY waiting and add generic asynchronous init/wait wrappers while preserving blocking APIs.
- Create `firmware/crates/binbook-fw/src/async_refresh.rs`: request types, refresh phases, coordinator state transitions, constants, recovery policy, and strategy selection.
- Modify `firmware/crates/binbook-fw/src/display.rs`: bounded-strip async streaming, grayscale preparation, silent reseed, visible reseed, and recovery helpers.
- Modify `firmware/crates/binbook-fw/src/main.rs`: Embassy startup plus input, display, and diagnostic tasks.
- Modify `firmware/crates/binbook-fw/src/diag.rs` and `diag_log.rs`: queued KEY semantics, multiple pending completions, shared snapshots, and structured phase/queue events.
- Modify `firmware/crates/binbook-diagnostic-protocol/src/lib.rs`: add event codes only; keep protocol version and STATUS payload unchanged.
- Modify `cli/src/lib.rs` and `cli/src/main.rs`: batched request/response transport and `diag exercise deferred-gray`.
- Add focused host tests under `firmware/crates/binbook-fw/tests/async_refresh.rs`; retain existing regressions in `firmware_logic.rs` and driver-local tests.
- Update the current-state reference/spec documents and replace `HANDOFF.md` only after observed hardware evidence exists.

### Task 1: Add Async HAL And Nonblocking SSD1677 Primitives

**Files:**
- Modify: `firmware/crates/xteink-hal/src/lib.rs`
- Modify: `firmware/crates/ssd1677-driver/src/lib.rs`
- Test: `firmware/crates/ssd1677-driver/src/lib.rs`

- [ ] **Step 1: Write failing driver tests for separated activation and BUSY state**

Add tests using the existing mock SPI and mock pins:

```rust
#[test]
fn trigger_refresh_sends_activation_without_waiting_for_busy() {
    let (mut driver, trace, busy_reads) = traced_driver_with_busy_high();

    driver.trigger_refresh(RefreshMode::Grayscale).unwrap();

    assert_eq!(busy_reads.get(), 0);
    assert!(trace.borrow().windows(2).any(|w| w == [0x22, 0xC7]));
    assert!(trace.borrow().contains(&0x20));
}

#[test]
fn is_busy_reports_the_input_pin_without_spi_traffic() {
    let (driver, trace, _) = traced_driver_with_busy_high();

    assert_eq!(driver.is_busy().unwrap(), true);
    assert!(trace.borrow().is_empty());
}
```

Use the actual grayscale update byte already defined by `UPDATE_CTRL_GRAYSCALE`; do not duplicate a new numeric command constant in the test helper.

- [ ] **Step 2: Run the focused tests and verify RED**

Run:

```bash
cd firmware
cargo test -p ssd1677-driver trigger_refresh_sends_activation_without_waiting_for_busy
cargo test -p ssd1677-driver is_busy_reports_the_input_pin_without_spi_traffic
```

Expected: compilation fails because `trigger_refresh` and `is_busy` do not exist. A failure caused by an incorrect mock or assertion does not satisfy RED.

- [ ] **Step 3: Add the minimal nonblocking driver API**

Add these public methods and make `refresh_with_delay` call them:

```rust
pub fn trigger_refresh(&mut self, mode: RefreshMode) -> HalResult<()>;
pub fn is_busy(&self) -> HalResult<bool>;

pub fn refresh_with_delay(
    &mut self,
    mode: RefreshMode,
    delay: &dyn Delay,
) -> HalResult<()> {
    self.trigger_refresh(mode)?;
    self.wait_ready_with_delay(delay)
}
```

`trigger_refresh` must send the existing mode-specific `0x21`, `0x22`, and `0x20` sequence and return without reading BUSY.

- [ ] **Step 4: Verify GREEN and existing driver compatibility**

Run:

```bash
cd firmware
cargo test -p ssd1677-driver
```

Expected: all existing and new driver tests pass.

- [ ] **Step 5: Write failing tests for asynchronous wait success and timeout**

Add `futures = { version = "0.3", features = ["executor"] }` under `ssd1677-driver` dev-dependencies. Define a mock async delay that records each awaited millisecond and changes the mock BUSY state after a configured count.

```rust
#[test]
fn async_wait_yields_until_busy_clears() {
    let (mut driver, delay) = async_busy_driver(3);

    futures::executor::block_on(driver.wait_ready_async(&delay)).unwrap();

    assert_eq!(delay.awaited_milliseconds(), 3);
}

#[test]
fn async_wait_times_out_at_the_named_limit() {
    let (mut driver, delay) = permanently_busy_async_driver();

    let result = futures::executor::block_on(driver.wait_ready_async(&delay));

    assert_eq!(result, Err(HalError::Timeout));
    assert_eq!(delay.awaited_milliseconds(), BUSY_TIMEOUT_MS);
}
```

- [ ] **Step 6: Run asynchronous tests and verify RED**

Run:

```bash
cd firmware
cargo test -p ssd1677-driver async_wait -- --nocapture
```

Expected: compilation fails because `AsyncDelay` and `wait_ready_async` do not exist.

- [ ] **Step 7: Add the generic async delay and driver methods**

Add to `xteink-hal`:

```rust
pub trait AsyncDelay {
    async fn ms(&self, ms: u32);
}
```

Add to `ssd1677-driver`:

```rust
pub async fn wait_ready_async<D: AsyncDelay>(&mut self, delay: &D) -> HalResult<()>;
pub async fn init_async<D: AsyncDelay>(&mut self, delay: &D) -> HalResult<()>;
pub async fn init_grayscale_async<D: AsyncDelay>(&mut self, delay: &D) -> HalResult<()>;
pub async fn refresh_async<D: AsyncDelay>(
    &mut self,
    mode: RefreshMode,
    delay: &D,
) -> HalResult<()>;
```

Use the same reset timings, register writes, LUT bytes, and timeout as the existing blocking paths. Factor shared command-only portions into private helpers so blocking and async paths cannot drift.

- [ ] **Step 8: Verify GREEN and independent crate builds**

Run:

```bash
cd firmware
cargo test -p xteink-hal
cargo test -p ssd1677-driver
```

Expected: both crates compile independently and all tests pass.

- [ ] **Step 9: Commit Task 1**

```bash
git add firmware/crates/xteink-hal/src/lib.rs firmware/crates/ssd1677-driver/Cargo.toml firmware/crates/ssd1677-driver/src/lib.rs firmware/Cargo.lock
git commit -m "feat(firmware): add async SSD1677 control primitives"
```

### Task 2: Build The Host-Testable Refresh Coordinator

**Files:**
- Create: `firmware/crates/binbook-fw/src/async_refresh.rs`
- Modify: `firmware/crates/binbook-fw/src/lib.rs`
- Create: `firmware/crates/binbook-fw/tests/async_refresh.rs`

- [ ] **Step 1: Write failing tests for constants, phases, and startup**

Define the expected public API in tests:

```rust
use binbook_fw::async_refresh::{
    PostGrayStrategy, RefreshAction, RefreshCoordinator, RefreshPhase,
    DISPLAY_BUSY_TIMEOUT_MS, DISPLAY_COMPLETION_CAPACITY, DISPLAY_STREAM_STRIP_ROWS,
    GRAY_SETTLE_DELAY_MS, INPUT_POLL_INTERVAL_MS, PAGE_TURN_QUEUE_CAPACITY,
};

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
```

- [ ] **Step 2: Run tests and verify RED**

Run:

```bash
cd firmware
cargo test -p binbook-fw --test async_refresh coordinator_ -- --nocapture
```

Expected: compilation fails because the module and types do not exist.

- [ ] **Step 3: Implement only the constants, enums, and startup state**

Define:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostGrayStrategy {
    SilentReseed,
    VisibleReseed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshPhase {
    BwReady,
    BwRefreshing,
    GrayDelay,
    GrayRefreshing,
    BwReseeding,
    Recovering,
    Fault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshAction {
    RenderBw { from: u32, target: u32 },
    RenderGray { page: u32 },
    ReseedBw { page: u32, visible: bool },
    RecoverBw { page: u32 },
    WaitForRequest,
    WaitUntil { deadline_ms: u64 },
    None,
}
```

Implement only enough coordinator state to pass the startup tests.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cd firmware
cargo test -p binbook-fw --test async_refresh coordinator_
```

Expected: the focused tests pass.

- [ ] **Step 5: Add RED tests for the complete state sequence**

Add separate tests with these exact preconditions and assertions:

| Test | Preconditions | Required assertions |
| --- | --- | --- |
| `bw_completion_starts_gray_delay_at_completion_time` | Page 1 is BW-ready; start page 2; report BW completion at tick 1000 | Page 1 remains displayed before completion; page 2 becomes displayed afterward; phase is `GrayDelay`; action is `WaitUntil { deadline_ms: 1350 }` |
| `request_during_gray_delay_cancels_gray_and_starts_bw` | Page 1 completed at tick 1000 with deadline 1350; start page 2 at tick 1200 | No grayscale action is emitted; phase becomes `BwRefreshing`; action is `RenderBw { from: 1, target: 2 }` |
| `request_during_gray_refresh_waits_for_reseed` | Page 1 is in `GrayRefreshing`; call `request_arrived` | Action is `None`; phase remains `GrayRefreshing`; displayed page remains 1 |
| `silent_strategy_reseeds_without_visible_activation` | Page 1 uses `SilentReseed`; report gray completion | Action is `ReseedBw { page: 1, visible: false }`; phase is `BwReseeding` |
| `fallback_strategy_reseeds_with_visible_activation` | Page 1 uses `VisibleReseed`; report gray completion | Action is `ReseedBw { page: 1, visible: true }`; phase is `BwReseeding` |
| `successful_reseed_restores_bw_ready_state` | Page 1 is in `BwReseeding`; report reseed completion | Phase is `BwReady`; displayed page remains 1; action is `WaitForRequest` |
| `first_failure_requests_one_safe_recovery` | Page 2 BW refresh is active; report failure | Action is `RecoverBw { page: 2 }`; phase is `Recovering`; displayed page is still 1 |
| `recovery_failure_enters_fault` | Page 2 recovery is active; report another failure | Action is `None`; phase is `Fault`; no second recovery is requested |
| `displayed_page_changes_only_after_refresh_completion` | Page 1 is BW-ready; start page 2 | Displayed page remains 1 until `record_bw_complete(2, now)` succeeds |

Use real coordinator methods and exact phase/action assertions. Do not assert private fields or duplicate the transition logic in test helpers.

- [ ] **Step 6: Run the sequence tests and verify RED**

Run:

```bash
cd firmware
cargo test -p binbook-fw --test async_refresh -- --nocapture
```

Expected: each newly introduced behavior fails because its transition is absent. Work one failing test at a time.

- [ ] **Step 7: Implement transitions one test at a time**

Expose event methods with explicit timestamps and results:

```rust
pub fn start_bw(&mut self, target: u32) -> RefreshAction;
pub fn record_bw_complete(&mut self, target: u32, now_ms: u64) -> RefreshAction;
pub fn request_arrived(&mut self) -> RefreshAction;
pub fn gray_deadline_elapsed(&mut self, now_ms: u64) -> RefreshAction;
pub fn record_gray_complete(&mut self) -> RefreshAction;
pub fn record_reseed_complete(&mut self) -> RefreshAction;
pub fn record_failure(&mut self) -> RefreshAction;
pub fn record_recovery_complete(&mut self, page: u32, now_ms: u64) -> RefreshAction;
```

After each method is minimally implemented, rerun only its failing test, then rerun the whole `async_refresh` test target.

- [ ] **Step 8: Verify all coordinator tests and regressions**

Run:

```bash
cd firmware
cargo test -p binbook-fw --test async_refresh
cargo test -p binbook-fw --test firmware_logic
```

Expected: both test binaries pass.

- [ ] **Step 9: Commit Task 2**

```bash
git add firmware/crates/binbook-fw/src/async_refresh.rs firmware/crates/binbook-fw/src/lib.rs firmware/crates/binbook-fw/tests/async_refresh.rs
git commit -m "feat(firmware): add deferred refresh coordinator"
```

### Task 3: Add Bounded Async Display Streaming And Both Reseed Strategies

**Files:**
- Modify: `firmware/crates/binbook-fw/src/display.rs`
- Modify: `firmware/crates/binbook-fw/tests/async_refresh.rs`
- Test: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Write RED tests for bounded strip streaming**

Use a fake SSD1677 sink that records windows, RAM-plane writes, refresh activations, and explicit yield points. Require 30 strips per 480-row plane:

```rust
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
```

Add a payload fixture whose compressed plane exceeds the 8 KiB scratch size so the test proves streaming rather than accidental materialization.

- [ ] **Step 2: Run the strip test and verify RED**

Run:

```bash
cd firmware
cargo test -p binbook-fw --test async_refresh async_grayscale_streams_each_plane_in_sixteen_row_strips -- --exact
```

Expected: compilation fails because the async strip render helper does not exist.

- [ ] **Step 3: Implement sequential strip decoding and cooperative yielding**

Keep one `PackBitsStream` per plane alive across all 30 strips. For each strip:

1. Set the 800x16 physical window.
2. Stream sixteen 100-byte rows using the existing row scratch buffer.
3. Release chip select normally.
4. Await one cooperative yield before the next strip.

Do not restart PackBits decoding from row zero for every strip.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cd firmware
cargo test -p binbook-fw --test async_refresh async_grayscale_streams_each_plane_in_sixteen_row_strips -- --exact
```

Expected: the strip counts, row heights, yields, and one grayscale activation pass.

- [ ] **Step 5: Write RED tests that discriminate silent and visible reseeding**

```rust
#[test]
fn silent_reseed_writes_bw_plane_to_both_ram_planes_without_activation() {
    let trace = run_async_reseed(PAGE_2, false);

    assert_eq!(trace.red_plane_slots, vec![2]);
    assert_eq!(trace.black_plane_slots, vec![2]);
    assert!(trace.refreshes.is_empty());
}

#[test]
fn visible_reseed_adds_exactly_one_full_refresh() {
    let trace = run_async_reseed(PAGE_2, true);

    assert_eq!(trace.red_plane_slots, vec![2]);
    assert_eq!(trace.black_plane_slots, vec![2]);
    assert_eq!(trace.refreshes, vec![RefreshMode::Full]);
}
```

- [ ] **Step 6: Run reseed tests and verify RED**

Run:

```bash
cd firmware
cargo test -p binbook-fw --test async_refresh reseed -- --nocapture
```

Expected: both tests fail because no reseed API exists.

- [ ] **Step 7: Implement the two post-gray paths**

Add one shared BW-plane streaming function and two thin policy wrappers named `reseed_bw_silent` and `reseed_bw_visible`. Both take the existing display, BinBook reader, book bytes, delay/yield adapter, panel mode, and page index arguments used by the asynchronous display path. Both initialize BW mode asynchronously and stream plane slot 2 into RED RAM and BLACK RAM. Only `reseed_bw_visible` triggers `RefreshMode::Full` and awaits BUSY.

- [ ] **Step 8: Add RED tests for BW differential and recovery paths**

Add `bw_differential_streams_old_and_new_planes`, requiring the normal BW transition to stream the displayed page's slot 2 to RED RAM, the target page's slot 2 to BLACK RAM, and trigger one partial refresh. Add `recovery_seeds_target_with_one_full_refresh`, requiring recovery to stream the target into both planes and trigger one full refresh. Do not add either helper before observing its named test fail.

- [ ] **Step 9: Run the new tests and verify RED**

```bash
cd firmware
cargo test -p binbook-fw --test async_refresh bw_differential_streams_old_and_new_planes -- --exact
cargo test -p binbook-fw --test async_refresh recovery_seeds_target_with_one_full_refresh -- --exact
```

Expected: both tests fail because the asynchronous BW differential and recovery helpers are absent.

- [ ] **Step 10: Implement only the BW differential and recovery helpers**

Reuse the bounded 16-row strip streamer. The differential helper writes the previous page's slot 2 to RED RAM, writes the target page's slot 2 to BLACK RAM, triggers `RefreshMode::Partial`, and awaits BUSY. The recovery helper writes the target page's slot 2 to both RAM planes, triggers `RefreshMode::Full`, and awaits BUSY.

- [ ] **Step 11: Run all display tests**

Run:

```bash
cd firmware
cargo test -p binbook-fw --test async_refresh
cargo test -p binbook-fw --test firmware_logic
cargo test -p ssd1677-driver
```

Expected: all tests pass with no full-frame allocation added.

- [ ] **Step 12: Commit Task 3**

```bash
git add firmware/crates/binbook-fw/src/display.rs firmware/crates/binbook-fw/tests/async_refresh.rs firmware/crates/binbook-fw/tests/firmware_logic.rs
git commit -m "feat(firmware): stream deferred display phases cooperatively"
```

### Task 4: Migrate The Firmware Binary To Embassy Tasks

**Files:**
- Modify: `firmware/crates/binbook-fw/Cargo.toml`
- Modify: `firmware/crates/binbook-fw/src/main.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`
- Modify: `firmware/Cargo.lock`

- [x] **Step 1: Write RED source and behavior tests for runtime invariants**

Add tests requiring:

```rust
#[test]
fn firmware_runtime_uses_approved_async_configuration() {
    let cargo = include_str!("../Cargo.toml");
    let main_rs = include_str!("../src/main.rs");

    assert!(cargo.contains("esp-rtos"));
    assert!(cargo.contains("embassy-sync"));
    assert!(main_rs.contains("DISPLAY_SPI_FREQUENCY_MHZ"));
    assert!(!main_rs.contains("Rate::from_mhz(4)"));
    assert!(main_rs.contains("input_task"));
    assert!(main_rs.contains("display_task"));
    assert!(main_rs.contains("diagnostic_task"));
}
```

Add host tests using an Embassy channel with capacity 16:

```rust
#[test]
fn display_request_channel_is_fifo_and_rejects_the_seventeenth_request() {
    // try_send 16 distinct turns, assert the seventeenth returns Full,
    // then try_receive and assert the original 16 values in exact order.
}
```

- [x] **Step 2: Run runtime tests and verify RED**

Run:

```bash
cd firmware
cargo test -p binbook-fw --test firmware_logic firmware_runtime_uses_approved_async_configuration -- --exact
cargo test -p binbook-fw --test async_refresh display_request_channel_is_fifo_and_rejects_the_seventeenth_request -- --exact
```

Expected: the first test fails because dependencies/tasks/20 MHz are absent; the second fails because the request/channel types are absent.

- [x] **Step 3: Add pinned async dependencies and request types**

Add `esp-rtos = "0.3"` with `esp32c3` and `embassy` features plus versions of `embassy-executor`, `embassy-sync`, `embassy-time`, and `static_cell` resolved on the same compatible line. Keep them behind `firmware-bin` where Cargo feature wiring allows it. Run `cargo tree -p binbook-fw` and require one `esp-hal 1.1.x` line; do not accept a second incompatible HAL version.

Define:

```rust
pub enum DisplayRequest {
    Turn { turn: PageTurn, completion_sequence: Option<u16> },
    Goto { page: u32, completion_sequence: u16 },
    Probe { kind: DisplayProbeKind, completion_sequence: u16 },
}

pub struct DisplayCompletion {
    pub sequence: u16,
    pub status: Status,
    pub page: u32,
}
```

The protocol-specific `Status` representation may be replaced by an app-local completion status if needed to keep default builds independent; conversion must occur in `diag.rs`.

- [x] **Step 4: Make channel tests GREEN**

Run:

```bash
cd firmware
cargo test -p binbook-fw --test async_refresh display_request_channel_is_fifo_and_rejects_the_seventeenth_request -- --exact
```

Expected: FIFO order and reject-newest behavior pass.

- [x] **Step 5: Implement Embassy startup and task ownership**

Use an `esp-rtos` Embassy main entry point. Initialize static request and completion channels before spawning:

- `input_task` owns ADC and calibrated GPIO1/GPIO2 channels.
- `display_task` owns SPI2, CS/DC/RST/BUSY, BinBook scratch, `RefreshCoordinator`, and panel mode.
- `diagnostic_task` owns USB Serial/JTAG only when `diagnostic-console` is enabled.

Set SPI with:

```rust
Rate::from_mhz(DISPLAY_SPI_FREQUENCY_MHZ)
```

Do not share the `Ssd1677Driver` through a mutex.

- [x] **Step 6: Implement responsive input and FIFO page rendering**

`input_task` waits `INPUT_POLL_INTERVAL_MS`, samples both ADC channels, feeds existing `InputState`, and uses `try_send` for each directional press. On full queue, increment a dropped-turn counter and emit `EVT_TURN_DROPPED` without removing an old request.

`display_task` must:

1. Complete startup grayscale and post-gray reseed while requests accumulate.
2. Receive one request at a time.
3. Resolve a `PageTurn` against the last completed page.
4. Complete boundary turns without display IO.
5. Render and complete every intermediate page.
6. Select between a 350 ms Embassy timer and a newly arrived request while in `GrayDelay`.
7. Let input and diagnostics continue while async setup, strip yields, and BUSY waits are pending.

- [x] **Step 7: Run host tests and target builds**

Run:

```bash
cd firmware
cargo test -p binbook-fw --test async_refresh
cargo test -p binbook-fw --test firmware_logic
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,debug-log --target riscv32imc-unknown-none-elf --release
```

Expected: host tests pass and both target images link. This is build evidence only, not device evidence.

- [x] **Step 8: Commit Task 4**

```bash
git add firmware/crates/binbook-fw/Cargo.toml firmware/crates/binbook-fw/src/async_refresh.rs firmware/crates/binbook-fw/src/main.rs firmware/crates/binbook-fw/tests/async_refresh.rs firmware/crates/binbook-fw/tests/firmware_logic.rs firmware/Cargo.lock
git commit -m "refactor(firmware): run display and input with Embassy"
```

### Task 5: Make Diagnostics Concurrent And Evidence-Rich

**Files:**
- Modify: `firmware/crates/binbook-diagnostic-protocol/src/lib.rs`
- Modify: `firmware/crates/binbook-fw/src/diag.rs`
- Modify: `firmware/crates/binbook-fw/src/diag_log.rs`
- Modify: `firmware/crates/binbook-fw/src/main.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Write RED tests for queued KEY semantics**

Add tests proving KEY dispatch preserves a turn rather than resolving against a stale page:

```rust
#[test]
fn batched_key_presses_are_resolved_when_dequeued() {
    let mut harness = AsyncDiagHarness::on_page(1, 4);
    harness.receive_key(10, KeyCode::Right);
    harness.receive_key(11, KeyCode::Right);
    harness.receive_key(12, KeyCode::Left);

    assert_eq!(harness.pending_turns(), [PageTurn::Next, PageTurn::Next, PageTurn::Previous]);
    assert_eq!(harness.rendered_pages_after_completion(), [2, 3, 2]);
    assert_eq!(harness.response_sequences(), [10, 11, 12]);
}
```

Add tests proving immediate STATUS and LOG_GET responses are produced while a render completion remains pending, and queue-full diagnostic KEY receives `Status::Error` without evicting old requests.

- [ ] **Step 2: Run diagnostics tests and verify RED**

Run:

```bash
cd firmware
cargo test -p binbook-fw --features diagnostic-console --test firmware_logic batched_key_presses_are_resolved_when_dequeued -- --exact
cargo test -p binbook-fw --features diagnostic-console --test firmware_logic immediate_commands_continue_while_render_is_pending -- --exact
```

Expected: tests fail because KEY currently returns a pre-resolved render target and only one hardware command can remain pending.

- [ ] **Step 3: Add structured event codes test-first**

First add a failing protocol test requiring stable nonzero codes and CLI names for:

```text
REFRESH_PHASE
TURN_QUEUED
TURN_DEQUEUED
TURN_DROPPED
RESEED_START
RESEED_COMPLETE
DISPLAY_RECOVERY
```

Then add the event constants to `binbook-diagnostic-protocol`, re-export them through `diag_log.rs`, and update CLI formatting. Keep `PROTOCOL_VERSION = 1` and the existing 21-byte STATUS payload unchanged.

- [ ] **Step 4: Route diagnostic display work through channels**

The diagnostic task must continue parsing RX and servicing immediate commands while hardware-backed commands await completion. For each KEY press:

1. Decode the key and map it to `PageTurn`.
2. Construct `DisplayRequest::Turn { turn, completion_sequence: Some(sequence) }` and pass it to `try_send`.
3. Emit `TURN_QUEUED` only after successful enqueue.
4. Return `Status::Error` immediately on full queue.
5. Emit the final response only after the matching `DisplayCompletion` arrives.

Use the existing sequence and opcode validation. Do not acknowledge a queued render as complete.

- [ ] **Step 5: Maintain an independently readable runtime snapshot**

Store current page, page count, panel mode, dropped-log count, protocol-error count, and last error in a short critical-section-protected snapshot updated only at completed state transitions. STATUS reads this snapshot without waiting for display completion.

Protect the fixed diagnostic log with a short critical-section mutex or route events through one bounded logging owner. Do not allocate and do not hold a lock across `.await`.

- [ ] **Step 6: Make diagnostics tests GREEN**

Run:

```bash
cd firmware
cargo test -p binbook-diagnostic-protocol
cargo test -p binbook-fw --features diagnostic-console --test firmware_logic
```

Expected: batched KEY order, delayed responses, immediate STATUS/LOG service, queue-full behavior, and event codes all pass.

- [ ] **Step 7: Build every USB/log feature combination**

Run:

```bash
cd firmware
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console,debug-log --target riscv32imc-unknown-none-elf --release
```

Expected: both builds link; diagnostic-console continues to own USB and `dbgprintln!` remains a no-op in the combined build.

- [ ] **Step 8: Commit Task 5**

```bash
git add firmware/crates/binbook-diagnostic-protocol/src/lib.rs firmware/crates/binbook-fw/src/diag.rs firmware/crates/binbook-fw/src/diag_log.rs firmware/crates/binbook-fw/src/main.rs firmware/crates/binbook-fw/tests/firmware_logic.rs cli/src/lib.rs
git commit -m "feat(firmware): queue diagnostic page turns asynchronously"
```

### Task 6: Add The Autonomous Deferred-Gray Exercise

**Files:**
- Modify: `cli/src/lib.rs`
- Modify: `cli/src/main.rs`
- Modify: `cli/tests/protocol.rs`
- Modify: `cli/tests/hardware_diagnostic.rs`

- [ ] **Step 1: Write RED transport tests for batched requests and matched responses**

Create a fake serial stream that receives three KEY frames with sequences 10, 11, and 12, then returns responses in fragmented reads. Require the transport to retain unrelated frames and return all three matching responses in sequence order.

```rust
#[test]
fn batch_transport_collects_every_sequence_checked_response() {
    let requests = key_batch(&[(10, RIGHT), (11, RIGHT), (12, LEFT)]);
    let mut io = FragmentedBatchIo::new(responses_for(&[10, 11, 12]));

    let responses = send_batch_and_receive_io(
        &mut io,
        &requests,
        Opcode::Key,
        &[10, 11, 12],
        Duration::from_secs(5),
    ).unwrap();

    assert_eq!(decoded_sequences(&responses), [10, 11, 12]);
}
```

Also test wrong opcode, duplicate sequence, missing sequence, non-OK response, and timeout. Each must return a discriminating error.

- [ ] **Step 2: Run batch tests and verify RED**

Run:

```bash
cd cli
cargo test --features serial-device batch_transport -- --nocapture
```

Expected: compilation fails because batch transport does not exist.

- [ ] **Step 3: Implement minimal batch transport**

Add:

```rust
pub fn send_batch_and_receive_io<T: Read + Write>(
    io: &mut T,
    requests: &[u8],
    expected_opcode: Opcode,
    expected_sequences: &[u16],
    timeout: Duration,
) -> Result<Vec<Vec<u8>>, String>;
```

Write all encoded requests in one call, parse bounded COBS frames, validate response kind/opcode/status, reject duplicate expected sequences, and return responses in `expected_sequences` order.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cd cli
cargo test --features serial-device batch_transport
```

Expected: every batch success and failure test passes.

- [ ] **Step 5: Write RED command tests for the autonomous exercise**

Require Clap to parse:

```bash
binbook-cli diag exercise deferred-gray --port /dev/ttyACM0
```

Use a scripted fake session to require this exact operation order:

```text
PAGE goto 0
STATUS expecting current_page=0
LOG clear
KEY RIGHT
LOG poll until REFRESH_PHASE=GrayRefreshing
batched KEY RIGHT, RIGHT, LEFT
responses 2, 3, 2 in sequences
LOG poll until RESEED_COMPLETE for page 2
KEY RIGHT
STATUS expecting current_page=3
LOG retrieval and invariant validation
```

The test must fail if the exercise only receives transport acknowledgements without matching page responses and structured events.

- [ ] **Step 6: Run command tests and verify RED**

Run:

```bash
cd cli
cargo test --features serial-device deferred_gray_exercise -- --nocapture
```

Expected: parse or orchestration failure because the command is absent.

- [ ] **Step 7: Implement `diag exercise deferred-gray`**

Keep one serial session open for the full exercise. Print each phase and elapsed time for webcam correlation. Validate:

- Final response pages are exactly `2, 3, 2, 3` after the initial page-1 turn.
- Every expected response has the correct opcode, sequence, and `Status::Ok`.
- Logs contain ordered queue, refresh, grayscale, and reseed events.
- Dropped-turn count remains zero.
- No render failure, recovery, timeout, or protocol error is introduced.
- Final STATUS is page 3 with `last_error=0`.

- [ ] **Step 8: Add an ignored live hardware test without running it**

Add `hardware_deferred_gray_exercise` to `cli/tests/hardware_diagnostic.rs`, marked `#[ignore]`. It invokes the same orchestration core and requires `/dev/ttyACM0`. Compile-list it only:

```bash
cd cli
cargo test --features serial-device --test hardware_diagnostic -- --list
```

Expected: the new ignored test is listed. Do not run it in this task.

- [ ] **Step 9: Run all CLI tests**

```bash
cd cli
cargo test
cargo test --features serial-device
```

Expected: all non-ignored tests pass.

- [ ] **Step 10: Commit Task 6**

```bash
git add cli/src/lib.rs cli/src/main.rs cli/tests/protocol.rs cli/tests/hardware_diagnostic.rs
git commit -m "feat(cli): add autonomous deferred grayscale exercise"
```

### Task 7: Add Compile-Time Probe Selection And Complete Host Verification

**Files:**
- Modify: `firmware/crates/binbook-fw/Cargo.toml`
- Modify: `firmware/crates/binbook-fw/src/async_refresh.rs`
- Modify: `firmware/crates/binbook-fw/tests/async_refresh.rs`
- Modify: `firmware/scripts/flash-xteink-x4-nav-probe.sh` only if it rejects the new feature argument

- [ ] **Step 1: Write RED tests for compile-time strategy selection**

Require default and probe selections to be explicit:

```rust
#[test]
fn normal_build_uses_visible_reseed_until_hardware_approval() {
    assert_eq!(configured_post_gray_strategy(), PostGrayStrategy::VisibleReseed);
}

#[cfg(feature = "deferred-gray-probe")]
#[test]
fn probe_build_uses_silent_reseed() {
    assert_eq!(configured_post_gray_strategy(), PostGrayStrategy::SilentReseed);
}
```

The conservative pre-experiment default is visible reseeding. Task 9 changes the permanent default only after the webcam verdict.

- [ ] **Step 2: Run strategy tests and verify RED**

Run:

```bash
cd firmware
cargo test -p binbook-fw --test async_refresh normal_build_uses_visible_reseed_until_hardware_approval -- --exact
cargo test -p binbook-fw --features deferred-gray-probe --test async_refresh probe_build_uses_silent_reseed -- --exact
```

Expected: compilation or assertion failure because the feature and selector are absent.

- [ ] **Step 3: Add the temporary probe feature and selector**

Add an empty Cargo feature named `deferred-gray-probe`. Implement `configured_post_gray_strategy()` using `cfg(feature = "deferred-gray-probe")`; do not add a runtime setting or persistent strategy storage.

- [ ] **Step 4: Verify GREEN**

Run both commands from Step 2. Expected: default selects visible and probe selects silent.

- [ ] **Step 5: Run the complete host matrix**

Run:

```bash
cd firmware
cargo clean
cargo test --workspace --features diagnostic-console
cargo test --workspace
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,debug-log --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console,debug-log --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console,deferred-gray-probe --target riscv32imc-unknown-none-elf --release
cd ../cli
cargo test
cargo test --features serial-device
cd ..
uv run pytest -q
git diff --check
```

Expected: every test/build passes, ignored hardware tests remain unexecuted, and `git diff --check` reports no errors.

- [ ] **Step 6: Audit constrained-memory and forbidden protocol changes**

Run:

```bash
rg -n "\[[^]]*48000|\[[^]]*96000|Vec<|Box<" firmware/crates/binbook-fw/src firmware/crates/ssd1677-driver/src
rg -n "serde_json|ciborium|prost|protobuf" firmware/crates
```

Expected: no new frame-sized arrays, heap-backed runtime collections, or forbidden protocol dependencies. Existing unrelated matches must be inspected and documented rather than ignored.

- [ ] **Step 7: Commit Task 7**

```bash
git add firmware/crates/binbook-fw/Cargo.toml firmware/crates/binbook-fw/src/async_refresh.rs firmware/crates/binbook-fw/tests/async_refresh.rs firmware/scripts/flash-xteink-x4-nav-probe.sh firmware/Cargo.lock
git commit -m "test(firmware): add silent reseed hardware probe mode"
```

### Task 8: Update Stable Documentation Before Hardware Work

**Files:**
- Create: `docs/specs/2026-06-27-x4-async-deferred-grayscale-design.md`
- Modify: `docs/reference/squidscript-and-xteink-reference.md`
- Modify: `docs/reference/xteink-x4-agent-device-verification.md`
- Modify: `BINBOOK_FORMAT_SPEC.md` only where it describes firmware refresh readiness

- [ ] **Step 1: Write the current design specification**

Document the constants, task ownership, FIFO semantics, phase transitions, async driver boundary, 20 MHz SPI setting, both reseed strategies, recovery behavior, diagnostic exercise, and the user-verdict gate. Mark silent reseeding as an unverified hardware hypothesis until Task 9.

- [ ] **Step 2: Update the hardware runbook**

Add a final deferred-gray section with the exact flash, 15-second boot capture, autonomous CLI exercise, webcam criteria, strategy-selection branch, final reflash, and independent STATUS/log checks. Preserve the rule that only one process owns `/dev/ttyACM0`.

- [ ] **Step 3: Remove stale refresh descriptions**

Search and update statements that claim display calls block the entire input loop or that every grayscale refresh permanently invalidates the next fast turn. Keep controller facts separate from the still-unverified silent-reseed hypothesis.

Run:

```bash
rg -n "4 MHz|FullBwSeed|bw_differential_ready|blocking|grayscale" BINBOOK_FORMAT_SPEC.md docs firmware/crates/binbook-fw/src
```

Review each match; do not mechanically replace historical documents under `docs/historical/`.

- [ ] **Step 4: Commit Task 8**

```bash
git add docs/specs/2026-06-27-x4-async-deferred-grayscale-design.md docs/reference/squidscript-and-xteink-reference.md docs/reference/xteink-x4-agent-device-verification.md BINBOOK_FORMAT_SPEC.md
git commit -m "docs: specify async deferred grayscale refresh"
```

### Task 9: Final Hardware Phase, BW-Seeding Experiment, And Completion Evidence

**This is the first task allowed to access hardware. Run every flash, USB, and serial command sequentially with host escalation. Never run two target-owning commands in parallel.**

**Files:**
- Modify after verdict: `firmware/crates/binbook-fw/src/async_refresh.rs`
- Modify after verdict: `firmware/crates/binbook-fw/Cargo.toml`
- Modify after verdict: `firmware/crates/binbook-fw/tests/async_refresh.rs`
- Modify after evidence: `docs/specs/2026-06-27-x4-async-deferred-grayscale-design.md`
- Modify after evidence: `HANDOFF.md`

- [ ] **Step 1: Confirm the hardware test binary is compiled and listed**

```bash
cd cli
cargo test --features serial-device --test hardware_diagnostic -- --list
cd ..
```

Expected: `hardware_deferred_gray_exercise` appears as ignored.

- [ ] **Step 2: Flash the silent-reseed experiment**

```bash
FW_FEATURES="firmware-bin,diagnostic-console,deferred-gray-probe" \
  firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Record chip revision, flash size, image size, and final flash result. Wait for USB re-enumeration before opening serial.

- [ ] **Step 3: Capture the required 15-second boot record**

Run the exact pyserial command from `AGENTS.md` against the escalated host-visible port. Record bootloader, partition, segment-load, and application-load lines. Packet firmware may remain silent after boot.

- [ ] **Step 4: Establish diagnostic baseline**

```bash
cd cli
cargo run --features serial-device -- diag hello --port /dev/ttyACM0
cargo run --features serial-device -- diag status --port /dev/ttyACM0
cd ..
```

Expected: protocol 1 identity/capabilities decode, page count is 4, and `last_error=0`.

- [ ] **Step 5: Run the autonomous BW-seeding experiment**

Tell the user the webcam observation is beginning, then run one target-owning process:

```bash
cd cli
cargo run --features serial-device -- \
  diag exercise deferred-gray --port /dev/ttyACM0
cd ..
```

The command must prove through responses, STATUS, and structured logs:

- page 0 baseline;
- page 1 appears through a completed BW turn;
- grayscale starts only after the 350 ms idle deadline;
- RIGHT, RIGHT, LEFT queue during grayscale;
- completed pages are exactly 2, 3, 2 in FIFO order;
- silent reseed completes without a refresh command;
- the next RIGHT completes on page 3;
- no turn drops, recovery events, protocol errors, display errors, or nonzero `last_error` occur.

- [ ] **Step 6: Obtain the webcam verdict**

Ask the user to confirm all five visible criteria:

1. Page 1 appeared in BW before grayscale.
2. Page 1 changed to grayscale after the idle delay.
3. Silent reseeding caused no visible flash, BW reversion, or corruption.
4. Queued intermediate BW pages appeared in the order 2, 3, 2.
5. The final page-3 BW transition was fast, complete, and artifact-free.

Do not infer any criterion from serial output. Record the user's exact pass/fail verdict.

- [x] **Step 7: Select one permanent compile-time strategy**

Before changing production configuration, change the focused default-strategy test to the webcam-selected expectation and add this source test:

```rust
#[test]
fn permanent_build_has_no_deferred_gray_probe_feature() {
    let cargo = include_str!("../Cargo.toml");
    assert!(!cargo.contains("deferred-gray-probe"));
}
```

Run both tests first. Expected: the no-probe test fails; when silent reseeding was selected, the default-strategy test also fails because the conservative pre-experiment default is visible. Only after observing those failures, change the production selector and Cargo features as follows.

If every automated and webcam criterion passes:

- Make `PostGrayStrategy::SilentReseed` the normal build default.
- Remove the `deferred-gray-probe` feature and its conditional selector.
- Change the default-strategy test to require silent reseeding.

If any criterion fails:

- Keep `PostGrayStrategy::VisibleReseed` as the normal build default.
- Remove the `deferred-gray-probe` feature and all silent-probe selection wiring.
- Keep the silent streaming helper only if recovery or a host test still uses it; otherwise remove it.
- Change the default-strategy test to require visible reseeding.

No runtime auto-detection or settings option may remain.

- [x] **Step 8: Re-run host verification after permanent selection**

```bash
cd firmware
cargo clean
cargo test --workspace --features diagnostic-console
cargo test --workspace
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console,debug-log --target riscv32imc-unknown-none-elf --release
cd ../cli
cargo test
cargo test --features serial-device
cd ..
uv run pytest -q
git diff --check
```

Expected: all tests/builds pass with no reference to `deferred-gray-probe`.

- [ ] **Step 9: Flash the permanent diagnostic firmware**

```bash
FW_FEATURES="firmware-bin,diagnostic-console" \
  firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Capture another 15-second boot record. This image, not the temporary probe, is the candidate final device state.

- [ ] **Step 10: Repeat the autonomous exercise against the permanent image**

```bash
cd cli
cargo run --features serial-device -- \
  diag exercise deferred-gray --port /dev/ttyACM0
cd ..
```

Require the same automated checks. Obtain a second webcam confirmation matching the selected strategy: persistent grayscale after silent reseed, or grayscale cleanup followed immediately by visible BW reseed.

- [ ] **Step 11: Run the complete device verification runbook**

Follow `docs/reference/xteink-x4-agent-device-verification.md` sequentially from HELLO through combined-feature USB ownership. Run all three ignored stream tests with one test thread, then independently query STATUS. Reflash `firmware-bin,diagnostic-console` after the combined debug/diagnostic ownership check and reconfirm HELLO.

- [ ] **Step 12: Update stable docs and HANDOFF from observed evidence**

Change the design spec from hypothesis language to the selected permanent behavior. Replace `HANDOFF.md` with a current-state snapshot containing:

- exact commands and relevant full output;
- starting and ending pages;
- measured phase timings;
- queue order and dropped count;
- selected permanent strategy and why;
- the user's webcam verdict;
- all failures/retries;
- host test/build evidence;
- an acceptance matrix mapping each requirement to implementation, automated test, observed result, and hardware evidence.

Any missing webcam criterion or failed runbook check remains explicitly incomplete.

- [ ] **Step 13: Perform an adversarial completion review**

Attempt to disprove each claim:

- Start away from page 0 before testing GOTO.
- Queue mixed directions so coalescing would produce a different visible sequence.
- Query STATUS while grayscale is active.
- Verify a silent reseed has no activation event.
- Verify the permanent feature list contains no probe selector.
- Search source for blocking BUSY loops still reachable from the async display task.
- Search docs for stale 4 MHz, blocking-input, and unselected-strategy claims.

Do not write `complete` if any claim fails this review.

- [ ] **Step 14: Commit the selected strategy and evidence**

Stage exact paths only:

```bash
git add firmware/crates/binbook-fw/Cargo.toml firmware/crates/binbook-fw/src/async_refresh.rs firmware/crates/binbook-fw/tests/async_refresh.rs firmware/Cargo.lock docs/specs/2026-06-27-x4-async-deferred-grayscale-design.md docs/reference/xteink-x4-agent-device-verification.md HANDOFF.md
git commit -m "feat(firmware): enable responsive deferred grayscale refresh"
```

## Acceptance Matrix Requirements

The final `HANDOFF.md` matrix must contain rows for:

| ID | Requirement |
| --- | --- |
| DG-01 | Display SPI runs at 20 MHz and full/partial/grayscale paths remain visually correct. |
| DG-02 | Input polling continues during mode setup, strip streaming, and BUSY waits. |
| DG-03 | Queue capacity is compile-time 16 and newest requests are rejected when full. |
| DG-04 | Every accepted intermediate turn renders in FIFO order. |
| DG-05 | Grayscale begins only after 350 ms idle following completed BW work. |
| DG-06 | Turns arriving during grayscale are retained and rendered afterward. |
| DG-07 | Primary silent reseed or permanent visible fallback matches the webcam verdict. |
| DG-08 | The next turn after post-gray reseeding is correct and fast. |
| DG-09 | One safe recovery occurs after display failure; a second failure enters Fault. |
| DG-10 | STATUS and LOG remain serviceable while display work is pending. |
| DG-11 | Diagnostic KEY responses complete only after their corresponding page outcome. |
| DG-12 | No full framebuffer or hidden frame-sized temporary is introduced. |
| DG-13 | Normal, debug, diagnostic, and combined feature builds pass after strategy selection. |
| DG-14 | Autonomous exercise and full hardware runbook pass on the permanent image. |

Completion requires implementation path, automated test, observed evidence, and hardware evidence for every applicable row. Host compilation alone does not satisfy any visible or timing requirement.
