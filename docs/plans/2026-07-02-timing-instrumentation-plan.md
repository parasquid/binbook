# Timing Instrumentation Implementation Plan

> **For agentic workers:** Execute this plan directly and sequentially. Do not delegate implementation to subagents: `AGENTS.md` requires plans to be executed inline with a current todo tracker. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add observable timing evidence for the Xteink X4 page-turn path from physical/diagnostic input through request enqueue, display execution, SSD1677 busy waits, page commit, and analysis output.

**Architecture:** Keep diagnostic wire records binary and unchanged: every timing datum is a normal `DiagLogRecord` with stable event codes and `arg0..arg2` payloads. Use the existing runtime event pipeline (`RuntimeEventKind` -> `RuntimeAggregator` -> diagnostic log) for firmware-owned timing, and expose SSD1677 wait timing through a reusable, no-std observer seam rather than coupling reusable crates to `binbook-fw` or diagnostic protocol numbers. The analysis script consumes existing `binbook diag logs` output or captured text fixtures, so it does not reimplement COBS framing.

**Tech Stack:** Rust workspace crates (`binbook-diagnostic-protocol`, `binbook-fw`, `ssd1677-driver`, `xteink-x4-display`, `binbook` CLI), Embassy time on firmware, Python 3.13 via `uv` for the host analysis script, live Xteink X4 verification on `/dev/ttyACM0` and webcam `/dev/video1`.

---

## TL;DR

**What you'll get:** A firmware diagnostic timeline that can answer: how long from input to queue, queue to display start, display start to page shown, how much time was spent waiting on SSD1677 BUSY, and which stage dominates page-turn latency.

**Why this approach:** It extends the log/event system that already reaches the serial diagnostic console, while preserving crate boundaries and the no-overhead production path.

**What it will not do:**

- It will not change input debounce/cooldown, display refresh policy, channel capacity, SPI frequency, or page rendering behavior.
- It will not add JSON/CBOR/protobuf or alter `LogRecordPayload` layout.
- It will not add firmware-specific dependencies to reusable crates.
- It will not claim completion without live-device flash, serial logs, and visible/page-state confirmation.

**Primary corrections from the earlier plan:**

- Hardware verification is mandatory for this firmware task; mock data is useful but not sufficient.
- Existing event codes already occupy `0x0100..0x0103`; new codes must not reuse that range.
- `EVT_INPUT_DECISION` already exists. Do not add duplicate constants with the same name.
- Do not add `diagnostic-events` logging to `ssd1677-driver`; add a generic observer seam instead.
- Do not write a Python serial protocol implementation when the Rust CLI already validates request/response framing.

---

## Files and responsibilities

### Modify

- `firmware/crates/binbook-diagnostic-protocol/src/lib.rs`
  - Owns stable diagnostic event numbers.
  - Add only the missing timing event constants.
  - Do not change `PROTOCOL_VERSION`, frame layout, log record layout, opcodes, or payload codecs unless tests prove a protocol contract requires it.

- `firmware/crates/binbook-diagnostic-protocol/tests/codec.rs`
  - Owns protocol-level golden checks for stable event codes and log record encoding.

- `firmware/crates/binbook-fw/src/runtime_engine.rs`
  - Owns firmware-local semantic runtime events.
  - Add typed variants for request enqueue/receive and observed busy waits.

- `firmware/crates/binbook-fw/src/runtime_aggregator.rs`
  - Owns projection from semantic runtime events into `DiagEvent` records.
  - Map new events to stable event codes, levels, subsystems, and `arg0..arg2` meanings.

- `firmware/crates/binbook-fw/src/runtime/input_task.rs`
  - Owns ADC button polling and request enqueue attempts.
  - Emit request enqueue/drop timing around `try_send()` without changing debounce/cooldown behavior.

- `firmware/crates/binbook-fw/src/runtime/display_task.rs`
  - Owns request receive timing and engine request lifecycle at the firmware task boundary.
  - Emit receive/start/end events with sequence/page/status details.

- `firmware/crates/binbook-fw/src/runtime/display_backend.rs`
  - Owns board-specific bridge between reusable display crate operations and firmware runtime events.
  - Provide a firmware observer for SSD1677 busy waits and page commit timing without changing render buffers or SPI behavior.

