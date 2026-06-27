# X4 BW Seed Refresh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. This repository asks agents to execute sequentially in the main thread and keep a todo tracker current. Do not delegate implementation to subagents. Use TDD for every code task: write the failing test first, run it and confirm the expected failure, implement the minimal change, then run the relevant tests and confirm they pass before starting the next task.

**Goal:** Fix dirty Xteink X4 page turns by requiring a full BW seed refresh before any partial BW differential refresh after grayscale rendering.

**Architecture:** Make BW differential readiness an explicit firmware state. A grayscale render remains the first-page and cleanup path, but it invalidates BW differential readiness; the next different page uses a full BW seed refresh that writes the target BW plane to both SSD1677 RAM planes before later full-screen partial differential turns are allowed. Track panel mode in `binbook-fw` so grayscale and BW refresh paths each initialize the SSD1677 controller for the mode they use.

**Tech Stack:** Rust `no_std` firmware crates, `ssd1677-driver`, `xteink-hal`, BinBook Rust parser, host tests with `cargo test`, pinned nightly ESP32-C3 firmware build, Xteink X4 hardware verification.

---

## Context

Read these files before editing:

- `docs/specs/2026-06-26-x4-bw-seed-refresh-design.md`
- `docs/specs/2026-06-26-x4-clean-differential-refresh-design.md`
- `firmware/crates/binbook-fw/src/refresh.rs`
- `firmware/crates/binbook-fw/src/display.rs`
- `firmware/crates/binbook-fw/src/main.rs`
- `firmware/crates/ssd1677-driver/src/lib.rs`

Do not change the `.binbook` format. Existing X4 plane 2 BW data is enough.
Chunk-dirty refresh remains a debug/probe path and must not become the normal
default.

## Files

- Modify: `firmware/crates/binbook-fw/src/refresh.rs`
  - Add `FullBwSeed`.
  - Track `bw_differential_ready`.
  - Gate partial differential decisions on BW readiness.
- Modify: `firmware/crates/binbook-fw/src/display.rs`
  - Add `PanelMode`.
  - Add explicit grayscale/BW mode initialization helpers.
  - Add full BW seed streaming.
  - Pass panel mode through the render path.
- Modify: `firmware/crates/binbook-fw/src/main.rs`
  - Own a `PanelMode` beside `RefreshState`.
  - Remove reliance on one boot-time grayscale init for all later paths.
  - Improve debug logs for policy, decision, and panel mode.
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`
  - Add TDD tests before implementation for refresh state and source-level render wiring.
- Modify: `BINBOOK_FORMAT_SPEC.md`
  - Clarify BW seed requirement after grayscale.
- Modify: `docs/specs/2026-06-26-x4-clean-differential-refresh-design.md`
  - Align the older clean-differential design with BW seed readiness.
- Modify: `HANDOFF.md`
  - Record final test, build, serial, and visual hardware evidence.

## Task 1: Make BW Differential Readiness Explicit

**Files:**
- Modify: `firmware/crates/binbook-fw/src/refresh.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Write failing refresh-state tests**

Add these tests to `firmware/crates/binbook-fw/tests/firmware_logic.rs` near the existing refresh policy tests:

```rust
#[test]
fn bw_seed_required_after_full_grayscale() {
    let mut state = RefreshState::new();
    let first = state.decide(0, None);
    assert_eq!(first, RefreshDecision::FullGrayscale);
    state.record_success(0, first);

    assert_eq!(state.decide(1, Some(0b101)), RefreshDecision::FullBwSeed);
}

#[test]
fn bw_seed_allows_full_screen_differential_after_record_success() {
    let mut state = RefreshState::new();
    let first = state.decide(0, None);
    state.record_success(0, first);
    let seed = state.decide(1, Some(0b101));
    assert_eq!(seed, RefreshDecision::FullBwSeed);
    state.record_success(1, seed);

    assert_eq!(
        state.decide(2, Some(0b111)),
        RefreshDecision::FullScreenDifferential
    );
}

#[test]
fn bw_seed_invalidated_by_cleanup_full_grayscale() {
    let mut state = RefreshState::new();
    let first = state.decide(0, None);
    state.record_success(0, first);
    let seed = state.decide(1, Some(1));
    state.record_success(1, seed);

    for page in 2..=6 {
        let decision = state.decide(page, Some(1));
        state.record_success(page, decision);
    }

    let cleanup = state.decide(7, Some(1));
    assert_eq!(cleanup, RefreshDecision::FullGrayscale);
    state.record_success(7, cleanup);

    assert_eq!(state.decide(8, Some(1)), RefreshDecision::FullBwSeed);
}

#[test]
fn bw_seed_required_before_chunk_dirty_policy() {
    let mut state = RefreshState::new();
    let first = state.decide_with_policy(0, None, RefreshPolicy::ChunkDirtyDifferentialDefault);
    state.record_success(0, first);

    assert_eq!(
        state.decide_with_policy(1, Some(0b101), RefreshPolicy::ChunkDirtyDifferentialDefault),
        RefreshDecision::FullBwSeed
    );

    state.record_success(1, RefreshDecision::FullBwSeed);
    assert_eq!(
        state.decide_with_policy(2, Some(0b101), RefreshPolicy::ChunkDirtyDifferentialDefault),
        RefreshDecision::AdjacentDirtyPartial {
            changed_chunk_mask: 0b101
        }
    );
}
```

