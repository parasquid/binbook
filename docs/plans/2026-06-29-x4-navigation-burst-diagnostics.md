# X4 Navigation Burst Diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` and execute this plan sequentially in the current worktree. Do not create a branch, worktree, or subagent. Maintain the todo tracker throughout execution. Every implementation task begins with a failing test and ends with the focused test passing.

**Goal:** Reproduce and localize the Xteink X4 rapid mixed-direction page-turn stall using a 16-page visual fixture, structured firmware evidence, queued serial KEY bursts, and synchronized webcam capture without changing input, queue, or display behavior.

**Architecture:** Expand the embedded navigation fixture to 16 visually unmistakable pages with a common orientation/calibration frame. Add observation-only events at ADC/input, request-start, boundary-no-op, and post-BUSY page-commit boundaries. A host CLI exercise sends real protocol-v1 KEY frames through the existing bounded FIFO, models the expected clamped page for every sequence, and validates responses, STATUS, and logs. A Python host runner records `/dev/video1`, executes the CLI exercise, and extracts labeled panel frames so logical and visible state can be compared remotely.

**Tech Stack:** Python 3.13, Pillow, `uv`, Rust `no_std`, Embassy, `esp-hal 1.1.1`, diagnostic protocol v1, Cargo, pyserial/serialport, ffmpeg, Xteink X4, SSD1677.

---

## Constraints and source of truth

- Read `AGENTS.md`, `AGENTS.local.md`, `BINBOOK_FORMAT_SPEC.md`, `docs/specs/2026-06-29-x4-staged-grayscale-design.md`, and `docs/reference/xteink-x4-agent-device-verification.md` before editing.
- Preserve unrelated changes. Work in the current branch/worktree. Do not create or switch branches.
- This is an evidence-first diagnostic change. Do **not** change ADC thresholds, the 50 ms polling cadence, the 100 ms cooldown, request capacity, page-turn semantics, grayscale timing, SSD1677 commands, or RAM-state behavior.
- Keep diagnostic protocol version `1` and the STATUS binary layout unchanged. New structured event codes are permitted.
- Boundary navigation remains clamped: Previous on page `0` stays at `0`; Next on page `15` stays at `15`.
- Keep required runtime metadata binary. Do not add JSON to `.binbook` or to firmware protocol payloads. Host-side evidence may use JSON Lines.
- Do not add framebuffer-sized firmware allocations. Keep the 16-row display streaming design.
- One process at a time may own `/dev/ttyACM0`. Webcam recording on `/dev/video1` may overlap the one serial-owning exercise process.
- Run all flash, serial, USB, webcam, and device commands with host escalation. Never use sandbox `/dev` visibility as evidence.
- Hardware verification is mandatory. Host tests and builds are necessary but not sufficient.
- Do not claim the original stall fixed in this plan. Completion means the evidence tooling works and the hardware run either passes cleanly or identifies the first divergent sequence and subsystem boundary.

## Known baseline evidence

- Live STATUS during planning reported `current_page=0`, `page_count=4`, `panel_mode=Grayscale`, `protocol_error_count=0`, and `last_error=0`.
- A fresh webcam capture at `/tmp/x4-stuck-troubleshoot.jpg` visibly showed page 0, matching STATUS.
- Recent logs showed committed transitions `3→2→1→0`, one clamped Previous request at page 0, then `0→1→2`, with successful grayscale completion and no display error.
- This baseline proves a normal boundary no-op exists; it does not reproduce the reported intermittent mixed-direction burst stall.
- The current input implementation uses synchronous ADC one-shot reads, 50 ms polling, and one global 100 ms cooldown. This is a hypothesis source only; do not refactor it in this plan.

## Evidence model

For every injected KEY sequence, the host must be able to correlate:

1. request bytes and unique protocol sequence;
2. expected clamped `from_page → target_page` computed from prior accepted requests;
3. firmware request-start event with the same sequence/from/target;
4. response opcode, sequence, status, and arrival order;
5. post-BUSY committed `PAGE_TURN`/completion event;
6. independent final STATUS;
7. webcam frame showing the large expected page number;
8. absence or presence of queue drops, protocol errors, display errors, recovery, or fault.

The first boundary where these disagree determines the follow-up:

| Evidence pattern | Localized subsystem |
|---|---|
| KEY request absent from command receipt | USB/parser/serial transport |
| Command receipt present but request-start absent | reservation/request channel |
| Request-start target differs from host model | relative navigation/FIFO state |
| Request-start correct but no completion | display engine/BUSY/recovery |
| Completion and STATUS correct but webcam page differs | SSD1677 RAM/activation/visible state |
| Serial bursts pass; physical transition is cooldown-suppressed or never decoded | ADC sampling/debounce input path |
| Only boundary requests do not move | expected clamp; report as boundary no-op |

