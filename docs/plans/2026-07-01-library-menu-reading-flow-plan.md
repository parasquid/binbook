# Library Menu and Reading Flow (Sub-project C) Implementation Plan

> **Location note:** Authored under `.omo/plans/` (planning agent sandbox). Belongs
> at `docs/plans/2026-07-01-library-menu-reading-flow-plan.md`. Move before execution.

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a visible e-ink library menu (browse with up/down, select to open,
back to return) and a `Menu ↔ Reading` state machine that reads BinBook files off
the SD card, reusing the existing staged GRAY2 refresh for the menu with an
interruptible gray settle, plus embedded-fallback and resume-last-read.

**Architecture:** `xteink-x4-display` gains a GRAY2 `embedded-graphics`
`DrawTarget` over a 96 KB framebuffer, and a path to feed that framebuffer through
the existing staged BW→gray refresh as a UI content source. `binbook-fw`'s
`display_task` gains the `Menu ↔ Reading` mode state machine (button intents),
menu rendering, embedded `nav_probe` fallback, and an internal-flash resume
record. Reading reuses the unchanged page-streaming pipeline over A's SD-backed
`ReadAt`.

**Tech Stack:** Rust (`no_std`), `embedded-graphics` + `embedded-graphics-core`,
a bitmap font (e.g. `profont`), `embedded-text`, `gray2-render`, `binbook-core`,
`binbook-storage` (A), Embassy, esp-hal.

**Authoritative refs:** spec
`docs/specs/2026-07-01-library-menu-reading-flow-design.md`;
`crates/xteink-x4-display/src/{render.rs,page_source.rs,native.rs,engine.rs}`
(staged refresh, `DisplayPhase::GrayDelay`, `cancel_overlay`); reference research
on SquidScript (5-row viewport, stroke highlight, `fast1bpp` menu) and
crosspoint-reader (`selectorIndex` + wrap-around).

**Depends on:** Sub-projects **A** (SD `ReadAt`, `binbook-storage`) and **B**
(populated card + `StoreList` for testing).

---

## Key seam resolved in Task 0 (READ FIRST)

**The firmware has no runtime graphics, and the render pipeline only streams
BinBook page planes.** Two things must be proven together before the menu can be
drawn:

1. **GRAY2 `DrawTarget` → SSD1677 absolute RAM.** A 96 KB framebuffer (480×800 ÷
   4 bpp) must be convertible to the absolute red/black RAM rows the SSD1677
   expects, reusing `gray2_render` primitives (there is a packed-gray2 → absolute
   path used by `render_absolute_gray`; confirm/extend it). `embedded-graphics`
   text + rectangles compose into the framebuffer; the framebuffer then feeds the
   staged refresh as a UI content source (not via `Book<R: ReadAt>`).
2. **Interruptible gray settle.** Each menu transition renders the BW base
   immediately; the gray overlay is scheduled (`GrayDelay` + `gray_deadline`) and
   **cancelled on the next button** (`cancel_overlay`), restarting a BW base. The
   engine primitives (`GrayDelay`, `gray_deadline`, `cancel_overlay`,
   `begin_base_sync`) already exist — Task 0 wires a UI-framebuffer render through
   them and proves, on hardware, that navigation feels snappy and gray settles on
   pause, without breaking the reading path.

**Task 0 is a gated spike** producing a tiny firmware that draws one hardcoded
menu frame (a rect + a line of text) into the framebuffer, refreshes it staged,
and re-renders on a button press with the gray overlay cancelled. Webcam it. Do
not start Tasks 6–7 until Task 0 documents the chosen framebuffer→RAM conversion
calls + the interruptible-gray wiring.

---

## File structure

**Create:**
- `crates/xteink-x4-display/src/framebuffer.rs` (GRAY2 framebuffer + `DrawTarget`)
- `crates/xteink-x4-display/src/ui_render.rs` (framebuffer → staged refresh)
- `firmware/crates/binbook-fw/src/menu.rs` (state machine + viewport + render)
- `firmware/crates/binbook-fw/src/resume.rs` (internal-flash resume record)
- tests under each