- [ ] **Step 2: Run tests and confirm the expected failure**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic bw_seed_ -- --nocapture
```

Expected result: compile failure because `RefreshDecision::FullBwSeed` does not exist.

- [ ] **Step 3: Implement the minimal refresh-state model**

In `firmware/crates/binbook-fw/src/refresh.rs`, add the enum variant:

```rust
pub enum RefreshDecision {
    FullGrayscale,
    FullBwSeed,
    AdjacentDirtyPartial { changed_chunk_mask: u32 },
    FullScreenDifferential,
    Noop,
}
```

Extend `RefreshState`:

```rust
pub struct RefreshState {
    previous_page: Option<u32>,
    fast_refresh_count: u32,
    full_refresh_cadence: u32,
    bw_differential_ready: bool,
}
```

Initialize `bw_differential_ready: false` in `new()`.

Update `decide_with_policy(...)` so the order is:

```rust
let Some(previous_page) = self.previous_page else {
    return RefreshDecision::FullGrayscale;
};
if previous_page == target_page {
    return RefreshDecision::Noop;
}
if self.fast_refresh_count >= self.full_refresh_cadence {
    return RefreshDecision::FullGrayscale;
}
if !self.bw_differential_ready {
    return RefreshDecision::FullBwSeed;
}
match policy {
    RefreshPolicy::FullScreenDifferentialDefault => RefreshDecision::FullScreenDifferential,
    RefreshPolicy::ChunkDirtyDifferentialDefault => {
        if let Some(mask) = transition_mask {
            RefreshDecision::AdjacentDirtyPartial {
                changed_chunk_mask: mask,
            }
        } else {
            RefreshDecision::FullScreenDifferential
        }
    }
}
```

Update `record_success(...)`:

```rust
pub fn record_success(&mut self, target_page: u32, decision: RefreshDecision) {
    self.previous_page = Some(target_page);
    match decision {
        RefreshDecision::FullGrayscale => {
            self.fast_refresh_count = 0;
            self.bw_differential_ready = false;
        }
        RefreshDecision::FullBwSeed
        | RefreshDecision::AdjacentDirtyPartial { .. }
        | RefreshDecision::FullScreenDifferential => {
            self.fast_refresh_count = self.fast_refresh_count.saturating_add(1);
            self.bw_differential_ready = true;
        }
        RefreshDecision::Noop => {}
    }
}
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic bw_seed_ -- --nocapture
```

Expected result: all four tests pass.

- [ ] **Step 5: Run firmware workspace tests**

Run:

```bash
cd firmware && cargo test --workspace
```

Expected result: pass before starting Task 2.

## Task 2: Add Panel Mode State And BW Seed Rendering

**Files:**
- Modify: `firmware/crates/binbook-fw/src/display.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Write failing source-level wiring tests**

Add these tests to `firmware_logic.rs`:

```rust
#[test]
fn bw_seed_display_rendering_tracks_panel_mode_and_seed_path() {
    let display_rs = include_str!("../src/display.rs");

    assert!(display_rs.contains("pub enum PanelMode"));
    assert!(display_rs.contains("FullBwSeed"));
    assert!(display_rs.contains("stream_bw_seed_full"));
    assert!(display_rs.contains("init_grayscale_with_delay"));
    assert!(display_rs.contains("init_with_delay"));
}

#[test]
fn bw_seed_streams_target_bw_to_both_ram_planes() {
    let display_rs = include_str!("../src/display.rs");

    assert!(display_rs.contains("stream_bw_seed_full"));
    assert!(display_rs.contains("stream_plane_chunks_to_red"));
    assert!(display_rs.contains("stream_plane_chunks_to_black"));
    assert!(display_rs.contains("RefreshMode::Full"));
}
```