- `crates/ssd1677-driver/src/wait.rs`
  - Owns generic SSD1677 busy wait loops.
  - Add an optional, dependency-free observer method that reports start/end and poll counts.

- `crates/ssd1677-driver/src/lib.rs`
  - Re-export the observer types if needed by `xteink-x4-display` or firmware tests.

- `crates/xteink-x4-display/src/panel.rs`, `crates/xteink-x4-display/src/render.rs`, and `crates/xteink-x4-display/src/probes.rs`
  - Thread the generic busy-wait observer only where required to observe waits.
  - Keep default APIs usable with a no-op observer.

- `crates/xteink-x4-display/tests/refresh.rs`
  - Add a behavior test when observer plumbing changes public panel/render APIs.

- `firmware/crates/binbook-fw/tests/diagnostic_structured_log.rs`
  - Verify firmware diagnostic event mappings and `arg0..arg2` contracts.

- `crates/binbook/src/diag_response.rs`
  - Add names for new event codes so `binbook diag logs` prints readable output.

- `scripts/analyze_timing.py`
  - Parse CLI log text and produce timeline/stage summary.
  - Do not talk directly to the serial device unless explicitly added later; use `cargo run -p binbook --features serial-device -- diag logs` as the capture surface.

- `tests/test_timing_analysis.py`
  - Host tests for parsing and metric calculation with deterministic sample log text.

- `HANDOFF.md`
  - Current-state snapshot of implementation, verification evidence, hardware evidence, and any unverified items.

### Do not modify

- Do not change `LogRecordPayload` byte layout or diagnostic frame opcodes.
- Do not change input thresholds, button mappings, debounce/cooldown, request channel capacity, display refresh policy, waveform LUTs, SPI pins, SPI frequency, or buffer sizes unless a test proves instrumentation cannot be implemented otherwise.
- Do not add heap allocation or full-page buffers to firmware or reusable display paths.

---

## Event contract

Keep existing event codes and meanings intact. Add these new constants in `binbook-diagnostic-protocol/src/lib.rs` using currently free ranges:

```rust
pub const EVT_REQUEST_ENQUEUE: u16 = 0x0207;
pub const EVT_REQUEST_RECEIVE: u16 = 0x0208;
pub const EVT_DISPLAY_REQUEST_START: u16 = 0x030D;
pub const EVT_DISPLAY_REQUEST_END: u16 = 0x030E;
pub const EVT_BUSY_WAIT_START: u16 = 0x0404;
pub const EVT_BUSY_WAIT_END: u16 = 0x0405;
```

Do not add a new `EVT_INPUT_DECISION`; it already exists at `0x0103`.

### Payload meanings

| Event | Subsystem | Level | `arg0` | `arg1` | `arg2` |
| --- | --- | --- | --- | --- | --- |
| `EVT_INPUT_DECISION` | Input | Info | observed button or `-1` | decision code: press `0`, released `1`, cooldown `2`, unchanged `-1` | elapsed since last press ms |
| `EVT_REQUEST_ENQUEUE` | Input | Info/Warn | request kind | sequence or `-1` | status: ok `0`, full/dropped `1`, unmapped `2` |
| `EVT_REQUEST_RECEIVE` | Navigation | Info | request kind | sequence or `-1` | queue age ms if known, else `-1` |
| `EVT_DISPLAY_REQUEST_START` | Display | Info | request kind | current page before request | target page if known, else `-1` |
| `EVT_DISPLAY_REQUEST_END` | Display | Info/Error | request kind | duration ms | status: ok `0`, error `1` |
| `EVT_BUSY_WAIT_START` | Display | Debug | wait site code | timeout ms | active-high busy state code if available, else `-1` |
| `EVT_BUSY_WAIT_END` | Display | Debug/Error | wait site code | elapsed/poll count ms | status: ready `0`, timeout `1`, pin error `2` |
| Existing `EVT_PAGE_TURN` | Navigation | Info | previous page | displayed page | `0` |
| Existing `EVT_TURN_DEQUEUED` | Input/Navigation legacy | Info | sequence or `-1` | page | `0` |

Request kind codes must be a local `repr(i32)`-style mapping in firmware tests, not copied ad hoc through call sites:

```rust
Turn = 0
Goto = 1
Probe = 2
MenuNext = 3
MenuPrev = 4
MenuSelect = 5
MenuBack = 6
```

