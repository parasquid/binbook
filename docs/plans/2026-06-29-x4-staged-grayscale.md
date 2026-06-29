# Xteink X4 Fast-Turn Staged-Grayscale Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` and execute this plan sequentially in the current worktree. Do not create a branch or delegate tasks. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the visible full grayscale refresh after each fast page turn with CrossPoint-style differential grayscale refinement driven entirely by compiler-generated BinBook planes.

**Architecture:** The X4 compiler emits a black-base plane and two grayscale overlay masks identified by a new `waveform_hint`. Firmware performs a fast differential BW turn, waits 350 ms, streams the overlay masks, and activates CrossPoint's short custom LUT without resetting the SSD1677. Successfully queued turns cancel refinement before activation through a request epoch; turns received after activation remain FIFO-queued. Post-overlay BW base sync is a cancellable optimization, and explicit controller-RAM states let a page turn skip or interrupt it without entering recovery.

**Tech Stack:** Python 3.13, BinBook binary sections, Rust `no_std`, Embassy tasks/channels, SSD1677, PackBits, diagnostic protocol v1, `espflash`, pyserial, ffmpeg, Xteink X4 webcam verification.

---

## Constraints and source of truth

- Read `AGENTS.md`, `AGENTS.local.md`,
  `docs/specs/2026-06-29-x4-staged-grayscale-design.md`, and
  `docs/reference/xteink-x4-agent-device-verification.md` before editing.
- Preserve unrelated changes in the current dirty worktree.
- Do not add framebuffer-sized allocations. Continue streaming 16-row strips.
- Do not change diagnostic protocol version 1 or STATUS layout.
- Candidate BinBook compatibility is intentionally broken. Reject old X4
  native-plane semantics rather than guessing.
- Use CrossPoint community SDK commit
  `198ad267219c25c8ab84418b806c66f1fb5216a3` as the byte-level LUT and command
  reference:
  <https://github.com/crosspoint-reader/community-sdk/blob/198ad267219c25c8ab84418b806c66f1fb5216a3/libs/display/EInkDisplay/src/EInkDisplay.cpp>.
- One process at a time may own `/dev/ttyACM0`. Webcam recording on
  `/dev/video1` may overlap a serial exercise.
- Keep the todo tracker and `HANDOFF.md` current throughout execution.

### Task 1: Lock the new BinBook native-plane contract with RED tests

**Files:**
- Modify: `tests/test_x4_native_planes.py`
- Modify: `tests/test_sections.py`
- Modify: `tests/test_validation.py`
- Modify: `tests/test_nav_probe_fixture.py`

- [ ] **Step 1: Replace the absolute-plane truth-table assertions**

Add a parameterized test for canonical GRAY2 levels. For the physical pixel
that maps to RAM byte 0 bit `0x80`, require:

```python
@pytest.mark.parametrize(
    ("gray", "msb", "lsb", "base"),
    [
        (0, 0x00, 0x00, 0x7F),  # black
        (1, 0x80, 0x80, 0x7F),  # dark gray
        (2, 0x80, 0x00, 0x7F),  # light gray
        (3, 0x00, 0x00, 0xFF),  # white
    ],
)
def test_x4_staged_planes_match_controller_masks(gray, msb, lsb, base):
    packed = _storage_pixel_page(gray, 799, 0)
    actual_msb, actual_lsb, actual_base = gray2_packed_to_x4_native_planes(
        packed, 800, 480
    )
    assert actual_msb[0] == msb
    assert actual_lsb[0] == lsb
    assert actual_base[0] == base
```

Untouched overlay bits are zero; untouched fast-base bits are one. Do not
assert an ordered or noise-dither pattern.

- [ ] **Step 2: Require an explicit staged waveform hint**

Add section round-trip assertions requiring
`WaveformHint.SSD1677_STAGED_GRAY2 == 2` and the X4 profile to serialize that
value. Add validation cases that reject X4 GRAY2 files with hint `0`, hint `1`,
an unknown hint, or a plane bitmap other than `0x07`.

- [ ] **Step 3: Update fixture reconstruction expectations**

Decode canonical pixels from `(base, overlay_msb, overlay_lsb)`:

```python
if base_bit:
    gray = 3
elif not msb_bit:
    gray = 0
elif lsb_bit:
    gray = 1
else:
    gray = 2
```