- [ ] **Step 2: Run tests and confirm failure**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic bw_seed_ -- --nocapture
```

Expected result: tests fail because `PanelMode` and `stream_bw_seed_full` do not exist.

- [ ] **Step 3: Add `PanelMode` and mode helpers**

In `display.rs`, add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelMode {
    Unknown,
    Grayscale,
    Bw,
}
```

Add helpers:

```rust
fn ensure_grayscale_mode<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    delay: &dyn xteink_hal::Delay,
    panel_mode: &mut PanelMode,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    if *panel_mode != PanelMode::Grayscale {
        display.init_grayscale_with_delay(delay)?;
        *panel_mode = PanelMode::Grayscale;
    }
    Ok(())
}

fn ensure_bw_mode<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    delay: &dyn xteink_hal::Delay,
    panel_mode: &mut PanelMode,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    if *panel_mode != PanelMode::Bw {
        display.init_with_delay(delay)?;
        *panel_mode = PanelMode::Bw;
    }
    Ok(())
}
```

- [ ] **Step 4: Pass panel mode through display entrypoints**

Update `display_page_with_policy(...)`, `display_page_with_chunk_dirty_probe_policy(...)`, and `display_page_with_refresh_policy(...)` to accept:

```rust
panel_mode: &mut PanelMode,
```

Pass it through wrapper calls before `target_page`.

- [ ] **Step 5: Add BW seed streaming**

Add:

```rust
fn stream_bw_seed_full<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut binbook::BinBook<&[u8], &mut [u8; 8192]>,
    book_bytes: &[u8],
    target_page: u32,
    delay: &dyn xteink_hal::Delay,
    panel_mode: &mut PanelMode,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    ensure_bw_mode(display, delay, panel_mode)?;
    let open = book.open_info();
    let pd = &book
        .page_info(target_page)
        .map_err(|_| HalError::InvalidParam)?
        .plane_dir;

    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_to_red(display, book_bytes, open.page_data_offset, pd, 2, delay)?;
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_to_black(display, book_bytes, open.page_data_offset, pd, 2, delay)?;
    display.refresh_with_delay(RefreshMode::Full, delay)
}
```

Update `stream_full_grayscale(...)` to accept `panel_mode` and call `ensure_grayscale_mode(...)` before writing planes.

Update `stream_bw_differential_full(...)` and `stream_bw_differential_chunked(...)` to accept `panel_mode` and call `ensure_bw_mode(...)` before writing BW planes.

Update the decision match:

```rust
RefreshDecision::FullBwSeed => {
    stream_bw_seed_full(display, book, book_bytes, target_page, delay, panel_mode)?;
}
```

- [ ] **Step 6: Run focused tests**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic bw_seed_ -- --nocapture
```

Expected result: both tests pass.

- [ ] **Step 7: Run firmware workspace tests**

Run:

```bash
cd firmware && cargo test --workspace
```

Expected result: pass before starting Task 3.

## Task 3: Wire Panel Mode And Decision Logging In `main.rs`

**Files:**
- Modify: `firmware/crates/binbook-fw/src/main.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Write failing source-level tests**

Add these tests to `firmware_logic.rs`:

```rust
#[test]
fn bw_seed_main_owns_panel_mode_state() {
    let main_rs = include_str!("../src/main.rs");

    assert!(main_rs.contains("PanelMode::Unknown"));
    assert!(main_rs.contains("&mut panel_mode"));
}

#[test]
fn bw_seed_firmware_logs_refresh_decisions_and_panel_mode() {
    let main_rs = include_str!("../src/main.rs");

    assert!(main_rs.contains("[REFRESH] policy=FullScreenDifferentialDefault"));
    assert!(main_rs.contains("decision="));
    assert!(main_rs.contains("[PANEL] mode="));
}
```

- [ ] **Step 2: Run tests and confirm failure**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic bw_seed_ -- --nocapture
```

Expected result: tests fail because `panel_mode` is not wired through `main.rs` yet and decision-level logging is missing.

- [ ] **Step 3: Remove eager grayscale initialization from `main.rs`**

Delete the boot-time call:

```rust
display
    .init_grayscale_with_delay(&delay)
    .expect("failed to initialize SSD1677 display");