Busy wait site codes must distinguish at least:

```rust
GenericWaitReady = 0
PanelInit = 1
BwRefresh = 2
GrayRefresh = 3
Probe = 4
```

If the actual call graph cannot identify the precise site without broad API churn, use `GenericWaitReady = 0` for the first implementation and record that limitation in `HANDOFF.md`; do not fake precision.

---

## Task 1: Protocol event constants and readable CLI names

**Files:**

- Modify: `firmware/crates/binbook-diagnostic-protocol/src/lib.rs:1158`
- Modify: `firmware/crates/binbook-diagnostic-protocol/tests/codec.rs:60`
- Modify: `crates/binbook/src/diag_response.rs:168`

- [ ] **Step 1: Write the failing protocol constant test**

  Add a test near `deferred_gray_event_codes_are_stable_and_nonzero`:

  ```rust
  #[test]
  fn timing_event_codes_are_stable_and_nonzero() {
      use binbook_diagnostic_protocol::{
          EVT_BUSY_WAIT_END, EVT_BUSY_WAIT_START, EVT_DISPLAY_REQUEST_END,
          EVT_DISPLAY_REQUEST_START, EVT_REQUEST_ENQUEUE, EVT_REQUEST_RECEIVE,
      };

      let expected = [
          (EVT_REQUEST_ENQUEUE, 0x0207),
          (EVT_REQUEST_RECEIVE, 0x0208),
          (EVT_DISPLAY_REQUEST_START, 0x030D),
          (EVT_DISPLAY_REQUEST_END, 0x030E),
          (EVT_BUSY_WAIT_START, 0x0404),
          (EVT_BUSY_WAIT_END, 0x0405),
      ];
      for (actual, expected) in expected {
          assert_eq!(actual, expected);
          assert_ne!(actual, 0);
      }
  }
  ```

- [ ] **Step 2: Run the failing test**

  Run: `cargo test -p binbook-diagnostic-protocol timing_event_codes_are_stable_and_nonzero -- --exact`

  Expected before implementation: compile failure for missing `EVT_*` constants.

- [ ] **Step 3: Add the constants**

  Add the six constants exactly as defined in the Event contract section. Do not renumber existing constants.

- [ ] **Step 4: Add CLI event names**

  Extend `event_name()` in `crates/binbook/src/diag_response.rs`:

  ```rust
  EVT_REQUEST_ENQUEUE => "REQUEST_ENQUEUE",
  EVT_REQUEST_RECEIVE => "REQUEST_RECEIVE",
  EVT_DISPLAY_REQUEST_START => "DISPLAY_REQUEST_START",
  EVT_DISPLAY_REQUEST_END => "DISPLAY_REQUEST_END",
  EVT_BUSY_WAIT_START => "BUSY_WAIT_START",
  EVT_BUSY_WAIT_END => "BUSY_WAIT_END",
  ```

- [ ] **Step 5: Verify**

  Run:

  ```bash
  cargo test -p binbook-diagnostic-protocol timing_event_codes_are_stable_and_nonzero -- --exact
  cargo test -p binbook --features serial-device
  ```

  Expected: both commands exit 0.

---

## Task 2: Firmware runtime event mapping tests

**Files:**

- Modify: `firmware/crates/binbook-fw/src/runtime_engine.rs:26`
- Modify: `firmware/crates/binbook-fw/src/runtime_aggregator.rs:1`
- Modify: `firmware/crates/binbook-fw/tests/diagnostic_structured_log.rs:1`

- [ ] **Step 1: Write failing aggregation tests**

  Add tests under `#![cfg(feature = "diagnostic-console")]` that instantiate `RuntimeAggregator::<4, 16>` with a `DiagnosticSnapshot`, commit one runtime event per new timing variant, and assert the log record event code, subsystem, level, and args.

  Required test names:

  ```rust
  timing_request_enqueue_maps_to_input_log_record
  timing_request_receive_maps_to_navigation_log_record
  timing_display_request_end_maps_status_and_duration
  timing_busy_wait_end_maps_timeout_as_error
  ```

  Each test must read records with `read_from_sequence(0, &mut out)` and assert exact `event`, `arg0`, `arg1`, and `arg2`. Do not use source-text assertions.