Retain the full-panel orientation, border, ruler, marker, and swatch checks.

- [ ] **Step 4: Verify RED**

```bash
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline pytest -q \
  tests/test_x4_native_planes.py tests/test_sections.py \
  tests/test_validation.py tests/test_nav_probe_fixture.py
```

Expected: failures show the current flat-threshold base, absolute grayscale
planes, and `waveform_hint=1`.

### Task 2: Make the compiler emit staged controller-ready planes

**Files:**
- Modify: `binbook/constants.py`
- Modify: `binbook/profiles/base.py`
- Modify: `binbook/profiles/xteink_x4_portrait.py`
- Modify: `binbook/sections.py`
- Modify: `binbook/pixels.py`
- Modify: `binbook/reader.py`
- Modify: `binbook/page_compiler.py`

- [ ] **Step 1: Define the public waveform discriminator**

```python
class WaveformHint(IntEnum):
    UNKNOWN = 0
    SSD1677_ABSOLUTE_GRAY2 = 1
    SSD1677_STAGED_GRAY2 = 2
```

Add `waveform_hint: WaveformHint` to `DisplayProfile`, set X4 portrait to
`SSD1677_STAGED_GRAY2`, and serialize `profile.waveform_hint` instead of the
hard-coded `1`.

- [ ] **Step 2: Generate overlay masks and the black base**

Keep `gray2_packed_to_x4_native_planes()` as the public compiler seam, but make
its return values `(overlay_msb, overlay_lsb, fast_base)`:

```python
overlay_msb_rows = [bytearray(X4_ROW_BYTES) for _ in range(X4_PHYSICAL_HEIGHT)]
overlay_lsb_rows = [bytearray(X4_ROW_BYTES) for _ in range(X4_PHYSICAL_HEIGHT)]
fast_base_rows = [bytearray([0xFF] * X4_ROW_BYTES) for _ in range(X4_PHYSICAL_HEIGHT)]

if gray in (1, 2):
    _set_native_bit(overlay_msb_rows[storage_y], storage_x)
if gray == 1:
    _set_native_bit(overlay_lsb_rows[storage_y], storage_x)
if gray != 3:
    _clear_native_bit(fast_base_rows[storage_y], storage_x)
```

Do not alter logical rotation or RAM-X reversal. `encoded_page()` continues to
store these values in slots `0, 1, 2` and split each into thirty chunks.

- [ ] **Step 3: Validate and decode the staged representation**

Require staged hint `2` and bitmap `0x07` for native X4 GRAY2. Update the
Python reader/viewer decode path to reconstruct canonical levels using the Task
1 truth table.

- [ ] **Step 4: Verify GREEN**

Run the Task 1 command. Expected: all focused tests pass.

### Task 3: Rebuild the fixture and make transitions use the base plane

**Files:**
- Modify: `firmware/scripts/build-nav-probe-fixture.py`
- Modify: `firmware/crates/binbook-fw/fixtures/nav_probe.binbook`
- Inspect: `binbook/writer.py`
- Test: `tests/test_nav_probe_fixture.py`

- [ ] **Step 1: Add a RED transition-mask assertion**

For every adjacent forward and backward fixture transition, independently XOR
the decompressed slot-2 chunks and require `changed_chunk_mask` to equal the set
of chunks containing a difference. Do not derive expected values through the
writer helper under test.

- [ ] **Step 2: Verify RED against the old fixture**

```bash
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline pytest -q \
  tests/test_nav_probe_fixture.py
```

Expected: staged waveform or plane decoding fails before fixture regeneration.

- [ ] **Step 3: Confirm writer metadata compares slot 2**

Preserve `_transition_index()` and `_compare_bw_chunks()` using uncompressed
slot-2 chunks. The new independent test must prove this behavior; do not change
the writer merely to make an expected value share its implementation. Firmware
must never calculate page diffs.

- [ ] **Step 4: Regenerate and verify**

```bash
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline python \
  firmware/scripts/build-nav-probe-fixture.py
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline pytest -q \
  tests/test_nav_probe_fixture.py tests/test_x4_native_planes.py
```

Expected generator summary: four pages, 360 chunks, six transitions. Expected
tests: pass with four distinct reconstructed swatches.

### Task 4: Parse and enforce the waveform hint in reusable Rust core

