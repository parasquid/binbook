# Library Menu and Reading Flow Design

> **Location note:** Authored under `.omo/specs/` (planning agent sandbox).
> Belongs at `docs/specs/2026-07-01-library-menu-reading-flow-design.md`.
> Move before implementation.

## Status and authority

This is an approved design for Sub-project **C** of the SD-card library feature
(**A** SD storage foundation → **B** diagnostic storage extension → **C** library
menu + reading flow). C delivers the visible e-ink library menu (browse with
up/down, select to open, back to return) and the menu↔reading state machine,
reading BinBook files off the SD card delivered by A.

`BINBOOK_FORMAT_SPEC.md` is authoritative for BinBook bytes and the X4 GRAY2
canonical values. `docs/reference/squidscript-and-xteink-reference.md` is
authoritative for X4 display/buttons/pins. C adds no BinBook format sections.

Build order is **A → B → C**. C depends on A (SD-backed `ReadAt`) and benefits
from B (a populated card for testing).

## Capability gap (confirmed during design)

The firmware today has **no runtime graphics or text rendering**. Every
`render_*` / `fill_*` function in `xteink-x4-display` is a page-pixel streamer
that reads pre-rasterized BinBook pixels and pushes them to the SSD1677. There is
no `embedded-graphics` dependency, no font, no glyph, and no rectangle/text
drawing anywhere in the firmware. **C therefore includes adding a runtime
graphics + text-rendering stack**, plus a mode state machine, plus refresh-mode
integration. This makes C the largest of the three sub-projects.

## Goals

- Draw a library menu on the e-ink display: a 5-row viewport of `.binbook` files
  with a clear highlight on the selected row.
- Navigate with up/down (wrap-around), open the selected book with select, and
  return from reading to the menu with select/back.
- Reuse the existing staged GRAY2 refresh pipeline (BW base → gray differential
  after idle) for the menu, with an interruptible gray settle so navigation feels
  snappy.
