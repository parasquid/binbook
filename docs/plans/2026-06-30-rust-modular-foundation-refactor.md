# Rust Modular Foundation Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` and execute this plan sequentially in the main thread. Do not delegate. Keep the todo tracker current after every task.

**Goal:** Replace the coupled Rust parser/display/firmware implementation with independently testable `no_std` crates that a future Rust-native SquidScript firmware can import directly.

**Architecture:** Use one root Cargo workspace and five layered library crates: `binbook-core`, `binbook-decompress`, `gray2-render`, `ssd1677-driver`, and `xteink-x4-display`. Keep ESP32-C3, Embassy, input, storage, and diagnostic coordination in `binbook-fw`; remove the custom `xteink-hal` transport abstraction in favor of `embedded-hal` 1.0, `embedded-hal-async`, and `embedded-storage` traits.

**Tech Stack:** Rust 2021, `no_std`, embedded-hal 1.0, embedded-hal-async 1.0, embedded-storage, Embassy, esp-hal, optional `lz4_flex`, Cargo, Python 3.13/uv for reference fixtures, Xteink X4 hardware, pyserial, ffmpeg webcam capture.

---

## Non-negotiable constraints

- `BINBOOK_FORMAT_SPEC.md` remains authoritative. Do not change the BinBook 0.1 wire format, section layouts, canonical color meanings, X4 plane slots, or chunk geometry.
- Default X4 output remains `GRAY2_PACKED`; `GRAY1_PACKED` remains explicit opt-in. Do not add `GRAY4_PACKED` output for the X4 profile.
- Reusable crates must compile without `std`, allocation, ESP, Embassy, diagnostic, CLI, or repository-file dependencies.
- All temporary decode/render buffers are caller-owned. Do not add full-page buffers, hidden fixed 8 KiB scratch arrays, or larger stacks to make tests pass.
- The existing `binbook/rust` path and Rust API may break. Do not add a compatibility facade. SquidScript is not modified in this plan.
- Python encoder and CLI user behavior must remain unchanged. General Python and CLI cleanup belongs in the roadmap task near the end.
- Every behavior change follows red, green, refactor. Run the failing test and confirm the failure reason before writing production code.
- Do not replace behavioral tests with source-text assertions. Delete source-shape assertions only after a compile-time, API, or behavioral test protects the same contract.
- Run hardware, serial, and webcam commands sequentially. Only one process may own `/dev/ttyACM0` at a time.
- A successful flash, response, or CLI exit is not display evidence. Verify final state independently through STATUS/log queries and fresh webcam captures.
- Do not claim completion until `HANDOFF.md` contains an acceptance matrix linking each requirement to implementation, automated tests, serial evidence, and webcam evidence.
- Before every commit, inspect `git diff --name-only` and stage only the files named by that task. The commit snippets use file globs only within the task-owned directories; never stage a whole repository subtree or unrelated concurrent changes.

## Target workspace and dependency graph

Create this workspace shape:

```text
Cargo.toml
Cargo.lock
crates/
├── binbook-core/
├── binbook-decompress/
├── gray2-render/
├── ssd1677-driver/
└── xteink-x4-display/
cli/
firmware/
├── crates/binbook-diagnostic-protocol/
└── crates/binbook-fw/
```

Required dependency direction:

```text
binbook-decompress -> binbook-core

gray2-render
ssd1677-driver -> embedded-hal, embedded-hal-async

xteink-x4-display
├── binbook-core
├── binbook-decompress
├── gray2-render
└── ssd1677-driver

binbook-fw
├── xteink-x4-display
├── binbook-diagnostic-protocol
├── embedded-storage
├── Embassy
└── esp-hal

binbook-cli
├── binbook-core
└── binbook-diagnostic-protocol
```

Forbidden edges:

- Library crates to `binbook-fw`, CLI, diagnostics, Embassy, esp-hal, or files outside their crate.
- `binbook-core` to decompression, rendering, or controller crates.
- `gray2-render` to BinBook or SSD1677 crates.
- `ssd1677-driver` to BinBook, X4, firmware, or custom HAL traits.

## Locked public interfaces

The implementation may add private helpers, but it must preserve these concepts and ownership rules.

### `binbook-core`

```rust
pub trait ReadAt {
    type Error;

    fn len(&mut self) -> Result<u64, Self::Error>;
    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), Self::Error>;
}

pub struct Book<R: ReadAt> {
    source: R,
    header: Header,
    sections: SectionDirectory,
}

pub enum Error<E> {
    Source(E),
    Format(FormatError),
    BufferTooSmall { required: usize, provided: usize },
}

pub struct PageNumber(u32);
pub enum PlaneSlot { OverlayMsb, OverlayLsb, FastBase, Reserved }
pub struct ChunkIndex(u8);
pub struct FileOffset(u64);
pub struct ByteLength(u32);
```

`Book::open` receives the source and caller-owned section-table scratch. Record accessors receive caller-owned fixed record buffers. Plane/chunk reads accept explicit descriptors and output slices. No public method returns a borrowed aggregate of concatenated compressed planes.