**Files:**
- Create: `rust/src/display_profile.rs`
- Modify: `rust/src/lib.rs`
- Modify: `rust/tests/integration.rs`
- Modify: `firmware/crates/binbook-fw/src/display.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Write RED parser tests**

Require the rebuilt fixture to return:

```rust
assert_eq!(
    book.display_profile()?.waveform_hint,
    WAVEFORM_SSD1677_STAGED_GRAY2
);
```

Add cases for a too-short DISPLAY_PROFILE section and unknown waveform hint.
The packed hint offset is 53 bytes from section start: three 8-byte
`StringRef` values plus byte 29 of the fixed fields.

- [ ] **Step 2: Implement the bounded parser**

```rust
pub const WAVEFORM_SSD1677_STAGED_GRAY2: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayProfileInfo {
    pub physical_width: u16,
    pub physical_height: u16,
    pub waveform_hint: u16,
}
```

Read the DISPLAY_PROFILE section through the existing reader abstraction. Do
not add allocations or sibling-path hacks.

- [ ] **Step 3: Enforce the firmware contract**

Native X4 GRAY2 validation must require physical `800x480`, staged hint `2`,
and bitmap `0x07`. Return `HalError::InvalidParam` before any RAM write for a
mismatched book.

- [ ] **Step 4: Verify GREEN**

```bash
cd rust
cargo test
cd ../firmware
cargo test -p binbook-fw --features diagnostic-console
```

Expected: all parser and firmware-logic tests pass.

### Task 5: Add the short differential-grayscale SSD1677 operation

**Files:**
- Modify: `firmware/crates/ssd1677-driver/src/lib.rs`
- Test: driver unit tests in the same file

- [ ] **Step 1: Write byte-sequence RED tests**

Add tests for `load_staged_grayscale_lut()` and
`activate_staged_grayscale_async()` requiring:

- exactly 105 bytes after command `0x32`;
- voltage bytes `0x17`, `[0x41, 0xA8, 0x32]`, and `0x30` through commands
  `0x03`, `0x04`, and `0x2C`;
- display update control 1 normal mode;
- display update control 2 value `0x0C` when already powered, or `0xCC` after
  a powered-down full seed;
- master activation `0x20` followed by bounded BUSY wait;
- no reset-pin transition, `SW_RESET` (`0x12`), full update `0xF7`, or
  absolute grayscale update `0xC7`.

- [ ] **Step 2: Port the pinned LUT verbatim**

Add `SSD1677_LUT_STAGED_GRAY: [u8; 112]` from CrossPoint's
`lut_grayscale` at commit
`198ad267219c25c8ab84418b806c66f1fb5216a3`. Include an MIT attribution and
source URL beside the constant. Use bytes `0..105` for `0x32`, byte `105` for VGH,
bytes `106..109` for source voltages, and byte `109` for VCOM. Reserved bytes
`110..112` are not transmitted.

Define `pub const STAGED_GRAY_LUT_REVISION: u16 = 1`. This is a firmware
diagnostic identifier, not BinBook data. Add a unit test fixing the revision to
`1` so a future LUT change requires an intentional revision update.

- [ ] **Step 3: Keep cold initialization separate**

Do not route staged methods through `init_grayscale_async()` because it resets
the controller. Reset-based initialization remains only for cold start,
explicit full probes, and recovery.

- [ ] **Step 4: Verify GREEN**

```bash
cd firmware
cargo test -p ssd1677-driver
```

Expected: driver sequence tests pass and existing full/partial paths remain
unchanged.

### Task 6: Stream staged planes with cancellation before activation

**Files:**
- Modify: `firmware/crates/binbook-fw/src/display.rs`
- Modify: `firmware/crates/binbook-fw/tests/async_refresh.rs`

- [ ] **Step 1: Write RED streaming tests**

Add `GrayRenderOutcome::{Completed, Cancelled}` and
`BaseSyncOutcome::{Completed, Cancelled}`. Require a normal render to
emit 480 slot-1 row writes to BW RAM, 480 slot-0 row writes to RED RAM, and then
the staged activation sequence.

Add cancellation tests at strip indices `0`, `1`, `15`, and `29`. Each must:

- stop only at a 16-row boundary;
- emit neither staged control (`0x0C`/`0xCC`) nor master activation;
- return `Cancelled`, not `HalError`;
- avoid recovery and visible refresh.

- [ ] **Step 2: Implement epoch-aware strip streaming**

```rust
pub enum GrayRenderOutcome {
    Completed,
    Cancelled,
}

