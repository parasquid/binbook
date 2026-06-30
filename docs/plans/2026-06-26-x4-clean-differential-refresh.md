# Xteink X4 Clean Differential Refresh Implementation Plan

> Historical implementation plan. Its crate paths and API examples describe a
> superseded refresh milestone; current boundaries are in
> [`2026-06-30-rust-modular-foundation-refactor.md`](2026-06-30-rust-modular-foundation-refactor.md).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Do not delegate implementation to subagents in this repository; `AGENTS.md` asks agents to work sequentially and keep a todo tracker current. Use TDD for every code task: write the failing test first, run it and confirm the expected failure, implement the minimal change, then run the relevant tests and confirm they pass before starting the next task.

**Goal:** Make Xteink X4 BinBook page turns clean by default while preserving a hardware-proven chunk-dirty fast path.

**Architecture:** Add an explicit firmware refresh policy mode so transition masks do not automatically imply chunk-dirty partial refresh. Keep full-screen BW differential as the clean fallback: previous page plane 2 streams to SSD1677 red RAM and target page plane 2 streams to black RAM before partial refresh. Add a debug-gated hardware probe for window/chunk partial refresh; the feature is not complete until host tests pass and Xteink X4 hardware verification is documented.

**Tech Stack:** Rust `no_std` firmware crates, `ssd1677-driver`, `xteink-hal`, BinBook Rust parser, Python BinBook writer fixtures, `cargo test`, pinned nightly firmware build, Xteink X4 SSD1677 hardware.

---

## Context For The Implementer

The current implementation already supports three refresh decisions in
`firmware/crates/binbook-fw/src/refresh.rs`:

- `FullGrayscale`
- `AdjacentDirtyPartial { changed_chunk_mask }`
- `FullScreenDifferential`

The corruption risk is that `RefreshState::decide(target_page, transition_mask)`
currently returns `AdjacentDirtyPartial` whenever a transition mask exists. In
`firmware/crates/binbook-fw/src/display.rs`, that writes only changed chunks
before triggering `RefreshMode::Partial`. If SSD1677 partial refresh affects
more than those rows, or if controller RAM outside those rows is still holding
grayscale seed planes, stale previous-page pixels can survive in white areas.

The clean fallback is already conceptually present:
`stream_bw_differential_full(...)` streams all previous BW chunks to red RAM and
all target BW chunks to black RAM before a partial refresh. The implementation
work is to make that fallback the default unless chunk-dirty mode is explicitly
enabled and hardware-proven.

Do not touch unrelated user changes. `AGENTS.md` may already be modified.

## Files

- Modify: `firmware/crates/binbook-fw/src/refresh.rs`
  - Add explicit refresh policy/config types.
  - Keep policy host-testable without hardware.
- Modify: `firmware/crates/binbook-fw/src/display.rs`
  - Pass the selected policy into page rendering.
  - Keep full-screen differential and chunk-dirty streaming paths separate.
  - Add a debug/probe entrypoint if the existing driver API is sufficient.
- Modify: `firmware/crates/binbook-fw/src/main.rs`
  - Use the clean default policy for normal navigation.
  - Wire any debug probe behind a feature or explicit function path.
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`
  - Add policy tests first.
  - Add stream-orchestration tests where possible without hardware.
- Modify: `firmware/crates/ssd1677-driver/src/lib.rs` only if a small reusable
  RAM-window helper is needed for the probe.
- Modify: `BINBOOK_FORMAT_SPEC.md`
  - Clarify chunk-dirty partial refresh is conditional on hardware proof.
- Modify: `docs/specs/2026-06-26-x4-native-chunked-refresh-design.md`
  - Align prior design language with the clean default/hardware gate.
- Modify: `HANDOFF.md`
  - Record test and hardware verification evidence.

## Task 1: Make Refresh Policy Explicit

**Files:**
- Modify: `firmware/crates/binbook-fw/src/refresh.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] Add failing tests to `firmware/crates/binbook-fw/tests/firmware_logic.rs`:

```rust
#[test]
fn refresh_policy_defaults_to_full_screen_differential_when_transition_exists() {
    let mut state = RefreshState::new();
    let seed = state.decide_with_policy(0, None, RefreshPolicy::FullScreenDifferentialDefault);
    state.record_success(0, seed);

    assert_eq!(
        state.decide_with_policy(1, Some(0b101), RefreshPolicy::FullScreenDifferentialDefault),
        RefreshDecision::FullScreenDifferential
    );
}

#[test]
fn refresh_policy_uses_dirty_chunks_only_when_explicitly_enabled() {
    let mut state = RefreshState::new();
    let seed = state.decide_with_policy(0, None, RefreshPolicy::ChunkDirtyDifferentialDefault);
    state.record_success(0, seed);

    assert_eq!(
        state.decide_with_policy(1, Some(0b101), RefreshPolicy::ChunkDirtyDifferentialDefault),
        RefreshDecision::AdjacentDirtyPartial {
            changed_chunk_mask: 0b101
        }
    );
}
```

- [ ] Run the tests and confirm they fail because `RefreshPolicy` and
  `decide_with_policy` do not exist:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic refresh_policy_ -- --nocapture
```

Expected result: compile failure naming `RefreshPolicy` or `decide_with_policy`.

- [ ] Implement the minimal policy type in `refresh.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshPolicy {
    FullScreenDifferentialDefault,
    ChunkDirtyDifferentialDefault,
}

impl RefreshState {
    pub fn decide_with_policy(
        &self,
        target_page: u32,
        transition_mask: Option<u32>,
        policy: RefreshPolicy,
    ) -> RefreshDecision {
        let Some(previous_page) = self.previous_page else {
            return RefreshDecision::FullGrayscale;
        };
        if previous_page == target_page {
            return RefreshDecision::Noop;
        }
        if self.fast_refresh_count >= self.full_refresh_cadence {
            return RefreshDecision::FullGrayscale;
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
    }
}
```

- [ ] Keep the existing `decide(...)` method temporarily, but make it call the
  clean default so old call sites become safe:

```rust
pub fn decide(&self, target_page: u32, transition_mask: Option<u32>) -> RefreshDecision {
    self.decide_with_policy(
        target_page,
        transition_mask,
        RefreshPolicy::FullScreenDifferentialDefault,
    )
}
```

- [ ] Import `RefreshPolicy` in `firmware_logic.rs` and run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic refresh_policy_ -- --nocapture
```

Expected result: all refresh policy tests pass.

- [ ] Run the full firmware workspace tests before moving on:

```bash
cd firmware && cargo test --workspace
```

Expected result: all tests pass.

## Task 2: Route Normal Page Rendering Through The Clean Default

**Files:**
- Modify: `firmware/crates/binbook-fw/src/display.rs`
- Modify: `firmware/crates/binbook-fw/src/main.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] Add a failing source-level test in `firmware_logic.rs` that prevents the
  normal render path from silently using the old implicit policy:

```rust
#[test]
fn display_page_with_policy_uses_explicit_refresh_policy() {
    let display_rs = include_str!("../src/display.rs");

    assert!(display_rs.contains("RefreshPolicy"));
    assert!(display_rs.contains("decide_with_policy"));
    assert!(!display_rs.contains("refresh_state.decide(target_page, transition_mask)"));
}
```

- [ ] Run and confirm failure:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic display_page_with_policy_uses_explicit_refresh_policy -- --nocapture
```

Expected result: test fails because `display.rs` still calls `refresh_state.decide(...)`.

- [ ] Change `display_page_with_policy(...)` in `display.rs` to accept a
  `RefreshPolicy` argument:

```rust
pub fn display_page_with_policy<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut binbook::BinBook<&[u8], &mut [u8; 8192]>,
    book_bytes: &[u8],
    delay: &dyn xteink_hal::Delay,
    refresh_state: &mut RefreshState,
    refresh_policy: RefreshPolicy,
    target_page: u32,
) -> HalResult<()>
```

- [ ] Update the decision line:

```rust
let decision = refresh_state.decide_with_policy(target_page, transition_mask, refresh_policy);
```

- [ ] Update imports at the top of `display.rs`:

```rust
use crate::refresh::{RefreshDecision, RefreshPolicy, RefreshState, X4_CHUNK_COUNT};
```

- [ ] Update `render_current_page(...)` in `main.rs` to pass the clean default:

```rust
binbook_fw::refresh::RefreshPolicy::FullScreenDifferentialDefault,
```

- [ ] Run the focused test:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic display_page_with_policy_uses_explicit_refresh_policy -- --nocapture
```

Expected result: test passes.

- [ ] Run the full firmware workspace tests before moving on:

```bash
cd firmware && cargo test --workspace
```

Expected result: all tests pass.

## Task 3: Add A Chunk-Dirty Debug Entry Point

**Files:**
- Modify: `firmware/crates/binbook-fw/src/display.rs`
- Modify: `firmware/crates/binbook-fw/src/main.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] Add failing source-level tests proving normal firmware defaults to the
  clean policy and chunk-dirty remains reachable only by explicit opt-in:

```rust
#[test]
fn chunk_dirty_normal_navigation_uses_full_screen_differential_default() {
    let main_rs = include_str!("../src/main.rs");

    assert!(main_rs.contains("RefreshPolicy::FullScreenDifferentialDefault"));
    assert!(main_rs.contains("display_page_with_policy"));
}

#[test]
fn chunk_dirty_policy_is_reserved_for_probe_or_debug_paths() {
    let display_rs = include_str!("../src/display.rs");

    assert!(display_rs.contains("display_page_with_refresh_policy"));
    assert!(display_rs.contains("RefreshPolicy::ChunkDirtyDifferentialDefault"));
}
```

- [ ] Run and confirm failure:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic chunk_dirty -- --nocapture
```

Expected result: at least one test fails because the helper does not exist yet.

- [ ] Rename the current configurable function to
  `display_page_with_refresh_policy(...)`, and add a clean-default wrapper:

```rust
pub fn display_page_with_policy<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut binbook::BinBook<&[u8], &mut [u8; 8192]>,
    book_bytes: &[u8],
    delay: &dyn xteink_hal::Delay,
    refresh_state: &mut RefreshState,
    target_page: u32,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    display_page_with_refresh_policy(
        display,
        book,
        book_bytes,
        delay,
        refresh_state,
        RefreshPolicy::FullScreenDifferentialDefault,
        target_page,
    )
}
```

- [ ] Add a debug/probe wrapper that explicitly opts into chunk-dirty policy:

```rust
pub fn display_page_with_chunk_dirty_probe_policy<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut binbook::BinBook<&[u8], &mut [u8; 8192]>,
    book_bytes: &[u8],
    delay: &dyn xteink_hal::Delay,
    refresh_state: &mut RefreshState,
    target_page: u32,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    display_page_with_refresh_policy(
        display,
        book,
        book_bytes,
        delay,
        refresh_state,
        RefreshPolicy::ChunkDirtyDifferentialDefault,
        target_page,
    )
}
```

- [ ] Keep `main.rs` calling the clean-default `display_page_with_policy(...)`
  wrapper, not the chunk-dirty probe wrapper.

- [ ] Run focused tests:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic chunk_dirty -- --nocapture
```

Expected result: both tests pass.

- [ ] Run the full firmware workspace tests before moving on:

```bash
cd firmware && cargo test --workspace
```

Expected result: all tests pass.

## Task 4: Add Hardware-Probe Logging And Build Surface

**Files:**
- Modify: `firmware/crates/binbook-fw/src/main.rs`
- Modify: `firmware/crates/binbook-fw/Cargo.toml`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] Add failing tests that require a compile-time probe surface and readable
  debug-log markers:

```rust
#[test]
fn firmware_has_chunk_dirty_probe_feature_gate() {
    let cargo_toml = include_str!("../Cargo.toml");

    assert!(cargo_toml.contains("chunk-dirty-probe"));
}

#[test]
fn firmware_logs_refresh_policy_and_probe_steps() {
    let main_rs = include_str!("../src/main.rs");

    assert!(main_rs.contains("[REFRESH] policy="));
    assert!(main_rs.contains("[PROBE] chunk_dirty_window"));
}
```

- [ ] Run and confirm failure:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic probe -- --nocapture
```

Expected result: tests fail because the feature/log markers are not present.

- [ ] Add the feature to `firmware/crates/binbook-fw/Cargo.toml`:

```toml
chunk-dirty-probe = []
```

- [ ] In `main.rs`, log the normal policy before rendering:

```rust
dbgprintln!("[REFRESH] policy=FullScreenDifferentialDefault page={}", page_index);
```

- [ ] Add a `#[cfg(feature = "chunk-dirty-probe")]` probe branch that uses
  `display_page_with_chunk_dirty_probe_policy(...)` for page turns and logs:

```rust
#[cfg(feature = "chunk-dirty-probe")]
dbgprintln!("[PROBE] chunk_dirty_window page={}", page_index);
```

