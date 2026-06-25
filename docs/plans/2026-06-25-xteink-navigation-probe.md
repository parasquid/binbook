# Xteink X4 Navigation Probe Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build and flash a four-page embedded BinBook firmware probe with Xteink directional-button navigation.

**Architecture:** Keep this milestone embedded-fixture based. Generate a deterministic four-page `GRAY2_PACKED` BinBook, render pages from `include_bytes!`, and add a small saturating page-navigation loop driven by GPIO1/GPIO2 ADC ladder buttons. Avoid increasing firmware scratch RAM by streaming embedded compressed page slices directly from the included book bytes.

**Tech Stack:** Python/Pillow BinBook generator, Rust `no_std` firmware, pinned `esp-hal` ADC one-shot reads, `ssd1677-driver`, `xteink-hal`, `cargo`, `uv`.

---

## File Structure

- Create `firmware/scripts/build-nav-probe-fixture.py`: deterministic fixture builder and self-checks.
- Create `firmware/crates/binbook-fw/fixtures/nav_probe.binbook`: generated four-page embedded probe.
- Create `firmware/scripts/flash-xteink-x4-nav-probe.sh`: build and flash wrapper for the navigation probe.
- Modify `firmware/crates/binbook-fw/src/input.rs`: raw ADC polling helper and button-to-navigation behavior.
- Modify `firmware/crates/binbook-fw/src/main.rs`: ADC setup, main navigation loop, embedded fixture selection.
- Modify `firmware/crates/binbook-fw/src/display.rs` or add a focused helper module: validate page metadata and derive compressed page slices without copying the full compressed page into scratch.
- Modify `firmware/crates/binbook-fw/tests/firmware_logic.rs`: host tests for navigation, input polling, and page-slice logic.
- Modify `docs/reference/xteink-x4-firmware-flashing.md`: current flash command and expected visible results.
- Modify `HANDOFF.md`: current status, verification, blockers, and hardware result.

## Task 1: Add Navigation Unit Tests

**Files:**
- Modify: `firmware/crates/binbook-fw/src/input.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] Add a firmware helper API in `input.rs` with this shape:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageTurn {
    Previous,
    Next,
}

pub fn page_turn_for_button(button: Button) -> Option<PageTurn> {
    match button {
        Button::Right | Button::Down => Some(PageTurn::Next),
        Button::Left | Button::Up => Some(PageTurn::Previous),
        Button::Back | Button::Select | Button::Power => None,
    }
}

pub fn apply_page_turn(current_page: u32, page_count: u32, turn: PageTurn) -> u32 {
    match turn {
        PageTurn::Next => {
            if page_count == 0 {
                0
            } else {
                current_page.saturating_add(1).min(page_count - 1)
            }
        }
        PageTurn::Previous => current_page.saturating_sub(1),
    }
}
```

- [ ] Before implementing, write tests in `firmware_logic.rs`:

```rust
use binbook_fw::input::{apply_page_turn, page_turn_for_button, PageTurn};

#[test]
fn directional_buttons_map_to_page_turns() {
    assert_eq!(page_turn_for_button(Button::Right), Some(PageTurn::Next));
    assert_eq!(page_turn_for_button(Button::Down), Some(PageTurn::Next));
    assert_eq!(page_turn_for_button(Button::Left), Some(PageTurn::Previous));
    assert_eq!(page_turn_for_button(Button::Up), Some(PageTurn::Previous));
    assert_eq!(page_turn_for_button(Button::Back), None);
    assert_eq!(page_turn_for_button(Button::Select), None);
    assert_eq!(page_turn_for_button(Button::Power), None);
}

#[test]
fn page_turns_clamp_at_book_edges() {
    assert_eq!(apply_page_turn(0, 4, PageTurn::Previous), 0);
    assert_eq!(apply_page_turn(0, 4, PageTurn::Next), 1);
    assert_eq!(apply_page_turn(2, 4, PageTurn::Previous), 1);
    assert_eq!(apply_page_turn(2, 4, PageTurn::Next), 3);
    assert_eq!(apply_page_turn(3, 4, PageTurn::Next), 3);
    assert_eq!(apply_page_turn(0, 0, PageTurn::Next), 0);
}
```

- [ ] Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic page_turn
```

Expected before implementation: compile failure or failing tests for missing symbols.

- [ ] Implement the helper exactly in `input.rs`.

- [ ] Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic page_turn
```

Expected after implementation: tests pass.

## Task 2: Add Raw ADC Polling Tests