pub enum BaseSyncOutcome {
    Completed,
    Cancelled,
}
```

`display_staged_grayscale_async()` returns `GrayRenderOutcome`, accepts
`expected_epoch`, and accepts a bounded
epoch-reading callback. Check before every strip and immediately before LUT
activation. Write slot 1 to BW RAM and slot 0 to RED RAM. Load and activate the
short LUT only while the epoch still matches.

- [ ] **Step 3: Implement cancellable background base sync**

`sync_bw_base_async()` streams the current slot-2 base to RED/previous RAM in
16-row strips without reset or activation. It accepts the same expected epoch,
checks before every strip, and returns `BaseSyncOutcome::Cancelled` immediately
after an epoch change. Do not rewrite BW/current RAM and do not retain the old
visible-reseed strategy.

Add tests for cancellation before strips `0`, `1`, `15`, and `29`, plus normal
completion. Cancellation must emit no activation and leave the displayed page
unchanged.

- [ ] **Step 4: Verify GREEN**

```bash
cd firmware
cargo test -p binbook-fw --features diagnostic-console --test async_refresh
```

Expected: payload, strip, cancellation, and background base-sync tests pass.

### Task 7: Adapt the display engine and request pipeline

**Files:**
- Modify: `firmware/crates/binbook-fw/src/async_refresh.rs`
- Modify: `firmware/crates/binbook-fw/src/runtime_engine.rs`
- Modify: `firmware/crates/binbook-fw/src/runtime.rs`
- Modify: `firmware/crates/binbook-fw/tests/runtime_engine.rs`

- [ ] **Step 1: Write RED coordinator and engine tests**

Cover these cases:

1. BW completion at `1000 ms` schedules refinement at `1350 ms`.
2. A request at `1349 ms` cancels the delay and starts BW immediately.
3. A successful enqueue during plane streaming changes the epoch and returns
   `Cancelled` without a failure event.
4. Queue-full rejection leaves the epoch unchanged and does not cancel.
5. A request after `0x0C`/`0xCC` activation queues until BUSY completion.
6. Completed base sync enters `BwBaseReady`.
7. A request queued during overlay BUSY skips background sync and enters
   `NeedsFullBwInputs`.
8. A request during `BaseSyncInProgress` cancels it and enters
   `NeedsFullBwInputs` without recovery.
9. The next turn from `GrayOverlayResident` or `NeedsFullBwInputs` writes the
   complete old base to RED and complete target base to BW before activation.
10. Overlay or base-sync SPI/BUSY error uses one recovery; recovery failure enters
   `Fault`.

- [ ] **Step 2: Extend the backend contract**

```rust
use crate::display::{BaseSyncOutcome, GrayRenderOutcome};

fn request_epoch(&self) -> u32;
async fn render_grayscale(
    &mut self,
    page: u32,
    expected_epoch: u32,
) -> HalResult<GrayRenderOutcome>;

async fn sync_bw_base(
    &mut self,
    page: u32,
    expected_epoch: u32,
) -> HalResult<BaseSyncOutcome>;
```

Host mocks own deterministic epochs. `HardwareDisplayBackend` reads a static
`AtomicU32` with acquire ordering.

- [ ] **Step 3: Increment only after enqueue success**

For physical buttons, increment after `REQUEST_CHANNEL.try_send()` succeeds.
For protocol commands, increment inside the aggregator's atomic
reserve-and-enqueue closure only when the request channel accepts the request.
Duplicate sequence, pending-capacity, and queue-full failures must not cancel
refinement.

- [ ] **Step 4: Add explicit controller-RAM state**

Define in `runtime_engine.rs`:

```rust
pub enum ControllerRamState {
    BwBaseReady,
    GrayOverlayResident,
    BaseSyncInProgress,
    NeedsFullBwInputs,
}
```

After staged activation, enter `GrayOverlayResident`. Start background sync
only if the request epoch is unchanged and no request is already queued.
Completed sync enters `BwBaseReady`; skipped or canceled sync enters
`NeedsFullBwInputs`. Neither skipped nor canceled sync increments error
counters or invokes recovery.

- [ ] **Step 5: Replace full grayscale and reseed actions**

Rename coordinator behavior from absolute grayscale/reseed to staged
overlay/base-sync while retaining the 350 ms deadline and FIFO page semantics.
Cancellation returns to request-waiting without changing the visible page,
error counters, or completion status.

- [ ] **Step 6: Repair cold start**

Initialization cold-resets once, safely seeds page 0's slot-2 base, records BW
completion, and schedules refinement after 350 ms. It must not interpret
overlay masks as absolute grayscale planes.

- [ ] **Step 7: Verify GREEN**

```bash
cd firmware
cargo test -p binbook-fw --features diagnostic-console \
  --test runtime_engine --test runtime_aggregator --test async_refresh