Remove the current placeholder `Info::info()`, `PageRef`, `decompress_page()`, and public raw-number APIs where a typed identifier is required. Metadata methods must either parse the real section/string data or not exist.

### `binbook-decompress`

```rust
pub fn decode_exact(
    method: binbook_core::CompressionMethod,
    input: &[u8],
    output: &mut [u8],
) -> Result<(), DecodeError>;

pub struct PackBitsDecoder {
    run: Option<PackBitsRun>,
}
```

`decode_exact` must reject short and overlong output, malformed runs, unsupported compression, and disabled LZ4. `PackBitsDecoder` must support input/run boundaries crossing output strips without rescanning earlier bytes.

### `gray2-render`

Expose typed pure operations for:

- canonical GRAY2 byte to absolute SSD1677 red/black plane bits;
- staged X4 `overlay_msb`, `overlay_lsb`, and `fast_base` reconstruction;
- ordered-dither BW conversion;
- row conversion into caller-owned output buffers.

The crate must not know about BinBook sections, SSD1677 commands, panel refreshes, or ESP hardware.

### `ssd1677-driver`

```rust
pub struct Ssd1677<SPI, DC, RST, BUSY> {
    spi: SPI,
    dc: DC,
    reset: RST,
    busy: BUSY,
    config: PanelConfig,
    state: ControllerState,
}
pub struct PanelConfig { /* dimensions and controller setup values */ }
pub enum RefreshMode { Full, Partial, Grayscale, StagedGrayscale }
pub enum ControllerState { Unknown, Powered, PoweredDown }
```

- `SPI` implements `embedded_hal::spi::SpiDevice<u8>` and owns chip-select behavior.
- `DC` and `RST` implement `embedded_hal::digital::OutputPin`.
- `BUSY` implements `embedded_hal::digital::InputPin`.
- Synchronous waits use `embedded_hal::delay::DelayNs`; asynchronous waits use `embedded_hal_async::delay::DelayNs`.
- Driver errors preserve SPI, pin, timeout, window, and buffer failure categories.
- The driver owns controller commands; X4 waveform/LUT policy lives in `xteink-x4-display` configuration.

### `xteink-x4-display`

```rust
pub struct RenderBuffers<'a> {
    pub compressed: &'a mut [u8],
    pub decoded_rows: &'a mut [u8],
}

pub enum PageTurn { Previous, Next, First, Last }
pub enum Probe { ClearWhite, WindowCorners, FullRefreshCurrent }
pub enum RenderRequest { Initialize, PageTurn(PageTurn), Goto(PageNumber), Probe(Probe) }
pub enum RenderOutcome { Completed(PageNumber), Cancelled(PageNumber), BoundaryNoop(PageNumber) }
pub struct DisplayEngine {
    current_page: PageNumber,
    page_count: u32,
    refresh: RefreshState,
    panel: PanelMode,
    controller: ControllerState,
}
pub trait EventSink { fn emit(&mut self, event: DisplayEvent); }
```

This crate owns X4 dimensions/rotation, three-plane validation, 16-row chunks, staged-gray cancellation, BW base synchronization, refresh policy, controller state, and display probes. It must not own Embassy channels, diagnostic protocol frames, ESP peripherals, flash partitions, or application lifecycle.

---

## Task 0: Protect and checkpoint the current verified worktree

**Files:** Existing modified and untracked navigation/diagnostic files shown by `git status --short`; do not include any refactor files because none should exist yet.

- [ ] Record `git status --short` and `git diff --stat` in the execution notes. Confirm the changes match the completed boundary FIFO/navigation diagnostic described by `HANDOFF.md`.
- [ ] Run the current baseline before committing:

```bash
uv run pytest -q
cargo test --manifest-path rust/Cargo.toml
cargo test --manifest-path firmware/Cargo.toml --workspace
cargo test --manifest-path firmware/Cargo.toml -p binbook-fw --features diagnostic-console
cargo test --manifest-path cli/Cargo.toml
cargo test --manifest-path cli/Cargo.toml --features serial-device
git diff --check
```

Expected baseline: Python `98 passed, 26 skipped`; Rust core `19 passed`; all firmware and CLI suites pass. Warnings are existing cleanup debt, not permission to introduce new warnings.

- [ ] Verify `HANDOFF.md` contains the live sequence-matched boundary burst, STATUS, log, and webcam evidence. If any claimed artifact is missing, repeat the corresponding device check before committing.
- [ ] Stage each file listed in the captured status with explicit path arguments. Never use `git add .` or `git add -A`, and do not stage a path that was absent from the checkpoint inventory.
- [ ] Commit the checkpoint:

```bash
git commit -m "fix(firmware): preserve queued boundary page turns"
```

- [ ] Confirm `git status --short` is clean before starting file moves. If it is not clean, stop and separate the remaining user changes instead of absorbing them into the refactor.

## Task 1: Save the architecture contract and characterization matrix

**Files:**

- Create: `docs/specs/2026-06-30-rust-modular-foundation-design.md`
- Create: `docs/reference/rust-crate-architecture.md`
- Modify: `AGENTS.md`