- Read selected books off the SD card (via A's `ReadAt`) through the existing
  page-rendering pipeline, unchanged.
- Always boot to something readable: fall back to the embedded `nav_probe.binbook`
  when no SD card or an empty `/books/` is present.
- Remember and resume the last book/page/menu position across power cycles.

## Non-goals (deferred)

- Title/author rows, covers, and rich metadata in the menu — v1 shows filename +
  page count (matches B's cheap `StoreList`). Titles can be added later via an
  on-demand `BookMetadata` fetch.
- A separate fast 1bpp refresh path — deliberately rejected in favor of reusing
  the staged GRAY2 pipeline (see Refresh model).
- Settings, table-of-contents, search, and any menu beyond the library list.
- The internal-flash (LittleFS) backend — roadmap (see B spec).

## Graphics foundation

- **`embedded-graphics` `DrawTarget` in `xteink-x4-display`**, backed by a **GRAY2
  framebuffer** (480×800 ÷ 4 bits/pixel = **96 KB**; acceptable on ESP32-C3).
  Use `embedded-graphics` primitives (rectangles, fill/stroke) and a compiled
  bitmap font (e.g. `profont`) for menu text. This `DrawTarget` seam is reusable
  by SquidScript's `display.rect`/`text` API.
- The framebuffer is treated as a **content source** for the existing staged
  refresh pipeline (the same row-fill path the BinBook page renderer uses), so
  menu frames refresh with identical mechanics to book pages.
- Reading is **unchanged**: BinBook pages still stream GRAY2 directly from the
  card via A's `ReadAt`; no framebuffer is allocated for reading.

## Mode state machine (`Menu ↔ Reading`)

- The mode + menu state live in **`display_task`** (which already owns the engine
  and page state), keeping all rendering logic in one place.
- Input is delivered as button **intents** rather than page-oriented requests.
  `displayRequest` is extended with menu intents
  (`MenuNext`/`MenuPrev`/`MenuSelect`/`MenuBack`/`MenuRender`). The exact split
  of translation responsibility (input task emits intents vs. forwards raw
  `Button` events for `display_task` to translate) is a plan detail — see open
  questions. Either way, `display_task` interprets intents per the current mode:
  - **Menu:** up/down → move selection (and scroll the viewport at the ends);
    select → open the selected book (load via A, switch to Reading); back →
    no-op at the top level (POWER owns sleep via its 2s hold).
  - **Reading:** up/down/left/right → page turns through the existing engine;
    select/back → return to the menu (close book, persist resume state).
- The diagnostic `Key`/`Page` opcodes continue to coexist by injecting the same
  intents (diagnostic-driven navigation still works in both modes).

## Menu rendering

- **5-row viewport** on the 480×800 logical display (matches SquidScript's proven
  geometry: rows start around y≈76, ~52 px stride).
- **Stroke-highlight rect** around the selected row (SquidScript's choice — a 1 px
  border box — chosen because it composes cleanly with the staged refresh and is
  visually clean in GRAY2).
- Rows show **filename + page count** (from B's `StoreList` / A's
  `enumerate_binbooks`). No title/author in v1.
- Composed via `embedded-graphics` text + rect into the GRAY2 framebuffer, then
  staged refresh.

## Scrolling

Viewport model (SquidScript pattern): `library_top` (index of the first visible
entry) + `library_selected` (0..=4 within the viewport). down at the viewport
bottom scrolls `library_top` by one; up at the top scrolls back. **Wrap-around**
at both list ends. The menu keeps a **bounded name cache** (it never holds the
whole library in RAM); `enumerate_binbooks` (A) is iterable/paginated.

## Refresh model (interruptible gray settle — the locked decision)

Menu navigation reuses the **same staged pipeline as BinBook rendering**, with the
gray overlay **interruptible by new input**:

1. A menu transition (up/down) renders the new menu state as the **BW base**
   (immediate visual feedback).
2. The engine enters the gray-idle wait (existing `GrayDelay` + `gray_deadline`).
3. If another button arrives during that wait, **cancel the pending gray overlay**
   (the engine already has `cancel_overlay()` / `begin_base_sync()`) and render
   the next state as a fresh BW base.
4. When input stops and idle elapses, the gray differential overlay applies,
   settling the current state to full GRAY2.

Net effect: snappy BW response on every keystroke, gray2 catches up on pause, and
**no separate 1bpp fast path** is needed. The engine primitives to support this
(`GrayDelay`, `gray_deadline`, `cancel_overlay`) already exist; the menu is a new
content source feeding the BW base. Entering the menu and opening a book also use
the full staged refresh.

## No-card / empty-library UX

If no SD card is present or `/books/` is empty, the menu shows the **embedded
`nav_probe.binbook`** as a fallback entry so the device always boots to something
readable (never an empty screen). `nav_probe.binbook` remains compiled in via
`include_bytes!` for this purpose.

## Resume (persist & resume across power cycles)

- Persist `{ last_book_name, last_page, menu_scroll_position }` to a **small
  state record in internal flash** (always present, even if the SD card is
  removed or swapped — chosen over an SD file so resume still works if the card
  changes).
- Written **infrequently**: on book close and on power-down/sleep entry, to bound
  flash wear. No write-per-keystroke.
- On boot, if the recorded book is still available (on SD or the embedded
  fallback), resume to it at the recorded page; otherwise boot to the menu.

## Crate changes

- **`xteink-x4-display`** (reusable): add the GRAY2 framebuffer type, the
  `embedded-graphics` `DrawTarget` impl, and the means to feed the framebuffer
  through the existing staged refresh as a content source. Add `embedded-graphics`
  + a bitmap font as dependencies.
- **`binbook-fw`** (app): add the `Menu ↔ Reading` mode state machine in
  `display_task`, menu rendering (via the `DrawTarget`), intent-based input
  handling, resume-state read/write to internal flash, and the embedded-fallback
  behavior.

## Testing and verification

- **Host unit tests (no hardware):**
  - `DrawTarget` correctness: render known text + rects into the GRAY2 framebuffer
    and assert expected packed GRAY2 bytes (including the highlight rect).
  - Mode state machine: feed intent sequences and assert mode transitions,
    selection/scroll math (including wrap-around and viewport scrolling), and
    resume-state round-trip.
- **Hardware evidence gate (webcam):**
  - Boot with a populated card → menu shows the list; navigate up/down (observe
    snappy BW highlight, gray settle on pause); select a book → correct page 0
    renders; page-turn forward/back; select/back returns to the menu at the same
    scroll position.
  - No-card / empty-card boot → menu shows the `nav_probe.binbook` fallback.
  - Power-cycle mid-book → resumes to the same book/page.

Capture webcam frames and serial state for each step. Per the completion-evidence
discipline, the visual result (correct page, correct highlight, correct resume)
must be independently observed, not inferred from ack responses.

## Open questions for the implementation plan

- Final `embedded-graphics` + bitmap-font crate choices and versions; confirm
  `no_std` + allocator-free rendering into the 96 KB GRAY2 framebuffer.
- Exact wiring of the framebuffer as a content source into the existing staged
  refresh (how `cancel_overlay` is triggered by a new intent mid-`GrayDelay`
  without racing the engine).
- Resume-state record layout, the internal-flash region it occupies (clear of the
  crash sector and the existing `FlashStorage` table), and the wear budget.
- Whether the input task forwards raw `Button` events or pre-translated intents,
  and how diagnostic-injected intents share the channel cleanly.