```

Expected: all engine, aggregator, and async display tests pass.

### Task 8: Make diagnostics prove the real staged operation

**Files:**
- Modify: `firmware/crates/binbook-fw/src/runtime_engine.rs`
- Modify: `firmware/crates/binbook-fw/src/runtime_aggregator.rs`
- Modify: `firmware/crates/binbook-fw/src/diag.rs`
- Modify: `cli/src/lib.rs`
- Modify: `cli/tests/hardware_diagnostic.rs`

- [ ] **Step 1: Write RED scripted validator tests**

Add `diag exercise staged-gray` tests that reject:

- overlay start less than 350 ms after BW completion;
- any `0xF7` or `0xC7` activation during refinement;
- missing or reordered overlay start/completion;
- gray completion for a canceled page;
- epoch change caused by a rejected request;
- missing waveform hint `2` or firmware LUT revision `1` evidence;
- a queued turn waiting for background base sync instead of skipping or
  canceling it;
- dropped turns, mismatched completion sequences, protocol errors, or
  unrecovered display errors;
- a final page other than the scripted target.

- [ ] **Step 2: Emit origin-timestamped staged events**

Use existing 24-byte records and protocol version 1. Add stable event codes
and names for `GRAY_DELAY_CANCELLED`, `GRAY_OVERLAY_START`,
`GRAY_OVERLAY_CANCELLED`, `GRAY_OVERLAY_ACTIVATE`,
`GRAY_OVERLAY_COMPLETE`, `BW_BASE_SYNC_START`, `BW_BASE_SYNC_CANCELLED`,
`BW_BASE_SYNC_COMPLETE`, `CONTROLLER_RAM_STATE`, and `WAVEFORM_SELECTED`.
`WAVEFORM_SELECTED` carries waveform hint `2` and LUT revision `1`. The
aggregator commits events before forwarding completions.

- [ ] **Step 3: Implement the single-port exercise**

The exercise must:

1. establish a nonzero-to-page-0 baseline;
2. clear logs and record host elapsed time;
3. perform one idle turn and validate BW then staged gray after at least 350 ms;
4. perform another turn and enqueue the following turn before activation;
5. prove the intermediate refinement was canceled;
6. on another page, enqueue a turn after `GRAY_OVERLAY_ACTIVATE` but before
   overlay BUSY completion, proving the queued turn skips background sync;
7. wait for the final page's refinement and base-sync completion;
8. independently query STATUS and LOG;
9. print event sequence, controller-RAM states, waveform/LUT revision, device
   timestamps, host elapsed times, starting page,
   final page, and all counters.

- [ ] **Step 4: Verify GREEN**

```bash
cd cli
cargo test --features serial-device
```

Expected: positive and negative staged-gray scripts pass; live tests remain
ignored unless explicitly requested.

### Task 9: Update stable documentation before hardware work

**Files:**
- Modify: `BINBOOK_FORMAT_SPEC.md`
- Modify: `docs/specs/2026-06-27-x4-async-deferred-grayscale-design.md`
- Modify: `docs/reference/xteink-x4-agent-device-verification.md`
- Modify: `docs/reference/xteink-x4-firmware-flashing.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Document fast turns accurately**

Make the authoritative format spec match the staged plane table. State that
this path is not dithering: non-white pixels first become black, then a short
differential waveform selectively lightens gray pixels.

- [ ] **Step 2: Replace obsolete full-grayscale language**