- [ ] Write the design spec using the dependency graph, public interfaces, exclusions, and hardware gates in this plan. State explicitly that the old `binbook/rust` path is intentionally removed and SquidScript is not migrated.
- [ ] Write the reference document as current-state architecture that will become true by the end of the refactor. Include crate responsibilities, allowed dependencies, buffer ownership, error boundaries, and exact build/test commands.
- [ ] Update the `AGENTS.md` modularity table to include `xteink-x4-display`, replace custom transport traits with embedded-hal interfaces, and state that firmware application code may not contain format parsing, PackBits logic, plane conversion, or SSD1677 commands.
- [ ] Run a documentation contradiction scan:

```bash
rg -n "binbook-core|binbook-decompress|gray2-render|ssd1677-driver|xteink-x4-display|xteink-hal" AGENTS.md docs README.md
```

Expected: all new names have one consistent responsibility; no document claims `xteink-hal` remains a required transport dependency.

- [ ] Commit:

```bash
git add AGENTS.md docs/specs/2026-06-30-rust-modular-foundation-design.md docs/reference/rust-crate-architecture.md
git commit -m "docs: define reusable Rust crate architecture"
```

## Task 2: Establish the root Cargo workspace without changing behavior

**Files:**

- Create: `Cargo.toml`
- Create: `Cargo.lock` through Cargo resolution
- Modify: `firmware/scripts/flash-xteink-x4-nav-probe.sh`
- Remove after verification: `rust/Cargo.lock`, `firmware/Cargo.lock`, `cli/Cargo.lock`, `firmware/Cargo.toml`

- [ ] Add a temporary workspace using the current package locations: `rust`, `cli`, and every crate under `firmware/crates`. Centralize edition, license, repository metadata, and dependency versions in `[workspace.package]` and `[workspace.dependencies]`.
- [ ] Run the root workspace before deleting nested manifests/lockfiles:

```bash
cargo test --workspace
```

Expected: the same host tests pass from the repository root. A nested-workspace error is the intended red signal until `firmware/Cargo.toml` is removed and members are owned by the root workspace.

- [ ] Remove the nested firmware workspace manifest and nested lockfiles. Update package manifests to inherit workspace metadata and dependencies.
- [ ] Update the flash script so the firmware artifact is `${ROOT}/target/riscv32imc-unknown-none-elf/release/binbook-fw`; do not leave it pointing at `firmware/target`.
- [ ] Run:

```bash
cargo test --workspace
cd firmware && cargo test --workspace
cd ../cli && cargo test
cd ..
git diff --check
```

Expected: all commands resolve the root workspace and pass.

- [ ] Commit:

```bash
git add Cargo.toml Cargo.lock rust/Cargo.toml cli/Cargo.toml firmware/crates/*/Cargo.toml firmware/scripts/flash-xteink-x4-nav-probe.sh
git add -u rust/Cargo.lock firmware/Cargo.lock cli/Cargo.lock firmware/Cargo.toml
git commit -m "refactor(rust): establish unified Cargo workspace"
```

## Task 3: Extract `binbook-core` with typed, source-backed access

**Files:**

- Create: `crates/binbook-core/Cargo.toml`
- Create focused modules under `crates/binbook-core/src/`: `lib.rs`, `source.rs`, `types.rs`, `error.rs`, `header.rs`, `section.rs`, `profile.rs`, `page.rs`, `navigation.rs`, `chunk.rs`, `transition.rs`, `strings.rs`, `book.rs`
- Create tests under `crates/binbook-core/tests/`: `open.rs`, `records.rs`, `sources.rs`, `metadata.rs`, `bounds.rs`
- Migrate fixtures into `crates/binbook-core/tests/fixtures/`
- Remove after migration: `rust/src/`, `rust/tests/`, `rust/Cargo.toml`

- [ ] Write failing tests first for:
  - `ReadAt::len` and `read_exact_at` source failures remaining distinguishable from format failures;
  - invalid magic, version, section sizes, file bounds, required sections, and string references;
  - typed page, plane, chunk, and transition indices rejecting out-of-range values;
  - every X4 plane slot and chunk descriptor from the current fixture;
  - real display profile and book metadata values, with no empty/zero placeholder aggregate;
  - output buffers one byte too small returning the exact required/provided sizes.
- [ ] Run the new crate tests and confirm they fail because `binbook_core` and the locked APIs do not exist:

```bash
cargo test -p binbook-core
```

- [ ] Implement the minimum parser by moving and splitting current parsing logic. `Book` stores only the source and validated section/index locations. Pass scratch buffers into operations; do not store a generic scratch object in `Book`.
- [ ] Implement a slice-backed `ReadAt` adapter for host tests and embedded fixtures. Bounds failures must use a typed source error.
- [ ] Delete `Info::info()`, `PageRef`, aggregate compressed-plane concatenation, and decompression code from the core crate.
- [ ] Run:

```bash
cargo test -p binbook-core
cargo check -p binbook-core --no-default-features --target riscv32imc-unknown-none-elf
cargo clippy -p binbook-core --all-targets -- -D warnings
```

Expected: all tests pass; the target check has no `std` or allocator requirement; Clippy is warning-free.