**Files:**
- Modify: `firmware/crates/binbook-fw/src/input.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] Add this method to `InputState`:

```rust
pub fn poll_raw(&mut self, ch1: u16, ch2: u16, now_ms: u64) -> Option<ButtonEvent> {
    let button = decode_buttons(ch1, ch2);

    let event = if button != self.last_button {
        if now_ms.saturating_sub(self.last_press_time) > self.cooldown_ms as u64 {
            self.last_press_time = now_ms;
            button.map(ButtonEvent::Press)
        } else {
            None
        }
    } else {
        None
    };

    self.last_button = button;
    event
}
```

- [ ] Change the existing generic `poll()` to call `poll_raw()` after reading `AdcPin` values:

```rust
pub fn poll(
    &mut self,
    ch1: &impl AdcPin,
    ch2: &impl AdcPin,
    now_ms: u64,
) -> Option<ButtonEvent> {
    self.poll_raw(ch1.read().unwrap_or(0), ch2.read().unwrap_or(0), now_ms)
}
```

- [ ] Add tests:

```rust
use binbook_fw::input::{ButtonEvent, InputState};

#[test]
fn raw_poll_emits_one_press_per_button_transition() {
    let mut input = InputState::new();

    assert_eq!(input.poll_raw(0, 0, 0), None);
    assert_eq!(input.poll_raw(500, 0, 150), Some(ButtonEvent::Press(Button::Right)));
    assert_eq!(input.poll_raw(500, 0, 300), None);
    assert_eq!(input.poll_raw(0, 0, 450), None);
    assert_eq!(input.poll_raw(0, 500, 600), Some(ButtonEvent::Press(Button::Down)));
}

#[test]
fn raw_poll_suppresses_transitions_inside_cooldown() {
    let mut input = InputState::new();

    assert_eq!(input.poll_raw(500, 0, 50), None);
    assert_eq!(input.poll_raw(500, 0, 150), None);
}
```

- [ ] Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic raw_poll
```

Expected: tests pass.

## Task 3: Generate the Four-Page Fixture

**Files:**
- Create: `firmware/scripts/build-nav-probe-fixture.py`
- Create/update: `firmware/crates/binbook-fw/fixtures/nav_probe.binbook`

- [ ] Create a script that:
  - opens `gray2_probe.binbook` with `BinBookReader`;
  - extracts page 0 compressed payload and metadata;
  - creates checkerboard, stripes, and lorem ipsum logical images;
  - packs generated pages with `pil_image_to_packed(..., dither=False)`;
  - compresses pages with `encode_packbits`;
  - writes `nav_probe.binbook` with `build_binbook`.

- [ ] Use coarse patterns so generated image pages remain RLE-friendly:
  - checkerboard cell size: `160` logical pixels;
  - stripes: vertical or broad logical stripes using all four gray levels.

- [ ] Preserve page 1 byte-for-byte by constructing the first `EncodedPage` from the current page 0 compressed payload and CRC.

- [ ] Add script assertions:

```python
assert len(reader.pages) == 4
assert reader.pages[0].plane_dir.sizes[0] == original.pages[0].plane_dir.sizes[0]
assert reader.page_data_slice(0, 0) == original.page_data_slice(0, 0)
for page in reader.pages:
    assert page.pixel_format == PixelFormat.GRAY2_PACKED
    assert (page.stored_width, page.stored_height) == (800, 480)
    assert page.plane_dir.bitmap == 0x01
```

If `BinBookReader` does not already expose `page_data_slice`, add a local script helper that computes it from `reader.page_data` and page plane offsets without changing public Python APIs.

- [ ] Run:

```bash
uv run python firmware/scripts/build-nav-probe-fixture.py
```

Expected: script writes `firmware/crates/binbook-fw/fixtures/nav_probe.binbook` and prints a short success line with page count and compressed sizes.

## Task 4: Add Embedded Page-Slice Logic

**Files:**
- Modify: `firmware/crates/binbook-fw/src/display.rs` or create a focused helper module.
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] Add a helper that validates current supported page metadata:

```rust
pub fn is_supported_embedded_gray2_page(page: &binbook::PageInfo) -> bool {
    page.pixel_format == binbook::page_index::PIXEL_FORMAT_GRAY2_PACKED
        && page.compression_method == binbook::page_index::COMPRESSION_RLE_PACKBITS
        && page.stored_width == DISPLAY_WIDTH
        && page.stored_height == DISPLAY_HEIGHT
        && page.plane_dir.bitmap == 0x01
}
```

- [ ] Add a helper that returns the single-plane compressed slice from embedded book bytes using `book.open_info().page_data_offset` and `page.plane_dir.offsets[0]`/`sizes[0]`. It must reject out-of-bounds ranges and unsupported page metadata with `HalError::InvalidParam`.

- [ ] Add tests with a small synthetic byte buffer proving:
  - supported metadata returns the expected slice;
  - unsupported plane bitmap is rejected;
  - out-of-bounds plane range is rejected.