### Task 1: Lock the 16-page fixture contract with RED tests

**Files:**
- Modify: `tests/test_nav_probe_fixture.py`
- Inspect: `firmware/scripts/build-nav-probe-fixture.py`
- Inspect: `firmware/crates/binbook-fw/fixtures/nav_probe.binbook`

- [ ] **Step 1: Add a failing page-count and index-layout test**

Add a test that opens the committed fixture and requires:

```python
def test_nav_probe_has_sixteen_numbered_pages():
    reader = BinBookReader.open(FIXTURE, validate=True)
    assert len(reader.pages) == 16
    assert len(reader.page_chunks) == 16 * 3 * 30
    assert len(reader.page_transitions) == 2 * (16 - 1)
    assert [page.page_number for page in reader.pages] == list(range(16))
```

- [ ] **Step 2: Add a failing common-frame test for all pages**

Decode every page through `_decode_x4_native_page()` and require all four 10-pixel borders, the center crosshair, four grayscale swatch centers, non-white pixels in every quadrant, and content on both sides of each centerline. Use the same exact border and swatch coordinates already proven for page 0:

```python
def test_every_nav_probe_page_keeps_orientation_and_gray_frame():
    reader = BinBookReader.open(FIXTURE)
    for page_index in range(16):
        image = _decode_x4_native_page(reader, page_index)
        assert max(image.crop((0, 0, 480, 10)).get_flattened_data()) == 0
        assert max(image.crop((0, 790, 480, 800)).get_flattened_data()) == 0
        assert max(image.crop((0, 0, 10, 800)).get_flattened_data()) == 0
        assert max(image.crop((470, 0, 480, 800)).get_flattened_data()) == 0
        assert image.getpixel((240, 400)) == 0
        assert [image.getpixel((x, 535)) for x in (140, 205, 270, 335)] == [0, 85, 170, 255]
```

- [ ] **Step 3: Add failing page-label and uniqueness tests**

The generator will expose `PAGE_LABEL_BOX = (70, 170, 410, 360)`. For each decoded page, compare that crop to the corresponding crop from every other page and require distinct bytes. Also require the full decoded pages to have 16 unique SHA-256 hashes. This verifies that each large page number is visually different without adding OCR as a test dependency.

```python
import hashlib


def test_nav_probe_pages_have_unique_labels_and_images():
    reader = BinBookReader.open(FIXTURE)
    images = [_decode_x4_native_page(reader, index) for index in range(16)]
    label_bytes = [image.crop((70, 170, 410, 360)).tobytes() for image in images]
    assert len(set(label_bytes)) == 16
    assert len({hashlib.sha256(image.tobytes()).digest() for image in images}) == 16
```

- [ ] **Step 4: Preserve explicit pattern tests**

Keep the existing page-1 checkerboard, page-2 grayscale-stripe, and page-3 large-text assertions. Adjust sample coordinates only where the common frame or label halo intentionally overlaps; sample dominant content outside `PAGE_LABEL_BOX`. Add representative assertions for pages 4–15 so a blank page or duplicated pattern fails.

Use this fixed pattern assignment:

| Page | Dominant inner pattern |
|---:|---|
| 0 | orientation/calibration target |
| 1 | 160-pixel black/white checkerboard |
| 2 | four-tone vertical bands |
| 3 | large Literata text |
| 4 | four-tone horizontal bands |
| 5 | rising `/` diagonals |
| 6 | falling `\\` diagonals |
| 7 | crosshatch |
| 8 | concentric rectangles |
| 9 | narrow vertical black/white bars |
| 10 | narrow horizontal black/white bars |
| 11 | black/dark/light/white quadrants |
| 12 | sparse black dot field |
| 13 | dense black dot field |
| 14 | asymmetric large X with top-left fill |
| 15 | inverse-phase 160-pixel checkerboard |

- [ ] **Step 5: Run the focused test and verify RED**

```bash
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline pytest -q tests/test_nav_probe_fixture.py
```

Expected: page-count, common-frame, label, uniqueness, and new-pattern tests fail against the existing four-page fixture. Existing transition-mask tests must continue to pass for the transitions that exist.

### Task 2: Generate the 16-page framed fixture

**Files:**
- Modify: `firmware/scripts/build-nav-probe-fixture.py`
- Modify: `firmware/crates/binbook-fw/fixtures/nav_probe.binbook`
- Test: `tests/test_nav_probe_fixture.py`

- [ ] **Step 1: Extract one common frame renderer**

Refactor the existing orientation drawing into helpers that operate on a supplied image and page number:

```python
PAGE_LABEL_BOX = (70, 170, 410, 360)


def _draw_common_frame(image: Image.Image, page_number: int, profile) -> None:
    """Draw the persistent orientation, coverage, ruler, and gray-calibration frame."""


def _draw_page_number(image: Image.Image, page_number: int, font) -> None:
    """Draw PAGE NN with black fill and a white halo inside PAGE_LABEL_BOX."""
```

Move the existing border, unique corner symbols, edge labels, center crosshair, rulers, 100-pixel major ticks, 50-pixel minor ticks, faint grid, orientation text, and grayscale swatches into `_draw_common_frame()`. Draw `PAGE {page_number:02d}` centered in `PAGE_LABEL_BOX` using a large font, black fill, and at least a 10-pixel white stroke so it remains readable over every pattern.

- [ ] **Step 2: Make pattern helpers draw only the inner content**

Define `CONTENT_BOX = (12, 12, 468, 788)`. Each pattern helper creates a logical 480×800 `L` image, draws its dominant pattern, then calls `_draw_common_frame()` last. Page 0 uses the orientation/calibration background; pages 1–15 use the fixed assignment from Task 1.

Do not erase or cover the final 10-pixel border, corner symbols, edge labels, center crosshair, swatches, or `PAGE NN` label after the common frame is drawn.

- [ ] **Step 3: Build all pages through the existing compiler seam**

Use one loop and preserve `dither=False`:

```python
images = [_make_page(profile, page_number) for page_number in range(16)]
pages = [
    encoded_page(pil_image_to_packed(image, profile, dither=False), 0, 0)
    for image in images
]
book_bytes = build_binbook(pages, profile, source_name="nav-probe")
```

Update the generator self-checks to require 16 pages, 1,440 chunks, and 30 transitions. Keep waveform hint `SSD1677_STAGED_GRAY2`, plane bitmap `0x07`, physical `800×480`, and three 30-chunk planes per page.

- [ ] **Step 4: Regenerate the fixture**

```bash
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline python firmware/scripts/build-nav-probe-fixture.py
```

Expected summary begins with:

```text
nav_probe.binbook: 16 pages, 1440 chunks, 30 transitions
```

- [ ] **Step 5: Run fixture tests and verify GREEN**

```bash
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline pytest -q tests/test_nav_probe_fixture.py
```

Expected: all fixture tests pass, including independent decompression and transition-mask comparison.

- [ ] **Step 6: Inspect decoded pages before continuing**

Decode all pages to `/tmp/nav-probe-pages/` and build a contact sheet with Pillow. Inspect the contact sheet with the image-viewing tool. Confirm every page number is legible and every existing/new pattern remains visible around it. Do not proceed if any number, corner label, swatch, border, or asymmetric marker is obscured.

### Task 3: Characterize current input decisions without changing behavior

**Files:**
- Modify: `firmware/crates/binbook-fw/src/input.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Write RED tests for detailed outcomes**

Add these public observation types:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDecision {
    Unchanged,
    Press(Button),
    Released,
    SuppressedByCooldown { observed: Option<Button>, elapsed_ms: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputPollOutcome {
    pub previous: Option<Button>,
    pub observed: Option<Button>,
    pub elapsed_since_last_press_ms: u32,
    pub decision: InputDecision,
}
```

Add `InputState::poll_raw_detailed(ch1, ch2, now_ms) -> InputPollOutcome`. Tests must characterize today's exact behavior:

- first transition at 50 ms is cooldown-suppressed;
- an accepted press after more than 100 ms emits exactly once;
- a direction change exactly 100 ms later is suppressed because the current comparison uses `>`;
- the suppressed observed button still becomes `last_button`;
- holding the suppressed button does not later emit a press;
- release changes observed state but emits no existing `ButtonEvent`;
- `poll_raw()` returns the same values it returned before this refactor.

- [ ] **Step 2: Run the focused tests and verify RED**

```bash
cd firmware
cargo test -p binbook-fw --features diagnostic-console input -- --nocapture
```

Expected: compile/test failure because the detailed types and method do not exist.

- [ ] **Step 3: Implement the observation wrapper with identical state transitions**

Move the existing `poll_raw()` state transition logic into `poll_raw_detailed()`. Compute `elapsed_since_last_press_ms` from the pre-update `last_press_time`, saturating to `u32::MAX`. Implement `poll_raw()` as a compatibility wrapper that maps only `InputDecision::Press(button)` to `Some(ButtonEvent::Press(button))` and returns `None` for every other decision.

Do not change:

- `cooldown_ms: 100`;
- `now_ms.saturating_sub(last_press_time) > cooldown_ms`;
- threshold constants;
- updating `last_button` after every sample;
- the lack of generated release events.

- [ ] **Step 4: Run focused and package tests and verify GREEN**

```bash
cd firmware
cargo test -p binbook-fw --features diagnostic-console input -- --nocapture
cargo test -p binbook-fw --features diagnostic-console
```

Expected: all tests pass and the old `poll_raw()` assertions remain unchanged.

### Task 4: Add structured input, start, and boundary evidence

**Files:**
- Modify: `firmware/crates/binbook-diagnostic-protocol/src/lib.rs`
- Modify: `firmware/crates/binbook-diagnostic-protocol/tests/codec.rs`
- Modify: `firmware/crates/binbook-fw/src/runtime_engine.rs`
- Modify: `firmware/crates/binbook-fw/src/runtime_aggregator.rs`
- Modify: `firmware/crates/binbook-fw/src/runtime.rs`
- Modify: `firmware/crates/binbook-fw/tests/runtime_engine.rs`
- Modify: `firmware/crates/binbook-fw/tests/runtime_aggregator.rs`
- Modify: `cli/src/lib.rs`
- Modify: `cli/tests/protocol.rs`

- [ ] **Step 1: Write RED event-code and formatter tests**

Reserve these event codes without changing protocol version:

```rust
pub const EVT_INPUT_TRANSITION: u16 = 0x0102;
pub const EVT_INPUT_DECISION: u16 = 0x0103;
pub const EVT_TURN_STARTED: u16 = 0x0205;
pub const EVT_TURN_BOUNDARY_NOOP: u16 = 0x0206;
```

Require the CLI formatter to print `INPUT_TRANSITION`, `INPUT_DECISION`, `TURN_STARTED`, and `TURN_BOUNDARY_NOOP`. Require all event codes to be unique and protocol version to remain `1`.

- [ ] **Step 2: Define exact event argument layouts**

Use the existing fixed `LogRecordPayload`; do not add bytes:

| Event | `arg0` | `arg1` | `arg2` |
|---|---|---|---|
| `INPUT_TRANSITION` | raw GPIO1 ADC | raw GPIO2 ADC | observed `Button as i32`, or `-1` for release/idle |
| `INPUT_DECISION` | observed `Button as i32`, or `-1` | `0=press`, `1=release`, `2=cooldown-suppressed` | `elapsed_since_last_press_ms`, saturated to `i32::MAX` |
| `TURN_STARTED` | protocol sequence, or `-1` for physical | source page | target page |
| `TURN_BOUNDARY_NOOP` | protocol sequence, or `-1` for physical | current page | `PageTurn as i32` |

- [ ] **Step 3: Add RED engine tests for start and boundary ordering**

Require a moving request to emit `TurnStarted` before the backend `Bw(from, target)` operation and `PageDisplayed`. Require a clamped request to emit `TurnBoundaryNoop`, perform no BW operation, preserve the current page, return success, and avoid emitting `PageDisplayed`.

For a boundary no-op during `GrayDelay`, characterize and preserve the current coordinator behavior, including any `GRAY_DELAY_CANCELLED` event. This diagnostic plan must expose that behavior, not silently correct it.

- [ ] **Step 4: Implement runtime event variants**

Add:

```rust
InputTransition { ch1: u16, ch2: u16, observed: Option<Button> },
InputDecision { observed: Option<Button>, decision: InputDecision, elapsed_ms: u32 },
TurnStarted { sequence: Option<u16>, from: u32, target: u32 },
TurnBoundaryNoop { sequence: Option<u16>, page: u32, turn: PageTurn },
```

In `DisplayEngine::request()`, compute `from` and `target` for logging without changing the existing coordinator call order. If `target == current_page`, emit `TurnBoundaryNoop`, then preserve today's request-arrival/cancellation behavior and successful no-render completion. Otherwise emit `TurnStarted` immediately before `navigate()` starts BW work. GOTO of the current page must also log a no-op equivalent while retaining its existing no-render semantics.

- [ ] **Step 5: Emit physical input evidence at origin time**

In `input_task`, call `poll_raw_detailed()`. Emit `InputTransition` only when `previous != observed`. Emit `InputDecision` for Press, Released, or SuppressedByCooldown; do not emit it for Unchanged. Preserve the existing request enqueue and epoch behavior for accepted Press decisions.

Event timestamps must use `embassy_time::Instant::now().as_millis()` at the input task, not the later aggregator time. Do not log every unchanged ADC sample.

- [ ] **Step 6: Project new events into the existing log ring**

Map events to the codes and argument layouts above. Keep `DIAG_LOG_RECORDS = 256`; the 16-key burst must fit without increasing RAM. Update CLI event names.