- [ ] Update CLI and firmware imports only enough to compile against `binbook-core`; do not introduce decompression or display changes yet.
- [ ] Run `cargo test --workspace` and commit:

```bash
git add Cargo.toml Cargo.lock crates/binbook-core/Cargo.toml crates/binbook-core/src/*.rs crates/binbook-core/tests/*.rs crates/binbook-core/tests/fixtures/* cli/Cargo.toml cli/src/*.rs firmware/crates/binbook-fw/Cargo.toml firmware/crates/binbook-fw/src/*.rs
git add -u rust/Cargo.toml rust/src rust/tests
git commit -m "refactor(binbook): extract typed format core"
```

## Task 4: Extract exact and streaming decompression

**Files:**

- Create: `crates/binbook-decompress/Cargo.toml`
- Create: `crates/binbook-decompress/src/{lib,error,packbits,lz4}.rs`
- Create: `crates/binbook-decompress/tests/{packbits,streaming,lz4}.rs`
- Modify: `crates/binbook-core` only to expose the typed `CompressionMethod`

- [ ] Write failing tests for literal runs, repeat runs, `0x80` no-op runs, malformed truncated runs, exact output sizing, runs crossing input/output strip boundaries, disabled LZ4, and LZ4 round trips when the feature is enabled.
- [ ] Add the multi-plane regression that the old API missed: three independently compressed planes with different bytes must decode into three distinct caller-selected outputs. There must be no API that silently overwrites one plane with the next.
- [ ] Run and confirm the intended red state:

```bash
cargo test -p binbook-decompress
```

- [ ] Implement `decode_exact` and the stateful `PackBitsDecoder`. Keep LZ4 behind a crate feature and map decoder failures to typed `DecodeError` variants.
- [ ] Move all PackBits and LZ4 production logic out of the former core and firmware display modules.
- [ ] Run:

```bash
cargo test -p binbook-decompress
cargo test -p binbook-decompress --features lz4
cargo check -p binbook-decompress --no-default-features --target riscv32imc-unknown-none-elf
cargo clippy -p binbook-decompress --all-targets --all-features -- -D warnings
cargo test --workspace
```

- [ ] Commit:

```bash
git add Cargo.toml Cargo.lock crates/binbook-core/Cargo.toml crates/binbook-core/src/*.rs crates/binbook-decompress/Cargo.toml crates/binbook-decompress/src/*.rs crates/binbook-decompress/tests/*.rs firmware/crates/binbook-fw/Cargo.toml firmware/crates/binbook-fw/src/*.rs cli/Cargo.toml cli/src/*.rs
git commit -m "refactor(binbook): isolate streaming decompression"
```

## Task 5: Extract pure GRAY2 rendering

**Files:**

- Create: `crates/gray2-render/Cargo.toml`
- Create: `crates/gray2-render/src/{lib,canonical,staged,dither,row}.rs`
- Create: `crates/gray2-render/tests/{canonical,staged,dither,golden}.rs`
- Modify initially: `firmware/crates/binbook-fw/src/display.rs`

- [ ] Write failing table tests for all four canonical values:

```text
0 black      -> red active, black active
1 dark gray  -> red active, black inactive
2 light gray -> red inactive, black active
3 white      -> red inactive, black inactive
```

- [ ] Add exhaustive byte-domain tests over all 256 packed GRAY2 byte values. Verify pure conversion against the Python writer’s `gray2_packed_to_x4_native_planes` output for a deterministic asymmetric row fixture.
- [ ] Add staged-plane reconstruction tests using the format specification formulas and verify black/white remain stable while dark/light gray remain distinct.
- [ ] Add ordered-dither tests at adjacent coordinates and row-buffer-too-small tests.
- [ ] Run `cargo test -p gray2-render` and confirm failure because the new crate API is absent.
- [ ] Implement only pure conversion and row operations. Do not move display refreshes, BinBook access, SSD1677 commands, or X4 page state into this crate.
- [ ] Replace firmware-local conversion helpers with `gray2-render` calls and delete the duplicated helpers.
- [ ] Run:

```bash
cargo test -p gray2-render
cargo check -p gray2-render --no-default-features --target riscv32imc-unknown-none-elf
cargo clippy -p gray2-render --all-targets -- -D warnings
cargo test --workspace
uv run pytest -q tests/test_pixels.py tests/test_x4_native_planes.py
```

- [ ] Commit:

```bash
git add Cargo.toml Cargo.lock crates/gray2-render/Cargo.toml crates/gray2-render/src/*.rs crates/gray2-render/tests/*.rs firmware/crates/binbook-fw/Cargo.toml firmware/crates/binbook-fw/src/display.rs tests/test_pixels.py tests/test_x4_native_planes.py
git commit -m "refactor(display): extract pure gray2 rendering"
```

## Task 6: Rebuild the SSD1677 driver on embedded-hal 1.0

**Files:**

- Create: `crates/ssd1677-driver/Cargo.toml`
- Create: `crates/ssd1677-driver/src/{lib,commands,config,driver,error,refresh,wait}.rs`
- Create: `crates/ssd1677-driver/tests/{commands,windows,refresh,async_wait}.rs`
- Remove after migration: `firmware/crates/ssd1677-driver/`
- Modify: firmware board adapters and mocks