Remove claims that normal refinement resets the controller, uses absolute
native grayscale, activates `0xC7`, or performs a two-plane BW reseed. Keep
full refreshes documented for cold start, explicit probes, and recovery.
Document that BinBook stores only waveform family `2` and pixel masks; LUT
revision `1` and all voltage bytes remain firmware-owned. Do not claim
temperature calibration.

- [ ] **Step 3: Update the runbook and handoff**

Add the exact serial exercise and webcam procedure from Tasks 11 and 12.
`HANDOFF.md` must distinguish host verification, transport acknowledgements,
serial state evidence, visual evidence, and unresolved criteria.

- [ ] **Step 4: Check stale references**

```bash
rg -n "absolute grayscale|0xC7|visible reseed|full grayscale|dithered BW" \
  BINBOOK_FORMAT_SPEC.md docs HANDOFF.md
```

Inspect every result. Retain a term only when it explicitly describes cold
start, recovery, a historical plan, or an unsupported alternative.

### Task 10: Run the complete host matrix

- [ ] **Step 1: Clean and test firmware**

```bash
cd firmware
cargo clean
cargo test --workspace --features diagnostic-console
cargo test --workspace
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build \
  -p binbook-fw --features firmware-bin \
  --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build \
  -p binbook-fw --features firmware-bin,diagnostic-console \
  --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build \
  -p binbook-fw --features firmware-bin,diagnostic-console,debug-log \
  --target riscv32imc-unknown-none-elf --release
```

Expected: every test and all three release builds pass.

- [ ] **Step 2: Test CLI and Python**

```bash
cd ../cli
cargo test
cargo test --features serial-device
cd ..
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline pytest -q
git diff --check
```

Expected: all non-live tests pass and no whitespace errors are reported.
Record exact counts in `HANDOFF.md`; do not reuse earlier counts.

### Task 11: Flash and verify the serial console sequentially

- [ ] **Step 1: Flash the permanent diagnostic image**

Run with host escalation:

```bash
FW_FEATURES="firmware-bin,diagnostic-console" \
  firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Record chip revision, flash size, application size, and final flash result.
Wait for `/dev/ttyACM0` to re-enumerate before opening it.

- [ ] **Step 2: Capture at least 15 seconds of boot serial**

Run with host escalation and no other serial owner:

```bash
uv run --with pyserial --no-project python3 -c '
import serial, time, sys
ser = serial.Serial("/dev/ttyACM0", 115200, timeout=1)
ser.dtr = False; ser.rts = False; time.sleep(0.05)
ser.rts = True; time.sleep(0.05); ser.rts = False; time.sleep(0.1)
start = time.time()
while time.time() - start < 15:
    data = ser.read(ser.in_waiting or 1)
    if data:
        sys.stdout.buffer.write(data); sys.stdout.flush()
ser.close()
' | tee /tmp/x4-staged-gray-boot.txt
```

Record bootloader, partition, segment-load, and application-load lines. Packet
firmware intentionally emits no debug text after boot.

- [ ] **Step 3: Verify identity and baseline state**

Run each command separately with host escalation:

```bash
cd cli
cargo run --features serial-device -- diag hello --port /dev/ttyACM0
cargo run --features serial-device -- diag status --port /dev/ttyACM0
cd ..
```

Require protocol 1, maximum frame 512, target `xteink-x4`, four fixture pages,
and zero dropped/protocol/unrecovered-error counters.

### Task 12: Capture and validate staged grayscale with the webcam

- [ ] **Step 0: Verify capture prerequisites without touching hardware**

```bash
ffmpeg -hide_banner -encoders | rg 'libx264'
ffmpeg -hide_banner -filters | rg 'crop|fps'
```

Expected: the H.264 encoder and both filters are listed. If `libx264` is not
available, use another listed lossless or visually lossless encoder and record
the exact substitution in `HANDOFF.md`; do not lower capture resolution or
frame rate.

- [ ] **Step 1: Start a native webcam recording**

Run with host escalation in an ongoing session. `/dev/video1` may overlap the
serial exercise because it is a separate device:

```bash
ffmpeg -hide_banner -loglevel error \
  -f video4linux2 -video_size 1920x1080 -framerate 30 \
  -i /dev/video1 -t 60 -c:v libx264 -preset ultrafast \
  /tmp/x4-staged-gray-full.mp4