- [ ] **Step 2: Run the failing tests**

  Run: `cargo test -p binbook-fw --features diagnostic-console timing_ -- --nocapture`

  Expected before implementation: compile failure for missing `RuntimeEventKind` variants.

- [ ] **Step 3: Add typed runtime variants**

  Add variants to `RuntimeEventKind` with typed fields, not raw diagnostic args:

  ```rust
  RequestEnqueue { kind: RuntimeRequestKind, sequence: Option<u16>, status: RequestEnqueueStatus },
  RequestReceive { kind: RuntimeRequestKind, sequence: Option<u16>, queue_age_ms: Option<u32> },
  DisplayRequestStart { kind: RuntimeRequestKind, current_page: u32, target_page: Option<u32> },
  DisplayRequestEnd { kind: RuntimeRequestKind, duration_ms: u32, status: RuntimeCompletionStatus },
  BusyWaitStart { site: BusyWaitSite, timeout_ms: u32, busy_state: Option<bool> },
  BusyWaitEnd { site: BusyWaitSite, elapsed_ms: u32, status: BusyWaitStatus },
  ```

  Define the supporting enums in `runtime_engine.rs` so the aggregator owns numeric conversion. Keep names firmware-local.

- [ ] **Step 4: Map variants in `RuntimeAggregator::commit()`**

  Convert the typed values to the Event contract payloads. Clamp durations with `min(i32::MAX as u32) as i32`. Use `LEVEL_ERROR` for `BusyWaitStatus::Timeout`, `BusyWaitStatus::PinError`, and `DisplayRequestEnd` error status.

- [ ] **Step 5: Verify**

  Run:

  ```bash
  cargo test -p binbook-fw --features diagnostic-console timing_ -- --nocapture
  cargo test -p binbook-fw --features diagnostic-console
  ```

  Expected: both commands exit 0.

---

## Task 3: Request enqueue and receive instrumentation

**Files:**

- Modify: `firmware/crates/binbook-fw/src/runtime/input_task.rs:48`
- Modify: `firmware/crates/binbook-fw/src/runtime/display_task.rs:181`
- Modify: `firmware/crates/binbook-fw/src/async_refresh.rs` when queue-age measurement is implemented by carrying an enqueue timestamp in `DisplayRequest`.
- Test: `firmware/crates/binbook-fw/tests/diagnostic_structured_log.rs`

- [ ] **Step 1: Write the discriminating test**

  Add a unit-style test for helper functions, not the infinite Embassy tasks. Extract pure helpers if necessary:

  ```rust
  request_kind_for_turn_is_stable
  request_enqueue_status_for_full_channel_is_dropped
  request_receive_uses_nonnegative_queue_age_when_sent_timestamp_exists
  ```

  The test must fail if enqueue success and enqueue failure produce the same `arg2`.

- [ ] **Step 2: Run the failing tests**

  Run: `cargo test -p binbook-fw --features diagnostic-console request_ -- --nocapture`

  Expected before implementation: missing helper or wrong mapping failure.

- [ ] **Step 3: Instrument `input_task.rs` around `try_send()`**

  For every `Press(button)` decision:

  1. Record `timestamp_ms = Instant::now().as_millis()` once for the decision.
  2. Map `button_to_request(button, mode)`.
  3. If there is no request for the current mode, emit `RequestEnqueue { status: Unmapped }`.
  4. If `try_send(request)` succeeds, emit `RequestEnqueue { status: Ok }` and then increment `REQUEST_EPOCH` exactly as today.
  5. If `try_send(request)` fails, emit `RequestEnqueue { status: Full }` and preserve the existing `TurnDropped` event for turns.

  Do not await on the display request channel and do not change the request channel capacity.

- [ ] **Step 4: Instrument `display_task.rs` after `request_rx.receive()`**

  Emit `RequestReceive` immediately in `Either::First(request)` before mode-specific handling. If the request carries a completion sequence, include it; otherwise use `None`. If enqueue timestamp is not stored in the request, set `queue_age_ms: None` rather than fabricating a queue age.

- [ ] **Step 5: Verify**

  Run:

  ```bash
  cargo test -p binbook-fw --features diagnostic-console request_ -- --nocapture
  cargo test -p binbook-fw --features diagnostic-console
  cargo test -p binbook-fw
  ```

  Expected: all commands exit 0. The no-feature test proves the instrumentation compiles away from normal firmware builds where code is feature-gated.