- [ ] Move the existing command-sequence assertions into external black-box tests before changing production code. Tests must cover reset timing, BW init, grayscale init, full/partial/grayscale refreshes, staged activation, window/counter endianness, row writes, BUSY timeout, and controller powered/powered-down state.
- [ ] Add failing compile/behavior tests using embedded-hal mock implementations. The public driver must accept `SpiDevice<u8>`, output pins, input pin, sync delay, and async delay without importing `xteink-hal`.
- [ ] Run `cargo test -p ssd1677-driver` and confirm the new interface fails before implementation.
- [ ] Implement shared command helpers once. Synchronous and asynchronous paths may differ only in wait strategy; they must not duplicate command sequences.
- [ ] Move X4-specific staged LUT bytes and voltage policy out of the generic driver and expose controller operations needed for `xteink-x4-display` to apply a panel configuration.
- [ ] Replace firmware wrappers with embedded-hal implementations. Use `SpiDevice` so CS ownership remains correct on the SPI bus shared with SD.
- [ ] Run:

```bash
cargo test -p ssd1677-driver
cargo check -p ssd1677-driver --no-default-features --target riscv32imc-unknown-none-elf
cargo clippy -p ssd1677-driver --all-targets -- -D warnings
cargo test --workspace
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
```

- [ ] Commit the host-verified driver migration before touching hardware:

```bash
git add Cargo.toml Cargo.lock crates/ssd1677-driver/Cargo.toml crates/ssd1677-driver/src/*.rs crates/ssd1677-driver/tests/*.rs firmware/crates/binbook-fw/Cargo.toml firmware/crates/binbook-fw/src/*.rs firmware/crates/binbook-fw/tests/*.rs
git add -u firmware/crates/ssd1677-driver
git commit -m "refactor(display): adopt embedded-hal ssd1677 driver"
```

## Task 7: Driver hardware gate with serial and webcam evidence

**Evidence directory:** `/tmp/binbook-rust-refactor-driver`

- [ ] Create the evidence directory and flash sequentially:

```bash
mkdir -p /tmp/binbook-rust-refactor-driver
FW_FEATURES="firmware-bin,diagnostic-console" firmware/scripts/flash-xteink-x4-nav-probe.sh |& tee /tmp/binbook-rust-refactor-driver/flash.txt
```

Expected: ESP32-C3 revision, 16 MB flash, application size, and `Flashing has completed!` are recorded.