```

Create panel mode state after `RefreshState`:

```rust
let mut refresh_state = binbook_fw::refresh::RefreshState::new();
let mut panel_mode = binbook_fw::display::PanelMode::Unknown;
render_current_page(
    &mut display,
    &mut book,
    &delay,
    &mut refresh_state,
    &mut panel_mode,
    current_page,
);
```

Update later `render_current_page(...)` calls to pass `&mut panel_mode`.

- [ ] **Step 4: Update `render_current_page(...)` signature and calls**

Add the parameter:

```rust
panel_mode: &mut binbook_fw::display::PanelMode,
```

Pass it into `display_page_with_policy(...)` and `display_page_with_chunk_dirty_probe_policy(...)`.

- [ ] **Step 5: Add decision and panel mode logging**

Add a small helper in `refresh.rs`:

```rust
impl RefreshDecision {
    pub const fn name(self) -> &'static str {
        match self {
            RefreshDecision::FullGrayscale => "FullGrayscale",
            RefreshDecision::FullBwSeed => "FullBwSeed",
            RefreshDecision::AdjacentDirtyPartial { .. } => "AdjacentDirtyPartial",
            RefreshDecision::FullScreenDifferential => "FullScreenDifferential",
            RefreshDecision::Noop => "Noop",
        }
    }
}
```

In `main.rs`, compute and log the planned decision before rendering:

```rust
let transition_mask =
    binbook_fw::display::find_transition_mask(book, refresh_state.previous_page(), page_index);
let decision = refresh_state.decide(page_index, transition_mask);
dbgprintln!(
    "[REFRESH] policy=FullScreenDifferentialDefault page={} decision={} panel_mode={:?}",
    page_index,
    decision.name(),
    panel_mode
);
dbgprintln!("[PANEL] mode={:?}", panel_mode);
```

For the probe branch, use:

```rust
let decision = refresh_state.decide_with_policy(
    page_index,
    transition_mask,
    binbook_fw::refresh::RefreshPolicy::ChunkDirtyDifferentialDefault,
);
dbgprintln!(
    "[PROBE] chunk_dirty_window page={} decision={} panel_mode={:?}",
    page_index,
    decision.name(),
    panel_mode
);
dbgprintln!("[PANEL] mode={:?}", panel_mode);
```

The display function will recompute the same decision. Keep this duplication only for debug visibility; do not let the logging path record success or mutate refresh state.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic bw_seed_ -- --nocapture
```

Expected result: both tests pass.

- [ ] **Step 7: Run firmware workspace tests**

Run:

```bash
cd firmware && cargo test --workspace
```

Expected result: pass before starting Task 4.

## Task 4: Update Documentation For BW Seed Semantics

**Files:**
- Modify: `BINBOOK_FORMAT_SPEC.md`
- Modify: `docs/specs/2026-06-26-x4-clean-differential-refresh-design.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Update `BINBOOK_FORMAT_SPEC.md`**

In the `xteink-x4-portrait` default refresh behavior section, revise the bullets so they say:

```markdown
- First render or cleanup cadence: stream plane 0 to red RAM, plane 1 to black
  RAM, then trigger grayscale refresh. This makes the visible page clean but
  does not make BW differential RAM valid.
- BW seed after grayscale: before the next partial differential page turn,
  stream the current page BW plane to both red RAM and black RAM, then trigger a
  full BW refresh.
- Clean default fast page turn: after BW seed is valid, stream the full previous
  BW plane to red RAM and the full current BW plane to black RAM, then trigger
  partial refresh. Firmware may stream this as 16-row chunks to keep RAM
  bounded.
- Chunk-dirty adjacent page turn: firmware may stream only transition-marked
  chunks when hardware verification has proven that the SSD1677 partial refresh
  is clean for that windowed update mode and BW seed state is valid.
```

- [ ] **Step 2: Update older clean-differential design**

In `docs/specs/2026-06-26-x4-clean-differential-refresh-design.md`, add a short note near "Frame Semantics":

```markdown
### BW Seed Readiness

A grayscale render must not be treated as a valid BW differential seed. After a
grayscale render or cleanup cadence, firmware must perform a full BW seed refresh
before any partial BW differential refresh. The BW seed writes the same target
page BW plane to red RAM and black RAM, then triggers a full BW refresh.
```

- [ ] **Step 3: Update `HANDOFF.md` before hardware**

Replace the top "Current State" section with current facts:

```markdown
# Handoff: Xteink X4 BW Seed Refresh

Date: 2026-06-26

## Current State

Dirty page turns still occur after the clean-differential default change. The
next implementation should follow `docs/plans/2026-06-26-x4-bw-seed-refresh.md`.

Working hypothesis: a grayscale render makes the visible page clean but does not
leave SSD1677 red RAM and black RAM in a valid BW differential seed state. The
fix is to add `FullBwSeed` between grayscale renders and partial BW differential
turns.

## Required Completion Gate