---

## Task 4: Display request lifecycle instrumentation

**Files:**

- Modify: `firmware/crates/binbook-fw/src/runtime/display_task.rs:241`
- Modify: `firmware/crates/binbook-fw/src/runtime_engine.rs` for any lifecycle helper enums not completed in Task 2.
- Test: `firmware/crates/binbook-fw/tests/diagnostic_structured_log.rs`

- [ ] **Step 1: Write lifecycle mapping tests**

  Add tests that commit `DisplayRequestStart` and `DisplayRequestEnd` events and assert exact log records for a successful turn and an error case.

- [ ] **Step 2: Run the failing tests**

  Run: `cargo test -p binbook-fw --features diagnostic-console display_request_ -- --nocapture`

  Expected before implementation: missing or incorrect lifecycle records.

- [ ] **Step 3: Instrument the engine request boundary**

  Around `engine.request(display_request(request), &mut backend, &mut events, now_ms).await`:

  1. Compute `start_ms = Instant::now().as_millis()`.
  2. Emit `DisplayRequestStart` with current page and target page if known.
  3. Await the existing request exactly once.
  4. Compute `duration_ms = end_ms.saturating_sub(start_ms).min(u32::MAX as u64) as u32`.
  5. Emit `DisplayRequestEnd` with `Ok` or `Error` based on the returned result.
  6. Preserve `events.flush().await` after the request.

  Do not reorder menu handling or `engine.advance()` polling.

- [ ] **Step 4: Verify**

  Run:

  ```bash
  cargo test -p binbook-fw --features diagnostic-console display_request_ -- --nocapture
  cargo test -p binbook-fw --features diagnostic-console
  ```

  Expected: all commands exit 0.

---

## Task 5: SSD1677 busy-wait observer seam

**Files:**

- Modify: `crates/ssd1677-driver/src/wait.rs:8`
- Modify: `crates/ssd1677-driver/src/lib.rs`
- Modify: `crates/xteink-x4-display/src/panel.rs`, `crates/xteink-x4-display/src/render.rs`, and `crates/xteink-x4-display/src/probes.rs`.
- Modify: `firmware/crates/binbook-fw/src/runtime/display_backend.rs:15`
- Test: `crates/ssd1677-driver/tests/async_wait.rs`

- [ ] **Step 1: Write the failing driver test**

  Add a test with fake BUSY pin and fake async delay proving an observer receives exactly one start and one end event for a ready path and a timeout path.

  Required assertions:

  - ready path status is `Ready`;
  - timeout path status is `Timeout`;
  - timeout path records `busy_timeout_ms` poll attempts;
  - existing `wait_ready_async()` behavior is unchanged.

- [ ] **Step 2: Run the failing driver test**

  Run: `cargo test -p ssd1677-driver busy_wait_observer -- --nocapture`

  Expected before implementation: missing observer API compile failure.

- [ ] **Step 3: Add the no-std observer API**

  Add dependency-free types in `wait.rs`:

  ```rust
  pub enum BusyWaitOutcome {
      Ready,
      Timeout,
      PinError,
  }

  pub trait BusyWaitObserver {
      fn busy_wait_start(&mut self, timeout_ms: u32, busy_state: Option<bool>);
      fn busy_wait_end(&mut self, elapsed_ms: u32, outcome: BusyWaitOutcome);
  }

  pub struct NoopBusyWaitObserver;
  ```

  Keep `wait_ready_async()` as the default public method and implement it by calling the observed variant with `NoopBusyWaitObserver`. Do not add `binbook-fw`, Embassy executor, diagnostic protocol, heap allocation, or a feature flag to `ssd1677-driver`.

- [ ] **Step 4: Thread observation through display and firmware**

  Add only the smallest API needed in `xteink-x4-display` for `HardwareDisplayBackend` to pass an observer into panel operations. In `display_backend.rs`, create a tiny observer that emits `RuntimeEventKind::BusyWaitStart`/`BusyWaitEnd` using `RUNTIME_EVENT_CHANNEL.sender().try_send(RuntimeEvent { timestamp_ms, kind })` with `Instant::now().as_millis()`.

  If a wait event is dropped because the runtime channel is full, do not block and do not retry; timing instrumentation must not change display timing.