```

- [ ] **Step 2: Run the autonomous serial exercise**

While recording, run as the sole `/dev/ttyACM0` owner:

```bash
cd cli
cargo run --features serial-device -- \
  diag exercise staged-gray --port /dev/ttyACM0 \
  | tee /tmp/x4-staged-gray-exercise.txt
cd ..
```

Require the CLI validator to pass. Do not convert a timeout into success; for a
timed-out mutation, query STATUS/logs before considering any retry.

- [ ] **Step 3: Derive the confirmed crop and evidence frames**

```bash
ffmpeg -hide_banner -loglevel error \
  -i /tmp/x4-staged-gray-full.mp4 \
  -vf "crop=440:770:770:250" -c:v libx264 -preset ultrafast \
  /tmp/x4-staged-gray-panel.mp4
mkdir -p /tmp/x4-staged-gray-frames
ffmpeg -hide_banner -loglevel error \
  -i /tmp/x4-staged-gray-panel.mp4 -vf "fps=4" \
  /tmp/x4-staged-gray-frames/frame-%04d.png
```

Inspect actual extracted frames, not the generated fixture. The crop includes
the black bezel; evaluate only the lighter active panel.

- [ ] **Step 4: Apply the visual acceptance criteria**

Correlate CLI host elapsed times and device timestamps with video frames.
Require:

1. complete full-panel fast base;
2. black, dark gray, and light gray initially black while white stays white;
3. refinement begins at least 350 ms after BW completion;
4. dark and light swatches become visibly distinct;
5. black and white regions remain stable;
6. no whole-panel flash, inversion, or clearing;
7. cancellation changes page before gray activation;
8. a turn queued during overlay BUSY starts without waiting for background
   base sync;
9. the final idle page refines and completes background base sync;
10. logs report waveform hint `2`, LUT revision `1`, and the expected
    controller-RAM state transitions.

Show the user `/tmp/x4-staged-gray-panel.mp4` and representative frame paths.
Record both the agent inspection and user verdict in `HANDOFF.md`.

- [ ] **Step 5: Independently verify final state**

After ffmpeg and the exercise exit, run serial commands sequentially:

```bash
cd cli
cargo run --features serial-device -- diag status --port /dev/ttyACM0
cargo run --features serial-device -- diag logs --port /dev/ttyACM0 --since 0
cd ..
```

Require the scripted final page, FIFO event order, intended overlay
cancellation only, skipped/canceled background sync for the queued turn, final
overlay and base-sync completion, waveform hint `2`, LUT revision `1`, and zero
dropped, protocol, or unrecovered-error counters.

### Task 13: Complete the device runbook and adversarial review

- [ ] **Step 1: Run all remaining live console checks**

Follow `docs/reference/xteink-x4-agent-device-verification.md` sequentially:
KEY/PAGE actions, logs and clear semantics, crash state, all visible probes,
ignored fragmented/batched/malformed stream tests with one test thread, and
combined diagnostic/debug USB ownership. Reflash the normal diagnostic image
afterward and reconfirm HELLO and STATUS.

- [ ] **Step 2: Restore and photograph page 0**

Run `full-refresh-current` or `page goto 0` as specified by the updated runbook,
wait for staged refinement, capture a fresh native still, apply
`crop=440:770:770:250`, and verify every orientation marker and all four
swatches.

- [ ] **Step 3: Produce the final acceptance matrix**

For every design requirement, record:

- implementation path;
- automated test;
- exact serial command and relevant output;
- independent final-state query;
- webcam file/frame evidence;
- verified, transport-only, visually unverified, or failed state.

- [ ] **Step 4: Attempt to disprove completion**

Inspect final source for reset, `SW_RESET`, `0xC7`, full-refresh, arbitrary LUT
bytes sourced from a BinBook, fabricated events, hard-coded STATUS, and
placeholder-response paths. Run discriminating
tests from nonzero pages and nonempty logs. Any contradiction keeps the task
incomplete and must be stated in `HANDOFF.md`.

- [ ] **Step 5: Final consistency check**

```bash
rg -n "2026-06-29-x4-staged-grayscale|SSD1677_STAGED_GRAY2" \
  BINBOOK_FORMAT_SPEC.md docs HANDOFF.md binbook firmware tests cli
git diff --check
```

Leave the device running the permanent `firmware-bin,diagnostic-console` image.
Do not claim completion without the serial and webcam evidence above.