- [ ] **Step 7: Run focused tests and verify GREEN**

```bash
cd firmware
cargo test -p binbook-diagnostic-protocol
cargo test -p binbook-fw --features diagnostic-console --test runtime_engine
cargo test -p binbook-fw --features diagnostic-console --test runtime_aggregator
cd ../cli
cargo test --test protocol
```

Expected: all focused tests pass; protocol version and STATUS layout tests remain unchanged.

### Task 5: Preserve response arrival order in batched transport

**Files:**
- Modify: `cli/src/lib.rs`
- Modify: `cli/tests/serial_transport.rs`

- [ ] **Step 1: Write RED transport observation tests**

Add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedResponse {
    pub sequence: u16,
    pub elapsed_ms: u128,
    pub frame: Vec<u8>,
}
```

Add `send_batch_observed_io()` that returns responses in actual arrival order. Tests must cover fragmented frames, unrelated sequences, duplicate responses, non-OK status, wrong opcode, missing sequences, and responses arriving in a different order from `expected_sequences`.

The existing `send_batch_and_receive_io()` currently normalizes results into expected-sequence order; keep its public behavior unchanged and implement it through the new observed function plus an explicit reorder.

- [ ] **Step 2: Run the transport tests and verify RED**

```bash
cd cli
cargo test --features serial-device --test serial_transport -- --nocapture
```

Expected: compile failure because `ObservedResponse` and `send_batch_observed_io()` do not exist.

- [ ] **Step 3: Implement the observed batch reader**

Write the entire request byte buffer once when `inter_key_ms == 0`. Record `Instant::now()` before the write. On each validated response, append one `ObservedResponse` immediately. Reject duplicates and return a missing-sequence error at timeout.

For configurable nonzero inter-key delay, add a separate writer that writes and flushes each complete frame, sleeps the requested interval, then reads the response set. Do not split individual protocol frames.

- [ ] **Step 4: Run transport tests and verify GREEN**

```bash
cd cli
cargo test --features serial-device --test serial_transport -- --nocapture
```

Expected: all existing and new transport tests pass.

### Task 6: Add the deterministic `nav-burst` exercise

**Files:**
- Modify: `cli/Cargo.toml`
- Modify: `cli/src/lib.rs`
- Modify: `cli/src/main.rs`
- Modify: `cli/tests/hardware_diagnostic.rs`
- Modify: `cli/tests/protocol.rs`

- [ ] **Step 1: Write RED CLI parsing tests**

Add this command:

```text
binbook-cli diag exercise nav-burst \
  --port /dev/ttyACM0 \
  --rounds 10 \
  --inter-key-ms 0 \
  --output /tmp/x4-nav-burst/evidence.jsonl
```

Defaults are `rounds=10`, `inter_key_ms=0`, and no output file unless supplied. Reject `rounds=0` and values above `100`.

- [ ] **Step 2: Write RED expected-page model tests**

Define the fixed interior keys:

```rust
pub const INTERIOR_BURST: [KeyCode; 16] = [
    KeyCode::Down,
    KeyCode::Down,
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Down,
    KeyCode::Up,
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Down,
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Up,
    KeyCode::Up,
    KeyCode::Down,
];