- [ ] Capture boot serial for at least 15 seconds with no other port owner:

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
" |& tee /tmp/binbook-rust-refactor-driver/boot.txt
```

- [ ] Query independent baselines:

```bash
cd cli
cargo run --features serial-device -- diag hello --port /dev/ttyACM0 | tee /tmp/binbook-rust-refactor-driver/hello.txt
cargo run --features serial-device -- diag status --port /dev/ttyACM0 | tee /tmp/binbook-rust-refactor-driver/status-before.txt
cd ..
```

Expected: protocol 1, frame limit 512, target `xteink-x4`, page count 16, zero protocol errors, and `last_error=0`.

- [ ] Run each probe separately, then take a fresh native-resolution webcam capture:

```bash
cd cli && cargo run --features serial-device -- diag probe --port /dev/ttyACM0 window-corners && cd ..
ffmpeg -hide_banner -loglevel error -f video4linux2 -i /dev/video1 -frames:v 1 /tmp/binbook-rust-refactor-driver/window-corners.jpg
cd cli && cargo run --features serial-device -- diag probe --port /dev/ttyACM0 clear-white && cd ..
ffmpeg -hide_banner -loglevel error -f video4linux2 -i /dev/video1 -frames:v 1 /tmp/binbook-rust-refactor-driver/clear-white.jpg
cd cli && cargo run --features serial-device -- diag probe --port /dev/ttyACM0 full-refresh-current && cd ..
ffmpeg -hide_banner -loglevel error -f video4linux2 -i /dev/video1 -frames:v 1 /tmp/binbook-rust-refactor-driver/full-refresh.jpg
```

- [ ] Inspect all three actual webcam files, using crop `crop=440:770:770:250` when needed. Verify four physical corner rectangles, an entirely white active panel, and restoration of the labeled orientation/calibration page with distinct grayscale swatches. Do not inspect a decoded fixture as a substitute.
- [ ] Query STATUS and logs independently after the probes and save them under the evidence directory.
- [ ] Immediately update `HANDOFF.md` with commands, output summaries, image paths, observed panel content, starting/ending state, and a driver-gate acceptance table. If any visible criterion fails, stop the refactor and fix the driver before continuing.

## Task 8: Extract the reusable X4 display pipeline

**Files:**

- Create: `crates/xteink-x4-display/Cargo.toml`
- Create focused modules under `crates/xteink-x4-display/src/`: `lib.rs`, `profile.rs`, `buffers.rs`, `page_source.rs`, `stream.rs`, `panel.rs`, `refresh.rs`, `engine.rs`, `events.rs`, `probes.rs`, `error.rs`
- Create tests under `crates/xteink-x4-display/tests/`: `validation.rs`, `streaming.rs`, `refresh.rs`, `engine.rs`, `cancellation.rs`, `probes.rs`
- Migrate behavior from: `firmware/crates/binbook-fw/src/display.rs`, `async_refresh.rs`, `refresh.rs`, and `runtime_engine.rs`

- [ ] Write failing tests first for every durable behavior currently protected by firmware tests:
  - X4 profile dimensions, 270-degree logical-to-physical rotation, waveform hint, three-plane bitmap, and 16-row chunk geometry;
  - caller buffers smaller than a compressed or decoded chunk;
  - absolute grayscale, full BW seed, full-screen differential BW, and explicitly gated dirty-chunk refresh;
  - staged overlay cancellation at strip boundaries;
  - background base-sync cancellation;
  - controller state after full, partial, staged, cancellation, and failure paths;
  - FIFO-relative page turns and boundary no-op completion;
  - recovery once, fault after repeated failure;
  - clear-white, corner-window, and full-refresh-current probes.
- [ ] Run `cargo test -p xteink-x4-display` and confirm failure because the crate is not implemented.
- [ ] Move the refresh coordinator and engine without changing event order or page-commit timing. Keep diagnostic event-number mapping in firmware; expose semantic `DisplayEvent` values from the crate.
- [ ] Implement chunk reads through `binbook_core::Book<R>` where `R: ReadAt` and decode through `binbook-decompress`. Eliminate every direct `book_bytes[start..end]` access and every fixed `BinBook<&[u8], &mut [u8; 8192]>` signature.
- [ ] Use `gray2-render` for plane reconstruction and `ssd1677-driver` for controller I/O. The X4 crate owns LUT/profile policy and refresh decisions.
- [ ] Delete migrated production helpers from firmware only after the new crate tests pass.
- [ ] Split tests by behavior. Replace `include_str!("../src/*.rs")` assertions with API, feature, build, or behavior checks. Preserve feature contracts through Cargo metadata or compile tests, not substring searches.
- [ ] Run:

```bash
cargo test -p xteink-x4-display
cargo check -p xteink-x4-display --no-default-features --target riscv32imc-unknown-none-elf
cargo clippy -p xteink-x4-display --all-targets -- -D warnings
cargo test --workspace
```

- [ ] Commit:

```bash
git add Cargo.toml Cargo.lock crates/xteink-x4-display/Cargo.toml crates/xteink-x4-display/src/*.rs crates/xteink-x4-display/tests/*.rs firmware/crates/binbook-fw/Cargo.toml firmware/crates/binbook-fw/src/*.rs firmware/crates/binbook-fw/tests/*.rs
git commit -m "refactor(display): extract xteink x4 pipeline"
```

## Task 9: Reduce firmware to platform wiring and remove `xteink-hal`

**Files:**

- Modify/split: `firmware/crates/binbook-fw/src/main.rs`, `runtime.rs`, `input.rs`, `flash.rs`, diagnostic modules
- Create focused firmware modules as needed: `board.rs`, `tasks.rs`, `display_adapter.rs`, `storage_adapter.rs`
- Remove: `firmware/crates/xteink-hal/`
- Remove migrated firmware display/refresh modules
- Split oversized firmware tests by subsystem without changing behavior

- [ ] Write failing tests that instantiate firmware adapters against the new library traits and verify diagnostic event mapping, request/completion sequencing, queue capacity, input mapping, flash boundaries, and feature gates.
- [ ] Run default and diagnostic firmware tests to capture the red failures caused by removing old modules/traits.
- [ ] Implement ESP/Embassy adapters only. `main.rs` must initialize hardware and start tasks; it must not parse BinBook records, decode PackBits, transform pixels, choose SSD1677 commands, or own refresh state.
- [ ] Replace custom flash traits with embedded-storage where supported. Keep ADC sampling behind a firmware-local seam because embedded-hal 1.0 has no general ADC trait.
- [ ] Remove `xteink-hal` and all imports. Verify with:

```bash
rg -n "xteink[_-]hal|BinBook<&\[u8\], &mut \[u8; 8192\]>|book_bytes\[|PackBitsStream" Cargo.toml crates firmware cli
```

Expected: no production references to `xteink-hal`, fixed 8 KiB BinBook scratch signatures, raw whole-book slicing, or firmware-local PackBits decoders.

- [ ] Run:

```bash
cargo test -p binbook-fw
cargo test -p binbook-fw --features diagnostic-console
cargo test --workspace
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
cd ..
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
cd ..
```

- [ ] Record default and diagnostic binary sizes and compare them with the checkpoint values in `HANDOFF.md`. Investigate unexpected growth before committing.
- [ ] Commit:

```bash
git add Cargo.toml Cargo.lock firmware/crates/binbook-fw/Cargo.toml firmware/crates/binbook-fw/src/*.rs firmware/crates/binbook-fw/tests/*.rs crates/*/Cargo.toml crates/*/src/*.rs crates/*/tests/*.rs
git add -u firmware/crates/xteink-hal firmware/crates/binbook-fw/src
git commit -m "refactor(firmware): isolate platform wiring"
```

## Task 10: Enforce workspace quality and independent compilation

**Files:** Root `Cargo.toml` and every changed crate manifest/source file.

- [ ] Add workspace lint configuration and opt changed crates into it. Reusable crates must deny warnings and unsafe code; firmware may contain narrowly documented unsafe blocks required by ESP startup.
- [ ] Remove all warnings observed in the initial baseline, including dead code and unused imports in changed tests. Do not silence warnings with broad `allow` attributes.
- [ ] Keep production modules focused. Split any newly created or substantially rewritten production file above 250 logical lines before proceeding.
- [ ] Run the full static matrix:

```bash
cargo fmt --all -- --check
cargo clippy -p binbook-core --all-targets -- -D warnings
cargo clippy -p binbook-decompress --all-targets --all-features -- -D warnings
cargo clippy -p gray2-render --all-targets -- -D warnings
cargo clippy -p ssd1677-driver --all-targets -- -D warnings
cargo clippy -p xteink-x4-display --all-targets -- -D warnings
cargo clippy -p binbook-fw --all-targets --features diagnostic-console -- -D warnings
cargo check -p binbook-core --no-default-features --target riscv32imc-unknown-none-elf
cargo check -p binbook-decompress --no-default-features --target riscv32imc-unknown-none-elf
cargo check -p gray2-render --no-default-features --target riscv32imc-unknown-none-elf
cargo check -p ssd1677-driver --no-default-features --target riscv32imc-unknown-none-elf
cargo check -p xteink-x4-display --no-default-features --target riscv32imc-unknown-none-elf
```

- [ ] Inspect `cargo metadata --no-deps --format-version 1` and `cargo tree` for each reusable crate. Confirm every forbidden dependency edge is absent.
- [ ] Commit:

```bash
git add Cargo.toml Cargo.lock crates/*/Cargo.toml crates/*/src/*.rs crates/*/tests/*.rs firmware/crates/binbook-fw/Cargo.toml firmware/crates/binbook-fw/src/*.rs firmware/crates/binbook-fw/tests/*.rs
git commit -m "chore(rust): enforce reusable crate boundaries"
```

## Task 11: Final automated regression matrix

- [ ] Start from a clean build baseline:

```bash
cargo clean
cargo test --workspace
cargo test -p binbook-fw --features diagnostic-console
cargo test -p binbook-cli --features serial-device
uv run pytest -q
uv run pytest -q tests/test_kerning_proof.py --run-proof
git diff --check
```

Expected: every Rust and Python suite passes. The proof suite must be run explicitly because its default tests are skipped.

- [ ] Build both pinned firmware variants:

```bash
cd firmware
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
cd ..
```

- [ ] Verify CLI surface behavior with `cargo run -p binbook-cli -- --help` and the Python encode/inspect/decode round trip documented in README. Confirm generated fixture bytes still validate against `BINBOOK_FORMAT_SPEC.md`.
- [ ] Do not continue to final hardware verification with a failing test, warning-as-error failure, format-byte difference, or unexplained binary-size regression.

## Task 12: Final X4 serial, navigation, staged-gray, and webcam gate

**Evidence directory:** `/tmp/binbook-rust-refactor-final`

- [ ] Flash the normal diagnostic image and save output:

```bash
mkdir -p /tmp/binbook-rust-refactor-final
FW_FEATURES="firmware-bin,diagnostic-console" firmware/scripts/flash-xteink-x4-nav-probe.sh |& tee /tmp/binbook-rust-refactor-final/flash.txt
```

- [ ] Capture the required 15-second boot record using the exact pyserial command from Task 7, writing `/tmp/binbook-rust-refactor-final/boot.txt`.
- [ ] Establish independent HELLO and STATUS baselines and save both outputs. Require protocol 1, frame limit 512, page count 16, zero dropped logs, zero protocol errors, and `last_error=0`.
- [ ] Run the synchronized navigation diagnostic:

```bash
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline python firmware/scripts/run-x4-nav-burst-diagnostic.py --port /dev/ttyACM0 --video-device /dev/video1 --rounds 10 --inter-key-ms 0 --output-dir /tmp/binbook-rust-refactor-final/nav-burst
```

Required evidence: 165 accepted KEY records, 165 unique sequence-matched `TURN_DEQUEUED` completions, modeled boundary results `0,1,0,0,1`, no protocol/display errors, final STATUS agreement, MP4, contact sheets, and a settled webcam frame.

- [ ] Run the staged-gray exercise with the serial port exclusively owned:

```bash
cd cli
cargo run --features serial-device -- diag exercise staged-gray --port /dev/ttyACM0 | tee /tmp/binbook-rust-refactor-final/staged-gray.txt
cd ..
```

- [ ] During and after the exercise, capture fresh webcam frames from `/dev/video1`. Verify the fast BW base, delayed dark/light gray separation, cancellation behavior, absence of full-panel flashes or corruption, FIFO result `2,3,2`, and final stable page 3.
- [ ] Query STATUS and paginate logs independently after the exercise. Save outputs under `/tmp/binbook-rust-refactor-final/`; require page 3, `last_error=0`, zero dropped turns, waveform hint 2, LUT revision 1, and ordered overlay/base-sync/controller-state events.
- [ ] Run the three ignored live transport tests sequentially, followed by STATUS, exactly as specified in `docs/reference/xteink-x4-agent-device-verification.md`.
- [ ] Capture a final native webcam image:

```bash
ffmpeg -hide_banner -loglevel error -f video4linux2 -i /dev/video1 -frames:v 1 /tmp/binbook-rust-refactor-final/final-settled.jpg
```

- [ ] Inspect the actual capture. Require the page label, TL/TR/BL/BR markers, edge labels, rulers, asymmetric marker, border, grid, and four calibration swatches to be correctly oriented, unclipped, and visually distinct.
- [ ] Update `HANDOFF.md` immediately with exact commands, relevant full outputs, artifact paths, start/end state, observed webcam content, known failures, and the acceptance matrix. Any blank hardware-evidence cell keeps the refactor incomplete.

## Task 13: Documentation, roadmap, and stale-reference cleanup

**Files:**

- Modify: `README.md`, `AGENTS.md`, `docs/README.md`, Rust architecture/reference docs, flashing/device-verification docs, `HANDOFF.md`, `docs/ROADMAP.md`

- [ ] Update all build commands for the root workspace and root `target/` directory.
- [ ] Document how an external Rust consumer imports each reusable crate, which features are available, which buffers it must provide, and which crate owns X4 display policy.
- [ ] Add these concrete roadmap sections:

```markdown
## Python authoring package modularization

Split binary format models/writing, EPUB ingestion, raster rendering, viewer, and kerning-proof server into independently testable modules. Define an explicit public package API, add basedpyright strict checking and Ruff gates, and preserve existing CLI output and BinBook bytes.

## Rust CLI and diagnostic protocol modularization

Split command models, serial transport, response formatting, exercise evidence, and protocol codecs into focused modules. Replace oversized test files and source-shape checks with public behavior and wire-format tests without changing protocol version 1.

## SquidScript Rust-native BinBook/display adoption

After SquidScript chooses its post-Zephyr firmware architecture, consume `binbook-core`, `binbook-decompress`, `gray2-render`, `ssd1677-driver`, and `xteink-x4-display` directly. Do not add a C ABI or compatibility facade before that architecture is selected.
```

- [ ] Search for stale crate names, paths, commands, and removed APIs:

```bash
rg -n "binbook/rust|firmware/target|firmware/crates/ssd1677-driver|xteink[_-]hal|PageRef|decompress_page\(|\[u8; 8192\]" . --glob '!target/**' --glob '!.git/**'
```

Expected: no stale production or documentation references; historical documents may retain old facts only when clearly marked historical.

- [ ] Run final docs and repository checks:

```bash
cargo test --workspace
uv run pytest -q
git diff --check
git status --short
```

- [ ] Commit:

```bash
git add README.md AGENTS.md HANDOFF.md docs/README.md docs/ROADMAP.md docs/reference/rust-crate-architecture.md docs/reference/xteink-x4-agent-device-verification.md docs/reference/xteink-x4-firmware-flashing.md docs/specs/2026-06-30-rust-modular-foundation-design.md Cargo.toml Cargo.lock
git commit -m "docs: document modular Rust foundation"
```

---

## Final acceptance matrix

The executor must replace this section in `HANDOFF.md` with observed evidence, not intentions.

| Requirement | Implementation proof | Automated proof | Serial proof | Webcam proof |
|---|---|---|---|---|
| Five independent reusable crates | Cargo metadata and dependency graph | Per-crate tests, Clippy, no_std target checks | Not applicable | Not applicable |
| No firmware-local parsing/decompression/plane/controller logic | Firmware imports and deleted old modules | Workspace tests and forbidden-pattern scan | HELLO/STATUS still operate | Final page renders correctly |
| BinBook bytes and parsing remain compatible | Python fixture plus core parser | Python round trip and core golden tests | Fixture opens with 16 pages | Navigation pages display |
| Canonical and staged grayscale remain correct | `gray2-render` and X4 pipeline | Exhaustive byte and staged-plane tests | Ordered staged-gray events | Four swatches visibly distinct |
| SSD1677 command behavior remains correct | embedded-hal driver | Exact command-sequence tests | Probe/render success events | Corners, white clear, restored page |
| Cancellation and FIFO semantics remain correct | X4 display engine | Engine/cancellation/boundary tests | 165 sequence-matched completions | Settled pages match model |
| RAM discipline is preserved | Caller-owned chunk buffers | Buffer-boundary tests and target builds | No display/last error | No clipping or incomplete writes |
| Firmware behavior is complete | Thin board/runtime adapters | Default and diagnostic test matrices | Independent STATUS/log queries | Fresh final settled capture |

## Completion rule

The refactor is complete only when every task is checked, every commit boundary is green, both hardware gates have current serial and webcam evidence, the final acceptance matrix has no missing cells, and an adversarial read of source plus live state cannot disprove any completion claim.