**Modify:**
- `crates/xteink-x4-display/Cargo.toml` (`embedded-graphics`, font) + `src/lib.rs`
- `firmware/crates/binbook-fw/src/async_refresh.rs` (`DisplayRequest` menu intents)
- `firmware/crates/binbook-fw/src/runtime/display_task.rs` (mode state machine)
- `firmware/crates/binbook-fw/src/runtime/input_task.rs` (button → intent)
- `firmware/crates/binbook-fw/src/main.rs` (resume on boot)

**Responsibilities:** `xteink-x4-display` owns the reusable `DrawTarget` +
framebuffer→refresh (reusable by SquidScript). `binbook-fw` owns the menu state
machine, button mapping, fallback, and resume (app-specific).

---

## Task 0: Spike — graphics → staged refresh + interruptible gray

**Files:** throwaway firmware branch; outcome recorded inline as `## Task 0 outcome`.

- [ ] **Step 1: Add deps** to `crates/xteink-x4-display/Cargo.toml`:

```toml
embedded-graphics = { version = "0.8", default-features = false }
embedded-graphics-core = { version = "0.4", default-features = false }
profont = { version = "0.6", default-features = false }   # or chosen bitmap font
```

(Task 0 confirms versions compile `no_std` for `riscv32imc`; pin the result.)

- [ ] **Step 2: Minimal `DrawTarget`** that writes packed GRAY2 into a `[u8; 48_000]`
  (96 KB) with a 2-bit `GrayColor` (`Black=0, DarkGray=1, LightGray=2, White=3`,
  matching BinBook canonical). Hardcode one frame: clear white, draw a black
  `Rectangle` border + a `Text` line ("MENU") with `profont::PROFONT_9_POINT`.