pub const INTERIOR_EXPECTED: [u32; 16] = [
    9, 10, 9, 10, 11, 10, 9, 10, 9, 10, 11, 10, 11, 10, 9, 10,
];
```

Starting from page 8 with page_count 16 must produce exactly `INTERIOR_EXPECTED`. The boundary burst `[Up, Down, Up, Up, Down]` from page 0 must produce `[0, 1, 0, 0, 1]`.

- [ ] **Step 3: Write RED scripted-I/O exercise tests**

The scripted test must prove this request flow for every round:

1. PAGE GOTO 8 and validate response page 8;
2. LOG CLEAR and capture its returned cursor;
3. send 16 KEY requests with unique sequences in one batch;
4. collect all 16 responses in actual arrival order;
5. query STATUS;
6. paginate logs until all 16 `TURN_STARTED` and all 16 sequence-matched completion records are collected;
7. validate expected from/target/page for every sequence;
8. reject `TURN_DROPPED`, `DISPLAY_ERROR`, `RENDER_FAILURE`, recovery, nonzero `last_error`, protocol errors, or dropped log records;
9. after all interior rounds, PAGE GOTO 0, clear logs, run the five-key boundary burst, and validate the two boundary no-op events.

Do not poll LOG forever: stop when every expected sequence has both a start and completion record and the final grayscale overlay/base-sync terminal event for the round has been observed. Apply a 70-second per-round deadline.

Allocate sequences from one monotonically increasing `u16` counter starting at `1`. Increment it for PAGE, LOG, KEY, and STATUS requests and never reuse a sequence during one exercise run. Ten rounds plus the boundary case remain far below `u16::MAX`.

- [ ] **Step 4: Define stable JSONL evidence records**

Write one JSON object per line with `schema_version: 1`. Use these record kinds:

```text
run_start: schema_version, host_unix_ms, port, rounds, inter_key_ms
key: round, sequence, key, expected_from, expected_to, response_order, response_elapsed_ms, host_unix_ms
status: round, host_unix_ms, current_page, page_count, panel_mode, dropped_log_count, protocol_error_count, last_error
log: round, sequence, tick_ms, level, subsystem, event, arg0, arg1, arg2
round_result: round, expected_page, status_page, key_count, error_count
run_result: rounds_completed, key_count, boundary_key_count, error_count
```

Write JSON strings with `serde_json`; add it only to the host CLI crate, never firmware crates.

- [ ] **Step 5: Implement validation and actionable failure output**

On the first divergence, print and write:

- round and protocol sequence;
- key and expected from/target;
- last confirmed page;
- whether command receipt, turn start, completion, STATUS, and visible evidence are available;
- recent relevant log records;
- the localized subsystem according to the Evidence model table.

Return a nonzero exit status on any invariant failure. A transport acknowledgement alone must never count as a successful page turn.

- [ ] **Step 6: Run CLI tests and verify GREEN**

```bash
cd cli
cargo test
cargo test --features serial-device
```

Expected: all non-hardware tests pass; live-device tests remain ignored unless explicitly selected.

### Task 7: Add synchronized webcam capture and contact sheets

**Files:**
- Create: `firmware/scripts/run-x4-nav-burst-diagnostic.py`
- Test: `tests/test_x4_nav_burst_runner.py`

- [ ] **Step 1: Write RED argument and dry-run tests**

The runner accepts:

```text
--port /dev/ttyACM0
--video-device /dev/video1
--rounds 10
--inter-key-ms 0
--output-dir /tmp/x4-nav-burst
--dry-run
```

In dry-run mode it must print, without executing, the ffmpeg recording command, cargo CLI exercise command, crop command, frame-extraction commands, and contact-sheet path. Tests must verify user arguments are forwarded unchanged.

- [ ] **Step 2: Implement safe process ownership**

The runner must:

1. create the output directory;
2. record `video_start_unix_ms`;
3. start exactly one ffmpeg process for `/dev/video1` at native `1920x1080`, 30 fps;
4. start exactly one serial-owning CLI exercise process;
5. stream CLI stdout/stderr to both terminal and transcript files;
6. stop ffmpeg with SIGINT after the CLI exits;
7. wait for ffmpeg to finalize the MP4;
8. fail if either process exits unsuccessfully or the video/evidence files are empty.

Use this confirmed panel crop:

```text
crop=440:770:770:250
```

- [ ] **Step 3: Extract evidence frames from host timestamps**

For every `key` JSONL record, compute:

```text
offset_seconds = (key.host_unix_ms - video_start_unix_ms) / 1000
```

Extract the cropped frame at that offset and name it:

```text
round-XX-seq-YYYY-expected-ZZ.jpg
```

Also extract one cropped frame 700 ms after the last key response in each round to capture the settled grayscale page. Generate one Pillow contact sheet per round with captions containing round, sequence, key, expected page, and elapsed milliseconds.

- [ ] **Step 4: Run runner tests and syntax checks**

```bash
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline pytest -q tests/test_x4_nav_burst_runner.py
uv run python -m py_compile firmware/scripts/run-x4-nav-burst-diagnostic.py
uv run python firmware/scripts/run-x4-nav-burst-diagnostic.py --dry-run \
  --port /dev/ttyACM0 --video-device /dev/video1 --rounds 10 \
  --inter-key-ms 0 --output-dir /tmp/x4-nav-burst
```

Expected: tests and compile pass; dry-run shows one camera process and one serial exercise process, never two serial owners.

### Task 8: Document diagnostics and preserve the ADC follow-up

**Files:**
- Create: `docs/specs/2026-06-29-x4-navigation-burst-diagnostics-design.md`
- Create: `docs/ROADMAP.md`
- Modify: `docs/README.md`
- Modify: `docs/reference/xteink-x4-agent-device-verification.md`
- Modify: `docs/reference/xteink-x4-firmware-flashing.md`
- Modify: `docs/specs/2026-06-25-xteink-navigation-probe-design.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Write the approved current-state diagnostic design**