- [ ] **Step 5: Verify**

  Run:

  ```bash
  cargo test -p ssd1677-driver busy_wait_observer -- --nocapture
  cargo test -p ssd1677-driver
  cargo test -p xteink-x4-display
  cargo test -p binbook-fw --features diagnostic-console
  cargo check -p ssd1677-driver --no-default-features --target riscv32imc-unknown-none-elf
  ```

  Expected: all commands exit 0.

---

## Task 6: Timing analysis script and tests

**Files:**

- Create: `scripts/analyze_timing.py`
- Create: `tests/test_timing_analysis.py`

- [ ] **Step 1: Write host tests first**

  Add tests with deterministic CLI log text containing these records in order:

  ```text
  seq=10 tick_ms=1000 level=2 subsystem=2 event=INPUT_DECISION arg0=1 arg1=0 arg2=300
  seq=11 tick_ms=1002 level=2 subsystem=2 event=REQUEST_ENQUEUE arg0=0 arg1=7 arg2=0
  seq=12 tick_ms=1004 level=2 subsystem=3 event=REQUEST_RECEIVE arg0=0 arg1=7 arg2=2
  seq=13 tick_ms=1005 level=2 subsystem=1 event=DISPLAY_REQUEST_START arg0=0 arg1=0 arg2=1
  seq=14 tick_ms=1010 level=1 subsystem=1 event=BUSY_WAIT_START arg0=2 arg1=15000 arg2=1
  seq=15 tick_ms=1320 level=1 subsystem=1 event=BUSY_WAIT_END arg0=2 arg1=310 arg2=0
  seq=16 tick_ms=1900 level=2 subsystem=3 event=PAGE_TURN arg0=0 arg1=1 arg2=0
  seq=17 tick_ms=1901 level=2 subsystem=1 event=DISPLAY_REQUEST_END arg0=0 arg1=896 arg2=0
  ```

  Define `SAMPLE_LOG` from the log text above, then add these tests:

  ```python
  def test_parse_cli_log_records_extracts_timing_events():
      records = timing.parse_log_text(SAMPLE_LOG)
      assert [record.event for record in records] == [
          "INPUT_DECISION",
          "REQUEST_ENQUEUE",
          "REQUEST_RECEIVE",
          "DISPLAY_REQUEST_START",
          "BUSY_WAIT_START",
          "BUSY_WAIT_END",
          "PAGE_TURN",
          "DISPLAY_REQUEST_END",
      ]


  def test_page_turn_summary_computes_stage_durations():
      [summary] = timing.build_timelines(timing.parse_log_text(SAMPLE_LOG))
      assert summary.input_to_enqueue_ms == 2
      assert summary.enqueue_to_receive_ms == 2
      assert summary.receive_to_display_start_ms == 1
      assert summary.display_request_ms == 896
      assert summary.busy_wait_ms == 310
      assert summary.input_to_page_ms == 900
      assert summary.bottleneck_stage == "display_request"


  def test_missing_required_event_reports_incomplete_timeline():
      incomplete = SAMPLE_LOG.replace("event=PAGE_TURN", "event=REFRESH_PHASE")
      assert timing.build_timelines(timing.parse_log_text(incomplete)) == []
  ```

- [ ] **Step 2: Run failing tests**

  Run: `uv run pytest -q tests/test_timing_analysis.py`

  Expected before implementation: import failure for missing script/module.

- [ ] **Step 3: Implement `scripts/analyze_timing.py`**

  Requirements:

  - Use only Python standard library.
  - Accept `--log-text PATH` to parse saved CLI output.
  - Accept `--capture --port /dev/ttyACM0 --since 0` to run `cargo run -p binbook --features serial-device -- diag logs --port <port> --since <cursor>` and parse stdout.
  - Print per-turn rows with input-to-enqueue, enqueue-to-receive, receive-to-display-start, display duration, busy-wait total, total input-to-page, and bottleneck stage.
  - Print summary min/max/avg/p95 for complete timelines.
  - Return nonzero when no complete page-turn timeline is found unless `--allow-incomplete` is set.

- [ ] **Step 4: Verify**

  Run:

  ```bash
  uv run pytest -q tests/test_timing_analysis.py
  uv run python scripts/analyze_timing.py --help
  ```

  Expected: tests exit 0 and help text lists `--log-text`, `--capture`, `--port`, `--since`, and `--allow-incomplete`.