- [ ] **Step 3: Framebuffer → absolute RAM → staged refresh.** Determine the
  `gray2_render` function that converts packed-GRAY2 rows to SSD1677 absolute
  red/black RAM (extend `render_absolute_gray`'s path if needed). Push the BW base
  first, schedule the gray overlay (`engine` enters `GrayDelay`), and on a button
  press call `cancel_overlay()` + re-render the BW base.

- [ ] **Step 4: Hardware proof + webcam.** Flash, drive one button, webcam-capture
  the BW-then-gray-settle and the cancel-on-next-press. Confirm reading
  (`nav_probe`) still renders unchanged afterward. Record:

```markdown
## Task 0 outcome
- embedded-graphics/font versions pinned: <fill>
- framebuffer→absolute-RAM call(s): <fill (gray2_render fn names)>
- interruptible-gray wiring: <fill (where cancel_overlay is invoked on new intent)>
- webcam evidence path: <fill>
```

- [ ] **Step 5: Commit deps** (the throwaway code is discarded; keep only the
  pinned deps).

```bash
git add crates/xteink-x4-display/Cargo.toml
git commit -m "build(xteink-x4-display): pin embedded-graphics + bitmap font deps"
```

---

## Task 1: `xteink-x4-display` — GRAY2 framebuffer + `DrawTarget`

**Files:** `crates/xteink-x4-display/src/framebuffer.rs`, `src/lib.rs`, tests

- [ ] **Step 1: Failing test**

```rust
#[test]
fn draw_target_writes_packed_gray2() {
    use embedded_graphics::prelude::*;
    use embedded_graphics::primitives::Rectangle;
    use embedded_graphics::Drawable;
    let mut fb = Gray2Framebuffer::new();
    fb.clear(GrayColor::White);
    Rectangle::new(Point::new(0,0), Point::new(3,0))
        .into_styled(fb.stroke_style(GrayColor::Black))
        .draw(&mut fb).unwrap();
    // 4 black pixels at row 0 = 0b00_00_00_00 in one GRAY2-packed byte (MSB-first)
    assert_eq!(fb.row(0)[0], 0b00_00_00_00);
}
```

- [ ] **Step 2: Run to verify it fails.**

- [ ] **Step 3: Implement `Gray2Framebuffer` + `GrayColor` + `DrawTarget`**
  (`fill_contiguous` + `draw_iter` writing 2-bit values MSB-first, row-major, into
  the 96 KB buffer; `size()` = 480×800 logical). No `alloc`. Expose `row(y)` for
  tests and `as_bytes()` for the renderer.

- [ ] **Step 4: Run to verify it passes** → `cargo test -p xteink-x4-display`.

- [ ] **Step 5: Commit**

```bash
git add crates/xteink-x4-display
git commit -m "feat(xteink-x4-display): GRAY2 framebuffer + embedded-graphics DrawTarget"
```

---

## Task 2: `binbook-fw` — menu state machine + viewport (host, no graphics)

**Files:** `firmware/crates/binbook-fw/src/menu.rs`, tests

Pure logic: `Mode { Menu, Reading }`, a viewport `{ top: usize, selected: usize }`,
list length, and `Button → MenuAction` math (next/prev with wrap-around; viewport
scroll at the ends; select→open; back→no-op). No display code.

- [ ] **Step 1: Failing tests** — assert: from a 7-item list at top, 5×down scrolls
  `top` to 1 and keeps `selected` at 4; down at the last item wraps to 0/0; up at
  0/0 wraps to last; `select` yields `Open(selected_index)`; `back` is a no-op.
- [ ] **Step 2: Run to verify they fail.**
- [ ] **Step 3: Implement** `MenuState`, `MenuAction`, and the transition functions.
  Keep a bounded name cache (heapless) fed by `binbook_storage::enumerate_binbooks`.
- [ ] **Step 4: Run to verify they pass** → `cargo test -p binbook-fw`.
- [ ] **Step 5: Commit**

```bash
git add firmware/crates/binbook-fw
git commit -m "feat(binbook-fw): menu mode state machine + viewport logic"
```

---

## Task 3: `binbook-fw` — button intents + `DisplayRequest` extension

**Files:** `firmware/crates/binbook-fw/src/async_refresh.rs` (`DisplayRequest`),
`src/runtime/input_task.rs`, `src/runtime/display_task.rs`, tests

- [ ] **Step 1: Extend `DisplayRequest`** with `MenuNext`/`MenuPrev`/`MenuSelect`/
  `MenuBack`/`MenuRender` (per spec; the input→intent translation split is a plan
  detail — Task 3 picks: `input_task` translates `Button`→intent using a shared
  `Mode` flag updated by `display_task` on transitions).
- [ ] **Step 2: Failing test** — feed a `Button` sequence in each mode and assert
  the intents emitted (Menu: Up/Down→Next/Prev, Select→MenuSelect, Back→no-op;
  Reading: Up/Down/Left/Right→page turns, Select/Back→MenuBack). Mode transitions
  (MenuSelect→Reading, MenuBack→Menu) flip the shared `Mode`.
- [ ] **Step 3: Implement** the translation + the shared `Mode` (an `AtomicU8` or
  embassy channel, consistent with the existing `REQUEST_EPOCH` pattern).
- [ ] **Step 4: Run to verify it passes** → `cargo test -p binbook-fw`.
- [ ] **Step 5: Commit**

```bash
git add firmware/crates/binbook-fw
git commit -m "feat(binbook-fw): button-intent mapping + menu DisplayRequest variants"
```

---

## Task 4: `binbook-fw` — menu rendering into the framebuffer (host)

**Files:** `firmware/crates/binbook-fw/src/menu.rs`, tests

Compose the 5-row viewport + stroke-highlight rect + filename text (+
`page_count`) into a `Gray2Framebuffer` via `embedded-graphics`. Rows start at
y≈76, ~52 px stride; highlight = 1 px `Rectangle` stroke around the selected row.

- [ ] **Step 1: Failing test** — given a 3-entry cache + `selected=1`, render into
  a framebuffer and assert: the selected row's highlight rect bytes are present,
  row-0 and row-2 text glyphs are non-white where expected, and only 3 rows drawn
  (no overflow). Use a tiny font + deterministic names.
- [ ] **Step 2: Run to verify it fails.**
- [ ] **Step 3: Implement** `render_menu(&mut fb, cache, viewport)` using
  `embedded_graphics::{primitives::Rectangle, text::Text, pixelcolor::BinaryColor}`
  adapted to `GrayColor`, and the pinned font.
- [ ] **Step 4: Run to verify it passes** → `cargo test -p binbook-fw`.
- [ ] **Step 5: Commit**

```bash
git add firmware/crates/binbook-fw
git commit -m "feat(binbook-fw): render 5-row menu with stroke highlight"
```

---

## Task 5: `xteink-x4-display` — framebuffer → staged refresh (uses Task 0)

**Files:** `crates/xteink-x4-display/src/ui_render.rs`, `src/lib.rs`, tests

- [ ] **Step 1: Implement** `render_ui_bw` and `render_ui_gray_overlay` that take a
  `&Gray2Framebuffer` and, using the Task-0-confirmed `gray2_render` conversion,
  push the BW base / gray overlay to the panel through the existing
  `X4Panel`/`BoardSpiDevice` SPI path. Mirror the signatures of
  `render_bw_differential` / `render_staged_overlay` but with a framebuffer source
  instead of `Book<R>`.
- [ ] **Step 2: Host test** — feed a known framebuffer, run the conversion, and
  assert the produced absolute red/black rows match a hand-computed expected set
  for a few rows (the conversion is pure; test it without hardware).
- [ ] **Step 3: Run to verify it passes** → `cargo test -p xteink-x4-display`.
- [ ] **Step 4: Commit**

```bash
git add crates/xteink-x4-display
git commit -m "feat(xteink-x4-display): render UI framebuffer through staged refresh"
```

---

## Task 6: `binbook-fw` — wire menu↔refresh + interruptible gray in `display_task`

**Files:** `firmware/crates/binbook-fw/src/runtime/display_task.rs`, tests

- [ ] **Step 1: Integrate** the `MenuState` + `render_menu` + `render_ui_*` into
  `display_task`. On a menu intent: cancel any pending gray overlay, render the new
  menu state's BW base, schedule the gray overlay. In `Reading`: open the selected
  book via `binbook_storage::open` (A's SD `ReadAt`) and page-turn through the
  existing pipeline. `MenuSelect` switches Menu→Reading; `MenuBack`/reading
  Select/Back switch Reading→Menu.
- [ ] **Step 2: Host test** — simulate an intent stream and assert the engine
  transitions through `DisplayPhase` correctly: BW render on each menu intent, gray
  scheduled, `cancel_overlay` invoked on the next intent, full gray settle when
  idle elapses. Use a mock `DisplayBackend` that records phase transitions.
- [ ] **Step 3: Run to verify it passes** → `cargo test -p binbook-fw`.
- [ ] **Step 4: Commit**

```bash
git add firmware/crates/binbook-fw
git commit -m "feat(binbook-fw): menu/reading refresh integration with interruptible gray"
```

---

## Task 7: `binbook-fw` — embedded `nav_probe` fallback + no-card/empty UX

**Files:** `firmware/crates/binbook-fw/src/menu.rs`, `src/runtime.rs`

- [ ] **Step 1:** If SD is absent or `enumerate_binbooks` yields no entries, seed
  the menu cache with a single synthetic entry backed by the embedded
  `nav_probe.binbook` (`SliceSource::new(PROBE_BOOK)`), so the device always boots
  to a readable list. Keep `PROBE_BOOK` (`include_bytes!`) as the fallback source.
- [ ] **Step 2: Host test** — with an empty/absent backend, `MenuState` reports one
  entry and `open` of it returns the embedded book.
- [ ] **Step 3: Run** → `cargo test -p binbook-fw`.
- [ ] **Step 4: Commit**

```bash
git add firmware/crates/binbook-fw
git commit -m "feat(binbook-fw): embedded nav_probe fallback for no-card/empty"
```

---

## Task 8: `binbook-fw` — resume state (internal flash)

**Files:** `firmware/crates/binbook-fw/src/resume.rs`, `src/main.rs`, tests

- [ ] **Step 1: Record layout** `{ magic, version, last_book_name[64], last_page: u32, menu_top: u32, menu_selected: u8 }`
  in a reserved internal-flash region clear of the crash sector and the existing
  `FlashStorage` table (coordinate offsets with `firmware/.../flash.rs`). Write
  only on book close / sleep entry (no per-keystroke writes).
- [ ] **Step 2: Host test** — encode/decode round-trip + wear-bound write-on-close
  logic (assert no write when only navigating).
- [ ] **Step 3: Implement** read-on-boot: if the recorded book is present (SD or
  fallback), resume to it at `last_page`; else boot to the menu at the recorded
  scroll position.
- [ ] **Step 4: Run** → `cargo test -p binbook-fw`.
- [ ] **Step 5: Commit**

```bash
git add firmware/crates/binbook-fw
git commit -m "feat(binbook-fw): persist/resume last book+page+menu in internal flash"
```

---

## Task 9: Hardware gate (webcam)

This is C's completion-evidence gate. Capture webcam frames + serial state for each.

- [ ] **Step 1: Populated card** (via B's CLI) → menu lists entries; navigate
  up/down (observe snappy BW highlight, gray settle on pause); **select** a book →
  page 0 renders correctly; page-turn forward/back; **select/back** returns to the
  menu at the same scroll position.
- [ ] **Step 2: No card / empty card** → menu shows the `nav_probe.binbook`
  fallback entry; selecting it opens and renders page 0.
- [ ] **Step 3: Resume** — open a book to a nonzero page, power-cycle; on boot the
  device resumes to the same book/page. Remove the card and reboot → falls back to
  `nav_probe` (resume record's book no longer present).
- [ ] **Step 4: Diagnostics coexistence** — `binbook diag` Key/Page opcodes still
  navigate in both modes (no regression from B).
- [ ] **Step 5: Record evidence** in `HANDOFF.md` (webcam frames, serial state).

---

## Task 10: Full workspace gate + HANDOFF

- [ ] **Step 1: Host gates**

```bash
cargo test --workspace
cargo test -p binbook-fw --features diagnostic-console,sd-storage
cargo test -p xteink-x4-display
```
Expected: all PASS.

- [ ] **Step 2: Firmware build**

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly \
  cargo build -p binbook-fw --features firmware-bin,diagnostic-console,sd-storage \
  --target riscv32imc-unknown-none-elf --release
```

- [ ] **Step 3: `HANDOFF.md`** — verified (DrawTarget/state-machine/menu-render/
  intent/refresh host tests; Task 9 webcam evidence for nav/select/page/back,
  fallback, resume); unverified (none after Task 9); known limitations (filename+
  page_count rows only; no title/author; single bounded name cache).
- [ ] **Step 4: Commit**

```bash
git add HANDOFF.md
git commit -m "docs: handoff for library menu + reading flow (sub-project C)"
```

---

## Self-review (spec coverage)

- **GRAY2 DrawTarget + 96 KB framebuffer in xteink-x4-display** → Tasks 0–1, 5. ✓
- **interruptible-gray (BW per transition, cancel on new input, settle on idle)** → Task 0 spike + Task 6. ✓
- **Menu↔Reading state machine in display_task, button intents** → Tasks 2–3, 6. ✓
- **5-row viewport, stroke highlight, filename+page_count** → Task 4. ✓
- **scrolling (library_top + selected, wrap-around, bounded cache)** → Task 2. ✓
- **reading off SD via A's ReadAt, unchanged pipeline** → Task 6. ✓
- **embedded nav_probe fallback (no-card/empty)** → Task 7. ✓
- **resume (internal flash, write-on-close)** → Task 8. ✓
- **back=no-op in menu, select/back→menu in reading** → Tasks 2–3. ✓

**Type consistency:** `Gray2Framebuffer`/`GrayColor` (Task 1) consumed by Tasks 4–6;
`MenuState`/`MenuAction` (Task 2) consumed by 4, 6, 7; `DisplayRequest` menu
variants (Task 3) match across input_task/display_task; resume record layout
(Task 8) matches main.rs read-on-boot. Engine primitives (`GrayDelay`,
`gray_deadline`, `cancel_overlay`) are the real existing names.

**No placeholders:** all code steps show code or follow a named existing pattern;
the framebuffer→absolute-RAM conversion is gated to the Task-0 spike's confirmed
`gray2_render` calls (not "TODO").

## Hardware gate (honesty)

C is **visually verified** (Task 9 webcam). Do not mark C complete without webcam
evidence of: menu navigation with the interruptible-gray behavior, correct page
render on open, back-to-menu, no-card fallback, and resume-after-power-cycle.
Reading-page correctness must be observed, not inferred from acks.