- [ ] Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic embedded
```

Expected: tests pass.

## Task 5: Wire Firmware Main Loop

**Files:**
- Modify: `firmware/crates/binbook-fw/src/main.rs`
- Modify: `firmware/crates/binbook-fw/Cargo.toml` only if an explicit ADC blocking dependency is unavoidable.

- [ ] Change the included fixture:

```rust
const PROBE_BOOK: &[u8] = include_bytes!("../fixtures/nav_probe.binbook");
```

- [ ] Import `esp_hal::analog::adc::{Adc, AdcConfig, Attenuation}` and configure GPIO1/GPIO2 as ADC inputs on ADC1 with `Attenuation::_11dB`.

- [ ] Add a local ADC read helper in `main.rs` that retries `adc.read_oneshot(&mut pin)` until it returns `Ok(value)`. Prefer matching `nb::Error::WouldBlock` through the type exposed by `esp-hal`; only add an explicit `nb = "1.1.0"` dependency if the compiler requires it.

- [ ] Replace the final idle loop with:
  - open `BinBook` once with small scratch for metadata;
  - render page `0`;
  - maintain `current_page: u32`;
  - poll ADC every 50-100 ms;
  - on `ButtonEvent::Press(button)`, map to page turn;
  - if page changes, render the new page.

- [ ] Keep render failures as `expect(...)` for this probe firmware, matching the current render-probe style.

- [ ] Run:

```bash
cd firmware && cargo test --workspace
```

Expected: all firmware host tests pass.

## Task 6: Add Flash Wrapper

**Files:**
- Create: `firmware/scripts/flash-xteink-x4-nav-probe.sh`
- Modify if desired: `firmware/scripts/flash-xteink-x4-gray2-probe.sh`
- Modify if desired: `firmware/scripts/flash-xteink-x4-smoke.sh`

- [ ] Create a wrapper mirroring `flash-xteink-x4-gray2-probe.sh`:
  - same root detection;
  - same `ESPFLASH` and `ESPFLASH_PORT` defaults;
  - same pinned nightly build command;
  - same `espflash flash` command.

- [ ] Keep existing smoke/gray2 wrappers working. Do not remove them in this milestone.

- [ ] Run syntax check:

```bash
bash -n firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Expected: no output and exit code 0.

## Task 7: Update Docs and Handoff

**Files:**
- Modify: `docs/reference/xteink-x4-firmware-flashing.md`
- Modify: `HANDOFF.md`

- [ ] Update the flashing doc’s current behavior from single-page GRAY2 render probe to four-page navigation probe.

- [ ] Document:
  - `firmware/scripts/flash-xteink-x4-nav-probe.sh`;
  - page order;
  - button mapping;
  - clamp behavior;
  - expected visible result after each page turn.

- [ ] Update `HANDOFF.md` with:
  - what changed;
  - verification commands and results;
  - hardware flash command;
  - observed hardware behavior or a clear note that hardware verification was not run.

## Task 8: Full Verification and Hardware Flash

**Files:**
- No source changes unless verification exposes a defect.

- [ ] Run fixture build:

```bash
uv run python --version
uv run python firmware/scripts/build-nav-probe-fixture.py
```

Expected interpreter on the current atomic Linux host: Python 3.13 from
Homebrew, selected through the repository `.python-version`. If `uv` selects a
newer interpreter and `pygame==2.6.1` tries to build from source with a missing
compiler such as `gcc-13`, fix the interpreter selection first. Do not use
`dnf` or `rpm-ostree`; use Homebrew for host tools if a real tool install is
needed.

- [ ] Run Rust tests:

```bash
cd firmware && cargo test --workspace
```

- [ ] Run release build:

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
```

- [ ] Run Python tests if local dependencies permit:

```bash
uv run pytest -q
```

- [ ] Flash with host access:

```bash
firmware/scripts/flash-xteink-x4-nav-probe.sh
```

- [ ] Verify on hardware:
  - boot shows gray bands;
  - `Right` or `Down` advances to checkerboard, stripes, then lorem ipsum;
  - `Left` or `Up` navigates backward;
  - page 1 previous and page 4 next clamp.

## Commit Guidance

Use specific paths when staging. Do not use `git add -A` or `git add .`.

Suggested commits:

```bash
git add docs/specs/2026-06-25-xteink-navigation-probe-design.md docs/plans/2026-06-25-xteink-navigation-probe.md
git commit -m "docs: plan xteink navigation probe"
```

```bash
git add firmware/crates/binbook-fw/src/input.rs firmware/crates/binbook-fw/tests/firmware_logic.rs
git commit -m "feat(binbook-fw): add page navigation state"
```

```bash
git add firmware/scripts/build-nav-probe-fixture.py firmware/crates/binbook-fw/fixtures/nav_probe.binbook
git commit -m "test(binbook-fw): add navigation probe fixture"
```

```bash
git add firmware/crates/binbook-fw/src/main.rs firmware/crates/binbook-fw/src/display.rs firmware/scripts/flash-xteink-x4-nav-probe.sh docs/reference/xteink-x4-firmware-flashing.md HANDOFF.md
git commit -m "feat(binbook-fw): navigate embedded probe pages"
```

## Assumptions

- Edge behavior clamps, not wraps.
- Page 1 remains byte-for-byte equivalent to the current gray-band probe payload.
- Page turns may block during e-ink grayscale refresh.
- Flash-backed upload/storage remains out of scope.