---

## Task 7: Host build and lint gates

**Files:** No new code beyond previous tasks.

- [ ] **Step 1: Format check**

  Run: `cargo fmt --all -- --check`

  Expected: exits 0. If it fails, run `cargo fmt --all`, inspect the diff, and rerun the check.

- [ ] **Step 2: Rust tests**

  Run:

  ```bash
  cargo test -p binbook-diagnostic-protocol
  cargo test -p ssd1677-driver
  cargo test -p xteink-x4-display
  cargo test -p binbook-fw
  cargo test -p binbook-fw --features diagnostic-console
  cargo test -p binbook --features serial-device
  cargo test --workspace
  ```

  Expected: every command exits 0. Investigate any failure before moving to hardware.

- [ ] **Step 3: Clippy and firmware target builds**

  Run:

  ```bash
  cargo clippy -p binbook-fw --all-targets --features diagnostic-console -- -D warnings
  cargo clippy -p ssd1677-driver --all-targets --all-features -- -D warnings
  cargo check -p ssd1677-driver --no-default-features --target riscv32imc-unknown-none-elf
  cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
  cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
  ```

  Expected: every command exits 0.

- [ ] **Step 4: Python tests**

  Run: `uv run pytest -q`

  Expected: exits 0.

---

## Task 8: Live Xteink X4 verification gate

**Files:**

- Modify: `HANDOFF.md`
- Read: `docs/reference/xteink-x4-agent-device-verification.md`
- Read: `AGENTS.local.md` for webcam `/dev/video1` and crop notes.

- [ ] **Step 1: Prepare serial CLI environment**

  Run:

  ```bash
  export PORT="${PORT:-/dev/ttyACM0}"
  export SYSTEMD_PREFIX="$(brew --prefix systemd)"
  export PKG_CONFIG_PATH="$SYSTEMD_PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
  export LIBRARY_PATH="$SYSTEMD_PREFIX/lib:${LIBRARY_PATH:-}"
  export LD_LIBRARY_PATH="$SYSTEMD_PREFIX/lib:${LD_LIBRARY_PATH:-}"
  cargo test -p binbook --features serial-device --test hardware_diagnostic -- --list
  ```

  Expected: the listed hardware tests include `hardware_byte_by_byte_status_request`, `hardware_two_frame_batched_request`, and `hardware_malformed_frame_does_not_wedge_stream`.

- [ ] **Step 2: Flash the diagnostic firmware**

  Run sequentially with no other serial process active:

  ```bash
  FW_FEATURES="firmware-bin,diagnostic-console" \
    firmware/scripts/flash-xteink-x4-nav-probe.sh
  ```

  Record chip, flash size, application size, and final flash result in `HANDOFF.md`.

- [ ] **Step 3: Capture boot serial for 15 seconds**

  Run the exact pyserial command from `AGENTS.md` using `/dev/ttyACM0`. Packet firmware may emit no text after boot; record any bootloader and application-load lines that appear in the 15-second capture.

- [ ] **Step 4: Establish protocol baseline**

  Run:

  ```bash
  cargo run -p binbook --features serial-device -- diag hello --port "$PORT"
  cargo run -p binbook --features serial-device -- diag status --port "$PORT"
  cargo run -p binbook --features serial-device -- diag logs --port "$PORT" --clear
  ```

  Expected: HELLO decodes protocol/capabilities, STATUS has `last_error=0`, and log clear returns a valid cursor.

- [ ] **Step 5: Generate a discriminating page turn**

  Run:

  ```bash
  cargo run -p binbook --features serial-device -- diag page --port "$PORT" goto 0
  cargo run -p binbook --features serial-device -- diag status --port "$PORT"
  cargo run -p binbook --features serial-device -- diag key --port "$PORT" RIGHT
  cargo run -p binbook --features serial-device -- diag status --port "$PORT"
  cargo run -p binbook --features serial-device -- diag logs --port "$PORT" --since 0 > /tmp/binbook-timing-log.txt
  ```

  Expected: STATUS proves page transition `0 -> 1`; log output includes `INPUT_DECISION`, `REQUEST_ENQUEUE`, `REQUEST_RECEIVE`, `DISPLAY_REQUEST_START`, at least one `BUSY_WAIT_START`/`BUSY_WAIT_END` pair during display work, `PAGE_TURN`, and `DISPLAY_REQUEST_END`.