Describe the 16-page fixture, event argument layouts, deterministic bursts, expected-page model, JSONL evidence, webcam correlation, failure localization table, and evidence-first scope. State that protocol v1 and STATUS remain unchanged.

- [ ] **Step 2: Create the current roadmap**

Add an evidence-gated item titled `X4 ADC input refactor` containing these facts:

- Current firmware: synchronous one-shot ADC read loop, 50 ms Embassy timer, one global 100 ms cooldown.
- Candidate architecture: `Adc::into_async()`, interrupt-completed `read_oneshot().await`, 20 ms Embassy timer sampling, independent stable-candidate state per ADC ladder, 30 ms debounce matching the verified SquidScript/X4 reference.
- Hardware fact: ADC conversion completion can be interrupt-driven, but resistor-ladder button detection still needs periodic sampling; GPIO edge-only detection cannot reliably distinguish ladder voltages. Continuous ADC/DMA is not the default because it adds power/RAM complexity without removing debounce.
- Evidence gate: do not implement until serial/camera stress and physical input logs localize the problem to ADC/debounce.
- Acceptance: rapid mixed-direction host sequences, calibrated threshold tests, queue/drop evidence, pinned builds, flash, serial capture, and live physical-button confirmation.

Link `docs/ROADMAP.md` from `docs/README.md`.

- [ ] **Step 3: Update the authoritative hardware runbook**

Add exact build, flash, boot capture, nav-burst runner, independent STATUS, independent LOG, and evidence-inspection commands. State that webcam page labels must be compared to expected pages; CLI success alone is insufficient.

- [ ] **Step 4: Remove stale four-page references**

Search all current docs and code comments:

```bash
rg -n "four-page|4 pages|page_count=4|pages 0.*3|last page.*3" \
  README.md HANDOFF.md docs firmware cli tests \
  -g '*.md' -g '*.rs' -g '*.py' --glob '!docs/historical/**' --glob '!target/**'
```

Update current references to the 16-page fixture. Do not rewrite historical documents.

- [ ] **Step 5: Keep HANDOFF as a current-state snapshot**

After hardware verification, replace its relevant sections with exact commands, outputs, artifact paths, first divergence or clean result, verified behavior, transport-only acknowledgements, visible evidence, and remaining unverified physical-button behavior. Do not append a chronological diary.

### Task 9: Run the full host and firmware verification matrix

- [ ] **Step 1: Clean firmware artifacts to prevent stale-build masking**

```bash
cd firmware
cargo clean
```

- [ ] **Step 2: Run firmware feature-enabled and default tests**

```bash
cd firmware
cargo test --workspace --features diagnostic-console
cargo test --workspace
```

Expected: both commands pass. The feature-enabled run is mandatory coverage for diagnostic code.

- [ ] **Step 3: Run CLI tests**

```bash
cd cli
cargo test
cargo test --features serial-device
```

Expected: all non-ignored tests pass; hardware tests remain ignored.

- [ ] **Step 4: Run Python tests**

```bash
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline pytest -q
```

Expected: full Python suite passes.

- [ ] **Step 5: Build the pinned firmware variants**

```bash
cd firmware
RUSTC="$(rustup which --toolchain nightly rustc)" \
  rustup run nightly cargo build -p binbook-fw \
  --features firmware-bin \
  --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" \
  rustup run nightly cargo build -p binbook-fw \
  --features firmware-bin,diagnostic-console \
  --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" \
  rustup run nightly cargo build -p binbook-fw \
  --features firmware-bin,diagnostic-console,debug-log \
  --target riscv32imc-unknown-none-elf --release
```

Expected: all three release builds pass. Record final binary sizes and confirm the 16-page fixture fits flash comfortably.

- [ ] **Step 6: Run source and diff checks**

```bash
git diff --check
rg -n "PROTOCOL_VERSION|StatusPayload" firmware/crates/binbook-diagnostic-protocol/src/lib.rs
```

Confirm protocol version is still `1`, STATUS fields and order are unchanged, no ADC timing/debounce constants changed, and no JSON/serde dependency entered a firmware crate.

### Task 10: Perform mandatory live-device serial and webcam verification

All commands in this task require host escalation. Run serial/device commands sequentially. Webcam recording may overlap only the single nav-burst serial exercise.

- [ ] **Step 1: Flash the permanent diagnostic image**

From repo root:

```bash
FW_FEATURES="firmware-bin,diagnostic-console" \
  firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Record chip revision, flash size, application size, and final flash result. Wait for USB re-enumeration before opening serial.

- [ ] **Step 2: Capture the boot record for at least 15 seconds**

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
" | tee /tmp/x4-nav-burst-boot.txt
```

Do not start another serial process until this exits.