Hardware verification is the final step. Do not mark the fix complete until
host tests, firmware builds, flash, serial capture, and visual Xteink X4 results
are recorded here.
```

Keep useful historical sections below this new top section, but remove or revise stale text that says normal firmware should already be clean.

- [ ] **Step 4: Run documentation/source checks**

Run:

```bash
rg -n 'normal firmware (should|shows).*clean|should observe page.*turns' BINBOOK_FORMAT_SPEC.md docs/specs/2026-06-26-x4-clean-differential-refresh-design.md HANDOFF.md
rg -n 'T[B]D|PLACEHOLD[E]R' BINBOOK_FORMAT_SPEC.md docs/specs/2026-06-26-x4-clean-differential-refresh-design.md HANDOFF.md
```

Expected result: no stale clean-navigation claim or placeholder wording remains in touched current-state sections.

## Task 5: Run Host Verification

**Files:**
- No source changes unless a test failure identifies a bug in Tasks 1-4.

- [ ] **Step 1: Run full firmware host tests**

Run:

```bash
cd firmware && cargo test --workspace
```

Expected result: all firmware workspace tests pass.

- [ ] **Step 2: Run Python tests**

Run:

```bash
uv run pytest -q
```

Expected result: all Python tests pass or existing skipped tests remain skipped.

- [ ] **Step 3: Build normal firmware**

Run:

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
```

Expected result: release build succeeds.

- [ ] **Step 4: Build debug-log firmware**

Run:

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,debug-log --target riscv32imc-unknown-none-elf --release
```

Expected result: release build succeeds.

## Task 6: Final Hardware Verification

**Files:**
- Modify: `HANDOFF.md`

This is the final task. Do not run hardware commands in parallel with any other command that may touch the same USB target.

- [ ] **Step 1: Flash normal debug firmware**

Run with escalation or direct host access:

```bash
FW_FEATURES="firmware-bin,debug-log" firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Expected result: flash succeeds.

- [ ] **Step 2: Capture serial output**

Run with escalation or direct host access:

```bash
uv run --with pyserial --no-project python3 -c "
import serial, time, sys
ser = serial.Serial('/dev/ttyACM0', 115200, timeout=1)
ser.dtr = False; ser.rts = False; time.sleep(0.05)
ser.rts = True; time.sleep(0.05); ser.rts = False; time.sleep(0.1)
start = time.time()
while time.time() - start < 15:
    data = ser.read(ser.in_waiting or 1)
    if data:
        sys.stdout.buffer.write(data)
        sys.stdout.flush()
ser.close()
"
```

Expected serial evidence:

```text
[REFRESH] policy=FullScreenDifferentialDefault page=0 decision=FullGrayscale
[REFRESH] policy=FullScreenDifferentialDefault page=1 decision=FullBwSeed
[REFRESH] policy=FullScreenDifferentialDefault page=2 decision=FullScreenDifferential
```

- [ ] **Step 3: Perform visual page-turn verification**

On the Xteink X4:

1. Boot to page 0 and confirm the first page is clean grayscale.
2. Press `Right` or `Down` once and confirm page 1 is clean after the full BW seed refresh.
3. Press `Right` or `Down` again and confirm page 2 is clean after partial BW differential refresh.
4. Press `Left` or `Up` back to pages 1 and 0 and confirm no stale previous-page pixels remain in white areas.

- [ ] **Step 4: Update `HANDOFF.md` with final evidence**

Record:

```markdown
## X4 BW Seed Refresh Verification

- Firmware tests: `cd firmware && cargo test --workspace` -> PASS
- Python tests: `uv run pytest -q` -> PASS
- Normal build: `cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release` -> PASS
- Debug-log build: `cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,debug-log --target riscv32imc-unknown-none-elf --release` -> PASS
- Flash command: `FW_FEATURES="firmware-bin,debug-log" firmware/scripts/flash-xteink-x4-nav-probe.sh` -> PASS
- Serial evidence: include the actual `FullGrayscale`, `FullBwSeed`, and `FullScreenDifferential` lines captured from the device.
- Visual result: state whether page 0 grayscale, first BW seed turn, and later BW differential turns were clean on hardware.
- Default policy left enabled: `FullScreenDifferentialDefault`; chunk-dirty remains probe-only.
```

Expected result: `HANDOFF.md` contains enough current evidence for another agent to continue without relying on chat context.

- [ ] **Step 5: Final status**

Only after Task 6 passes, report that the dirty page-turn fix is complete. If hardware still shows dirty pages, leave the status as incomplete and record the exact failed decision sequence and visual result in `HANDOFF.md`.