- [ ] **Step 6: Analyze captured timing**

  Run:

  ```bash
  uv run python scripts/analyze_timing.py --log-text /tmp/binbook-timing-log.txt
  ```

  Expected: output contains one complete timeline, positive total input-to-page time, nonnegative queue/display/busy-wait durations, and a named bottleneck stage.

- [ ] **Step 7: Capture visible evidence**

  Run:

  ```bash
  ffmpeg -hide_banner -loglevel error -f video4linux2 -i /dev/video1 -frames:v 1 /tmp/x4-timing-verification.jpg
  ```

  Inspect `/tmp/x4-timing-verification.jpg`. Record the file path and what is visibly shown on the panel in `HANDOFF.md`. Do not substitute decoded fixtures or simulator output for this image.

- [ ] **Step 8: Record acceptance matrix in `HANDOFF.md`**

  Include every row from this matrix and fill the evidence column with exact commands and key output:

  | Requirement | Implementation path | Automated test | Live evidence |
  | --- | --- | --- | --- |
  | Input decision timestamp appears | `input_task.rs` -> `RuntimeAggregator` -> `EVT_INPUT_DECISION` | `cargo test -p binbook-fw --features diagnostic-console timing_` | `/tmp/binbook-timing-log.txt` contains `INPUT_DECISION` |
  | Request enqueue status appears | `input_task.rs` -> `EVT_REQUEST_ENQUEUE` | request enqueue mapping test | log contains `REQUEST_ENQUEUE arg2=0` for successful turn |
  | Request receive appears | `display_task.rs` -> `EVT_REQUEST_RECEIVE` | request receive mapping test | log contains `REQUEST_RECEIVE` after enqueue |
  | Display request duration appears | `display_task.rs` -> `EVT_DISPLAY_REQUEST_START/END` | display lifecycle mapping test | analysis output reports display duration |
  | Busy wait timing appears | `ssd1677-driver` observer -> firmware observer -> `EVT_BUSY_WAIT_*` | `cargo test -p ssd1677-driver busy_wait_observer` | log contains start/end pair |
  | Page commit observed | existing `PageDisplayed` -> `EVT_PAGE_TURN` | existing structured log tests | STATUS and log prove page 1 displayed |
  | Analysis computes bottleneck | `scripts/analyze_timing.py` | `uv run pytest -q tests/test_timing_analysis.py` | script output names bottleneck |
  | No normal-build diagnostic dependency | feature-gated firmware integration and reusable observer default | `cargo test -p binbook-fw`; firmware-bin build without diagnostic-console | normal firmware build exits 0 |

---

## Final adversarial review

- [ ] Re-read the original task and confirm no timing stage is represented only by a compile check.
- [ ] Inspect `git diff` and confirm no refresh timing, debounce, channel capacity, SPI frequency, or protocol frame layout changed.
- [ ] Run `git diff --check`.
- [ ] Confirm `HANDOFF.md` separates verified behavior, transport-only acknowledgements, unverified visual results, known failures, and incomplete requirements.
- [ ] Confirm no completion claim says hardware work is complete without flash, serial, and webcam evidence.

---

## Commit strategy

Do not commit unless the user explicitly asks. If asked to commit after verification, use specific paths only and Conventional Commits:

1. `feat(diag): add timing diagnostic event codes`
2. `feat(firmware): log page-turn timing events`
3. `feat(ssd1677): expose busy-wait observer`
4. `feat(scripts): analyze diagnostic timing logs`
5. `docs: record timing instrumentation evidence`

Never use `git add -A` or `git add .`.

---

## Success criteria

The work is complete only when all are true:

1. New timing events appear as structured diagnostic log records during a live page turn.
2. Existing protocol frame and log record layouts are unchanged.
3. Normal non-diagnostic firmware builds and tests still pass.
4. Feature-gated diagnostic tests pass with `--features diagnostic-console`.
5. Reusable crates remain independently buildable/testable and do not depend on firmware or diagnostic protocol crates.
6. `scripts/analyze_timing.py` produces a complete timeline and bottleneck summary from captured log text.
7. Live Xteink X4 flash, serial, page-state query, and webcam evidence are recorded in `HANDOFF.md`.