- [ ] **Step 3: Establish baseline HELLO and STATUS**

```bash
cd cli
cargo run --features serial-device -- diag hello --port /dev/ttyACM0
cargo run --features serial-device -- diag status --port /dev/ttyACM0
cd ..
```

HELLO must report protocol 1 and KEY/PAGE/STATUS/LOG/CRASH/DISPLAY_PROBE. STATUS must report `page_count=16`, zero protocol errors, and `last_error=0`.

- [ ] **Step 4: Run the synchronized diagnostic**

```bash
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline python \
  firmware/scripts/run-x4-nav-burst-diagnostic.py \
  --port /dev/ttyACM0 \
  --video-device /dev/video1 \
  --rounds 10 \
  --inter-key-ms 0 \
  --output-dir /tmp/x4-nav-burst
```

The runner must produce a nonempty MP4, CLI transcript, stderr transcript, JSONL evidence file, cropped key frames, settled-gray frames, and one contact sheet per round.

- [ ] **Step 5: Query independent final state**

After the runner exits:

```bash
cd cli
cargo run --features serial-device -- diag status --port /dev/ttyACM0
cargo run --features serial-device -- diag logs --port /dev/ttyACM0 --since 0
cd ..
```

Record full relevant output. If logs require pagination, continue from each printed `next_cursor` until the expected final round events are collected. Never reuse a guessed cursor.

- [ ] **Step 6: Inspect every contact sheet and the video**

Use the image viewer on each contact sheet. For every key frame, compare the visible `PAGE NN` against the JSONL `expected_to`. Inspect settled frames for four distinct grayscale swatches and correct page number. Review the MP4 around the first mismatch or, if clean, sample the beginning, middle, and end of every round.

Reject clean status if any frame shows stale page content, wrong page number, clipping, rotation, mirroring, full-panel flash, incomplete write, or gray refinement applied to the wrong page.

- [ ] **Step 7: Produce the mandatory acceptance matrix**

In `HANDOFF.md`, include one row for each requirement:

| Requirement | Implementation path | Automated test | Device/serial evidence | Webcam evidence | State |
|---|---|---|---|---|---|
| 16-page labeled fixture | generator/compiler | fixture tests | STATUS page_count | page contact sheet | verified/failed |
| 160 interior KEY requests | CLI burst | scripted exercise | start/completion sequence | per-key frames | verified/failed |
| 5 boundary KEY requests | CLI boundary burst | model tests | boundary no-op events | pages 0/1 | verified/failed |
| FIFO ordering | request channel/engine | transport/runtime tests | response and completion order | visible order | verified/failed |
| Expected vs committed page | host model/engine | model tests | TURN_STARTED/PAGE_TURN/STATUS | PAGE NN | verified/failed |
| Zero drops/errors | aggregator/status | error-path tests | counters/events | N/A | verified/failed |
| Correct staged gray | display engine | existing staged tests | overlay/base-sync events | settled swatches | verified/failed |
| Physical rapid-button path | input task | detailed outcome tests | input transition/decision logs | visible pages | verified/pending |

Any row lacking implementation, automated test, and required observed evidence remains incomplete.

- [ ] **Step 8: Perform adversarial review before any completion claim**

Attempt to disprove the result:

- compare a mid-run expected page against both STATUS/log state and its actual camera frame;
- verify a boundary no-op from page 0 rather than from an already ambiguous state;
- search JSONL for missing/duplicate sequences and non-monotonic response order;
- search logs for `TURN_DROPPED`, `DISPLAY_ERROR`, `RENDER_FAILURE`, recovery, or fault;
- inspect source to confirm the exercise used real KEY frames and did not bypass the request channel;
- confirm webcam images came from `/dev/video1`, not decoded fixture output;
- confirm no ADC cadence/debounce behavior changed in this diagnostic plan.

If the serial/camera run diverges, document the first failing sequence and localized subsystem and stop. Write a root-cause-specific fix plan before changing behavior. If serial/camera passes, leave the diagnostic image running and document that the downstream queue/display path passed while physical ADC/debounce remains the next evidence target.

## Completion criteria

This diagnostic plan is complete only when:

- all focused and full host tests pass;
- pinned firmware builds pass;
- the diagnostic image is flashed successfully;
- a 15-second boot record is captured;
- the 10-round serial burst and boundary burst execute on the live X4;
- STATUS and logs are independently queried;
- webcam video and labeled frames are inspected against expected pages;
- `HANDOFF.md` contains the acceptance matrix and exact artifact paths;
- `docs/ROADMAP.md` durably records the evidence-gated ADC refactor;
- the result is stated as either a localized first divergence or a clean downstream-path diagnostic, never as an unverified fix.