The probe may reuse the existing navigation fixture and buttons; do not add
hardware-specific fake paths. The purpose is to let the operator build a probe
firmware that visibly exercises chunk-dirty page turns and reports the selected
mode over serial.

- [ ] Run focused tests:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic probe -- --nocapture
```

Expected result: both tests pass.

- [ ] Run the full firmware workspace tests before moving on:

```bash
cd firmware && cargo test --workspace
```

Expected result: all tests pass.

## Task 5: Update Specs And Reference Docs

**Files:**
- Modify: `BINBOOK_FORMAT_SPEC.md`
- Modify: `docs/specs/2026-06-26-x4-native-chunked-refresh-design.md`
- Modify: `docs/reference/xteink-x4-firmware-flashing.md`

- [ ] Update `BINBOOK_FORMAT_SPEC.md` X4 default refresh behavior to say:

```markdown
- Clean default fast page turn: stream the previous page BW plane to red RAM and
  the current page BW plane to black RAM for the full screen, then trigger
  partial refresh. Firmware may stream this as 16-row chunks to keep RAM bounded.
- Chunk-dirty adjacent page turn: firmware may stream only transition-marked
  chunks when hardware verification has proven that the SSD1677 partial refresh
  is clean for that windowed update mode.
```

- [ ] Update `docs/specs/2026-06-26-x4-native-chunked-refresh-design.md` so
  its refresh policy section no longer states adjacent dirty partial is
  unconditional default behavior.

- [ ] Add a short command note to `docs/reference/xteink-x4-firmware-flashing.md`
  documenting the probe build:

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,debug-log,chunk-dirty-probe --target riscv32imc-unknown-none-elf --release
```

- [ ] Run firmware tests after docs changes, because source-level tests include
  source/doc-facing strings:

```bash
cd firmware && cargo test --workspace
```

Expected result: all tests pass.

## Task 6: Build Firmware

**Files:**
- No source edits expected.

- [ ] Run the normal firmware build with the pinned nightly toolchain:

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
```

Expected result: build succeeds and produces
`firmware/target/riscv32imc-unknown-none-elf/release/binbook-fw`.

- [ ] Run the probe firmware build:

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,debug-log,chunk-dirty-probe --target riscv32imc-unknown-none-elf --release
```

Expected result: build succeeds.

- [ ] If either build fails, fix the failing code with TDD where possible, rerun
  the focused test, rerun `cd firmware && cargo test --workspace`, then rerun
  both builds before continuing.

## Task 7: Hardware Verification Gate

**Files:**
- Modify: `HANDOFF.md`

- [ ] Flash and run the probe firmware on the Xteink X4 using the repository's
  current flash script or documented command. Hardware/serial commands require
  host access; use escalated execution up front.

- [ ] Capture serial output with the documented monitor command:

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

- [ ] Page through the probe fixture and visually inspect whether old-page
  pixels remain in white areas after chunk-dirty page turns.

- [ ] Flash and run the normal firmware build, then page through the same
  fixture. Verify the clean default does not show previous-page corruption.

- [ ] Update `HANDOFF.md` with:

```markdown
## X4 Clean Differential Refresh Verification

- Firmware tests: `cd firmware && cargo test --workspace` -> PASS
- Normal build: paste the exact command used -> PASS
- Probe build: paste the exact command used -> PASS
- Probe hardware run: paste the exact flash/run command used -> PASS or FAIL
- Serial evidence: paste the key `[REFRESH]` and `[PROBE]` lines
- Visual result: state whether chunk-dirty was clean or corrupt, and whether the normal clean default was clean or corrupt
- Default policy left enabled: `FullScreenDifferentialDefault` unless probe proved chunk-dirty clean.
```

- [ ] The feature is not complete if this task is not done. If hardware is not
  connected or host access fails, stop and record the blocker in `HANDOFF.md`
  instead of claiming completion.

## Final Verification

- [ ] Run Python tests because BinBook metadata/docs are part of the same repo
  contract:

```bash
uv run pytest -q
```

Expected result: all Python tests pass.

- [ ] Run firmware host tests one last time:

```bash
cd firmware && cargo test --workspace
```

Expected result: all firmware tests pass.

- [ ] Confirm the firmware builds still pass:

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
```

Expected result: build succeeds.

- [ ] Confirm `HANDOFF.md` includes hardware verification evidence. Without
  that evidence, the feature remains incomplete.
