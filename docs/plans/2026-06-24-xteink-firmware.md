# BinBook Xteink X4 Firmware — Continuation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task. Do not dispatch subagents unless the user explicitly asks for subagents or parallel agent work. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a bare-metal `no_std` Rust firmware foundation for the Xteink X4 that can display GRAY1 BinBook pages with low button-to-display latency, plus a Rust host CLI for device management.

**Architecture:** The firmware workspace is split into reusable crates under `firmware/crates/`: `xteink-hal` for hardware traits, `ssd1677-driver` for the e-ink display command layer, and `binbook-fw` for board/app logic. Host-testable logic is implemented in library modules; the bare-metal binary is gated behind the `firmware-bin` feature so normal host tests do not try to link a `no_std` entry point. The display path must stream decompressed GRAY1 rows directly to SPI without a framebuffer.

**Tech Stack:** Rust nightly for target firmware builds, host Rust tests for reusable logic, existing `rust/` BinBook crate, ESP32-C3 target `riscv32imc-unknown-none-elf`, SSD1677/GDEQ0426T82 e-ink controller.

**Spec:** [`docs/specs/2026-06-24-xteink-firmware-design.md`](../specs/2026-06-24-xteink-firmware-design.md)

**Reference:** [`docs/reference/squidscript-and-xteink-reference.md`](../reference/squidscript-and-xteink-reference.md)

**Flashing reference:** [`docs/reference/xteink-x4-firmware-flashing.md`](../reference/xteink-x4-firmware-flashing.md)

---

## Current State as of 2026-06-25

This plan replaces the stale first-pass implementation plan. The original goal remains valid, but several snippets in the prior plan were wrong or stale.

### Implemented and verified

- `firmware/Cargo.toml` workspace with members:
  - `crates/xteink-hal`
  - `crates/ssd1677-driver`
  - `crates/binbook-fw`
- `firmware/rust-toolchain.toml` pins nightly and `riscv32imc-unknown-none-elf`.
- `firmware/.cargo/config.toml` enables `build-std = ["core", "alloc"]` and passes the ESP32-C3 linker script for target builds.
- `firmware/crates/xteink-hal/src/lib.rs` defines reusable `no_std` HAL traits.
- `firmware/crates/ssd1677-driver/src/lib.rs` defines the SSD1677 command layer with tests for:
  - Xteink X4 full RAM window: X byte window `0..99`, Y row window `0..479`.
  - Pixel-window-to-RAM-byte-window conversion.
  - Little-endian Y counter writes.
- `firmware/crates/binbook-fw/src/` contains host-testable modules:
  - `input.rs`: ADC ladder button decoding and cooldown state.
  - `display.rs`: BinBook PackBits row decompression and display streaming skeleton.
  - `flash.rs`: basic raw flash file table lookup.
  - `serial.rs`: allocation-free borrowed serial command parser.
  - `main.rs`: ESP32-C3 `no_std` display smoke-test entry point, compiled only with `--features firmware-bin`.
- `firmware/crates/binbook-fw/tests/firmware_logic.rs` covers input, decompression, orientation mapping, display smoke pattern generation, flash lookup, and serial parsing.
- `cli/` contains a default-compiling `binbook-cli` skeleton. `serialport` is optional behind the `serial-device` feature because this host may not have `libudev.pc`.
- `docs/specs/2026-06-24-xteink-firmware-design.md` has status `In Progress`.
- `AGENTS.md` includes firmware build commands.
- `firmware/scripts/flash-xteink-x4-smoke.sh` builds and flashes the current smoke firmware with `espflash`.

### Verified commands

Run these from the repository root unless a command says otherwise.

```bash
cd firmware && cargo test --workspace
```

Expected current result: pass. Current test count is 13 firmware tests: 10 in `binbook-fw`, 3 in `ssd1677-driver`. Warnings from the existing `rust/` BinBook crate may appear.

```bash
cd firmware && cargo clippy --workspace --all-targets
```

Expected current result: exit 0. Existing `rust/` BinBook warnings may appear.

```bash
cd cli && cargo check
```

Expected current result: pass with default features. Do not enable `serial-device` unless the host has `libudev.pc`.

Use rustup's pinned nightly `cargo` and `rustc` for the target build. Do not rely on arbitrary tools from `PATH`; if `cargo` and `rustc` resolve from different toolchain managers, the build can fail with a missing `core` crate or reject nightly-only flags even when the target is installed.

```bash
cd firmware
RUSTC="$(rustup which --toolchain nightly rustc)" \
  rustup run nightly cargo build \
  -p binbook-fw \
  --features firmware-bin \
  --target riscv32imc-unknown-none-elf \
  --release
```

Expected current result: pass. Current smoke-test binary:

```bash
ls -lh firmware/target/riscv32imc-unknown-none-elf/release/binbook-fw
```

Expected output: `firmware/target/riscv32imc-unknown-none-elf/release/binbook-fw`. This binary initializes the SSD1677 display through the verified Xteink X4 pins and draws an asymmetric physical-framebuffer smoke pattern.

Flash the smoke firmware from the repository root with:

```bash
firmware/scripts/flash-xteink-x4-smoke.sh
```

The script uses `${HOME}/.cargo/bin/espflash` by default, flashes `/dev/ttyACM0` by default, sets `--chip esp32c3`, and sets the verified Xteink X4 flash size `--flash-size 16mb`. Override `ESPFLASH`, `ESPFLASH_PORT`, `CARGO`, `RUSTC`, or `IMAGE` only when the host layout differs.

Smoke-test behavior after flashing:

- One filled black box at each physical corner.
- No center vertical stripe.
- The firmware clears both SSD1677 RAM planes to white and full-refreshes before drawing the boxes. The pattern is intentionally physical-framebuffer oriented; it verifies SPI, GPIO, SSD1677 init, RAM writes, and full refresh before testing BinBook logical page orientation.

### Known verification blocker

The required Python suite is blocked by host dependencies, not by the firmware changes:

```bash
uv run pytest -q
```

Current blocker: `pygame==2.6.1` fails to build because the host lacks X11 headers (`X11/Xlib.h`). Running without sync:

```bash
UV_NO_SYNC=1 uv run pytest -q
```

Current observed result: `70 passed, 26 skipped, 1 failed`, where the one failure is `ModuleNotFoundError: No module named 'pygame'` in `tests/test_viewer.py::test_image_to_surface_preserves_dimensions`.

### Important corrections from the stale plan

- Plans belong under `docs/plans/`, not `docs/superpowers/plans/`.
- From `firmware/crates/binbook-fw`, the existing Rust BinBook crate path is `../../../rust`, not `../../rust`.
- Release profile settings belong in `firmware/Cargo.toml`, not in a non-root workspace package.
- Do not put `embedded-hal` in the initial reusable crates unless code actually uses it. The current `xteink-hal` traits are local and dependency-free.
- SSD1677 X byte window for 800 pixels is `0x00..0x63`, not `0x00..0x0C`.
- SSD1677 Y row window for 480 rows is `0x0000..0x01DF`, sent as `[0x00, 0x00, 0xDF, 0x01]`.
- SSD1677 Y counter bytes are sent low byte then high byte.
- `serialport` pulls `libudev-sys` on Linux; keep it optional until real serial transport is implemented or host dependencies are documented.
- Host tests must not build the `no_std` firmware binary. Keep `[[bin]].required-features = ["firmware-bin"]`.
- Xteink X4 logical-to-physical rotation is `270` degrees clockwise, matching the verified SquidScript target metadata. Older docs and first-pass plans that said `90` predated hardware verification.

---

## File Structure

Current intended structure:

```text
binbook/
├── rust/
│   └── Cargo.toml                         # existing BinBook parser crate, name = "binbook"
├── firmware/
│   ├── .cargo/config.toml                  # nightly build-std settings
│   ├── Cargo.toml                          # firmware workspace root
│   ├── rust-toolchain.toml                  # nightly + riscv32imc target
│   └── crates/
│       ├── xteink-hal/
│       │   ├── Cargo.toml
│       │   └── src/lib.rs                  # reusable hardware traits
│       ├── ssd1677-driver/
│       │   ├── Cargo.toml
│       │   └── src/lib.rs                  # reusable SSD1677 command layer
│       └── binbook-fw/
│           ├── Cargo.toml
│           ├── src/
│           │   ├── lib.rs                  # host-testable firmware modules
│           │   ├── main.rs                 # ESP32-C3 display smoke-test entry point
│           │   ├── input.rs                # button decoding and debounce state
│           │   ├── display.rs              # GRAY1 row decompression/display pipeline
│           │   ├── serial.rs               # serial command parser
│           │   └── flash.rs                # raw flash file table lookup
│           └── tests/firmware_logic.rs     # host integration tests
└── cli/
    ├── Cargo.toml
    └── src/main.rs                         # host CLI skeleton
```

Future hardware-specific modules may be added under `xteink-hal` or `binbook-fw`, but keep board-specific pin mappings out of reusable display/parser crates.

---

## Task 1: Replace Placeholder Display Pipeline With Page-Oriented Streaming Tests

**Goal:** Make `binbook-fw::display` prove that a compressed GRAY1 page stream is decompressed row-by-row and written to the display without a framebuffer.

**Files:**
- Modify: `firmware/crates/binbook-fw/src/display.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Write a failing test for multi-row streaming**

Add this test to `firmware/crates/binbook-fw/tests/firmware_logic.rs`. It should use a fake display sink, not the real SSD1677 driver, so the test only verifies firmware display pipeline behavior.

```rust
#[test]
fn streams_two_decompressed_rows_to_sink_without_framebuffer() {
    use binbook_fw::display::{stream_gray1_rows, GRAY1_ROW_BYTES};

    let mut input = Vec::new();
    input.extend_from_slice(&[0xBF, 0xFF]); // repeat 64, enough to fill first 60-byte row
    input.extend_from_slice(&[0xBF, 0x00]); // repeat 64, enough to fill second 60-byte row

    let mut rows = Vec::new();
    stream_gray1_rows(&input, 2, |row_index, row| {
        rows.push((row_index, row.to_vec()));
        Ok(())
    })
    .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, 0);
    assert_eq!(rows[0].1, vec![0xFF; GRAY1_ROW_BYTES]);
    assert_eq!(rows[1].0, 1);
    assert_eq!(rows[1].1, vec![0x00; GRAY1_ROW_BYTES]);
}
```

- [ ] **Step 2: Run the test and verify it fails**

```bash
cd firmware
cargo test -p binbook-fw --test firmware_logic streams_two_decompressed_rows_to_sink_without_framebuffer
```

Expected: fail because `stream_gray1_rows` does not exist.

- [ ] **Step 3: Implement minimal streaming helper**

Add this to `firmware/crates/binbook-fw/src/display.rs`:

```rust
pub fn stream_gray1_rows<E>(
    compressed_data: &[u8],
    row_count: u16,
    mut write_row: impl FnMut(u16, &[u8]) -> Result<(), E>,
) -> Result<(), E> {
    let mut row_buf = [0u8; GRAY1_ROW_BYTES];
    let mut input_pos = 0;

    for row in 0..row_count {
        input_pos += decompress_row(&compressed_data[input_pos..], &mut row_buf);
        write_row(row, &row_buf)?;
    }

    Ok(())
}
```

Then change `display_page()` to call `stream_gray1_rows()` and forward each row to `display.write_row(row, row_buf)`.

- [ ] **Step 4: Run tests**

```bash
cd firmware
cargo test -p binbook-fw --test firmware_logic
```

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add firmware/crates/binbook-fw/src/display.rs firmware/crates/binbook-fw/tests/firmware_logic.rs
git commit -m "feat(binbook-fw): stream GRAY1 rows through display pipeline"
```

---

## Task 2: Add Rotation Mapping Tests for Portrait GRAY1 Pages

**Goal:** Define and test logical portrait-to-physical landscape coordinate mapping before touching page rendering code.

**Files:**
- Modify: `firmware/crates/binbook-fw/src/display.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Write failing tests for coordinate mapping**

Add this test:

```rust
#[test]
fn maps_logical_portrait_coordinates_to_physical_landscape() {
    use binbook_fw::display::logical_to_physical;

    assert_eq!(logical_to_physical(0, 0), (799, 0));
    assert_eq!(logical_to_physical(479, 0), (799, 479));
    assert_eq!(logical_to_physical(0, 799), (0, 0));
    assert_eq!(logical_to_physical(479, 799), (0, 479));
    assert_eq!(logical_to_physical(123, 456), (343, 123));
}
```

- [ ] **Step 2: Run the test and verify it fails**

```bash
cd firmware
cargo test -p binbook-fw --test firmware_logic maps_logical_portrait_coordinates_to_physical_landscape
```

Expected: fail because `logical_to_physical` does not exist.

- [ ] **Step 3: Implement minimal mapping**

Add to `firmware/crates/binbook-fw/src/display.rs`:

```rust
pub const PAGE_WIDTH: u16 = 480;
pub const PAGE_HEIGHT: u16 = 800;
pub const DISPLAY_WIDTH: u16 = 800;
pub const DISPLAY_HEIGHT: u16 = 480;

pub fn logical_to_physical(logical_x: u16, logical_y: u16) -> (u16, u16) {
    (PAGE_HEIGHT - 1 - logical_y, logical_x)
}
```

If constants already exist, keep one definition and update only the function.

- [ ] **Step 4: Run tests**

```bash
cd firmware
cargo test -p binbook-fw --test firmware_logic
```

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add firmware/crates/binbook-fw/src/display.rs firmware/crates/binbook-fw/tests/firmware_logic.rs
git commit -m "feat(binbook-fw): add portrait-to-physical coordinate mapping"
```

---

## Task 3: Add Flash Read Window Abstraction for Stored BinBooks

**Goal:** Provide a small, testable abstraction for reading file bytes from the raw flash table so later BinBook parsing can consume a selected file without copying it into RAM.

**Files:**
- Modify: `firmware/crates/binbook-fw/src/flash.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Write failing tests for bounded file reads**

Add this test:

```rust
#[test]
fn reads_file_bytes_relative_to_flash_file_offset() {
    let mut flash = MockFlash::new();
    flash.write_entry(0, "sample.binbook", 64, 4);
    flash.write(64, &[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();

    let mut storage = FlashStorage::new(flash);
    let info = storage.find("sample.binbook").unwrap().unwrap();

    let mut out = [0u8; 2];
    storage.read_file(&info, 1, &mut out).unwrap();

    assert_eq!(out, [0xAD, 0xBE]);
}
```

- [ ] **Step 2: Run the test and verify it fails**

```bash
cd firmware
cargo test -p binbook-fw --test firmware_logic reads_file_bytes_relative_to_flash_file_offset
```

Expected: fail because `read_file` does not exist.

- [ ] **Step 3: Implement bounded read**

Add this method to `impl<F: Flash> FlashStorage<F>` in `firmware/crates/binbook-fw/src/flash.rs`:

```rust
pub fn read_file(&self, info: &FileInfo, offset: u32, buf: &mut [u8]) -> HalResult<()> {
    let end = offset.saturating_add(buf.len() as u32);
    if end > info.size {
        return Err(xteink_hal::HalError::InvalidParam);
    }

    self.flash.read(info.offset + offset, buf)
}
```

If the current `Flash` trait requires `&self` for `read`, keep `read_file(&self, ...)`. If it changes to require `&mut self`, update both the trait and call sites consistently.

- [ ] **Step 4: Run tests**

```bash
cd firmware
cargo test -p binbook-fw --test firmware_logic
```

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add firmware/crates/binbook-fw/src/flash.rs firmware/crates/binbook-fw/tests/firmware_logic.rs
git commit -m "feat(binbook-fw): read file windows from raw flash storage"
```

---

## Task 4: Define a Minimal BinBook Page Source Interface

**Goal:** Bridge stored file bytes toward rendering without tying `binbook-fw` to a concrete flash implementation or buffering full pages.

**Files:**
- Create: `firmware/crates/binbook-fw/src/book.rs`
- Modify: `firmware/crates/binbook-fw/src/lib.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Write failing tests for a borrowed page source**

Add this test:

```rust
#[test]
fn page_source_reads_exact_compressed_page_slice() {
    use binbook_fw::book::{PageExtent, PageSource};

    struct Bytes<'a>(&'a [u8]);

    impl PageSource for Bytes<'_> {
        type Error = ();

        fn read_at(&self, offset: u32, out: &mut [u8]) -> Result<(), Self::Error> {
            let offset = offset as usize;
            out.copy_from_slice(&self.0[offset..offset + out.len()]);
            Ok(())
        }
    }

    let source = Bytes(&[0, 1, 2, 3, 4, 5, 6, 7]);
    let extent = PageExtent { offset: 2, size: 3 };
    let mut out = [0u8; 3];

    source.read_page(&extent, &mut out).unwrap();

    assert_eq!(out, [2, 3, 4]);
}
```

- [ ] **Step 2: Run the test and verify it fails**

```bash
cd firmware
cargo test -p binbook-fw --test firmware_logic page_source_reads_exact_compressed_page_slice
```

Expected: fail because `book` does not exist.

- [ ] **Step 3: Implement the interface**

Create `firmware/crates/binbook-fw/src/book.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageExtent {
    pub offset: u32,
    pub size: u32,
}

pub trait PageSource {
    type Error;

    fn read_at(&self, offset: u32, out: &mut [u8]) -> Result<(), Self::Error>;

    fn read_page(&self, extent: &PageExtent, out: &mut [u8]) -> Result<(), Self::Error> {
        self.read_at(extent.offset, out)
    }
}
```

Modify `firmware/crates/binbook-fw/src/lib.rs`:

```rust
pub mod book;
pub mod display;
pub mod flash;
pub mod input;
pub mod serial;
```

- [ ] **Step 4: Run tests**

```bash
cd firmware
cargo test -p binbook-fw --test firmware_logic
```

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add firmware/crates/binbook-fw/src/book.rs firmware/crates/binbook-fw/src/lib.rs firmware/crates/binbook-fw/tests/firmware_logic.rs
git commit -m "feat(binbook-fw): define page source interface"
```

---

## Task 5: Implement Serial RX State Machine

**Goal:** Turn the current stateless parser into a bounded RX state machine that accepts byte chunks and emits complete commands on newline.

**Files:**
- Modify: `firmware/crates/binbook-fw/src/serial.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Write failing test for chunked serial input**

Add this test:

```rust
#[test]
fn serial_state_parses_command_split_across_chunks() {
    use binbook_fw::serial::{Command, SerialState};

    let mut state = SerialState::<64>::new();

    assert_eq!(state.feed(b"UPLO"), None);
    assert_eq!(state.feed(b"AD sample.binbook 4\n"), Some(Command::Upload {
        name: "sample.binbook",
        size: 4,
    }));
}
```

- [ ] **Step 2: Run the test and verify it fails**

```bash
cd firmware
cargo test -p binbook-fw --test firmware_logic serial_state_parses_command_split_across_chunks
```

Expected: fail because `SerialState` does not exist.

- [ ] **Step 3: Implement bounded state**

Add to `firmware/crates/binbook-fw/src/serial.rs`:

```rust
pub struct SerialState<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> SerialState<N> {
    pub const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Option<Command<'_>> {
        for &byte in bytes {
            if byte == b'\n' {
                let line = core::str::from_utf8(&self.buf[..self.len]).ok()?;
                let command = parse_command(line);
                self.len = 0;
                return Some(command);
            }

            if self.len == N {
                self.len = 0;
                return Some(Command::Unknown);
            }

            self.buf[self.len] = byte;
            self.len += 1;
        }

        None
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cd firmware
cargo test -p binbook-fw --test firmware_logic
```

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add firmware/crates/binbook-fw/src/serial.rs firmware/crates/binbook-fw/tests/firmware_logic.rs
git commit -m "feat(binbook-fw): parse serial commands from bounded RX state"
```

---

## Task 6: Add CLI Protocol Formatting Tests

**Goal:** Make the host CLI generate the same serial commands that firmware parses, before implementing real serial I/O.

**Files:**
- Create: `cli/src/lib.rs`
- Modify: `cli/src/main.rs`
- Create: `cli/tests/protocol.rs`

- [ ] **Step 1: Write failing CLI protocol tests**

Create `cli/tests/protocol.rs`:

```rust
use binbook_cli::protocol::{delete_command, list_command, upload_command};

#[test]
fn formats_serial_protocol_commands() {
    assert_eq!(list_command(), "LIST\n");
    assert_eq!(delete_command("sample.binbook"), "DELETE sample.binbook\n");
    assert_eq!(
        upload_command("sample.binbook", 12345),
        "UPLOAD sample.binbook 12345\n"
    );
}
```

- [ ] **Step 2: Run the test and verify it fails**

```bash
cd cli
cargo test --test protocol
```

Expected: fail because `binbook_cli::protocol` does not exist.

- [ ] **Step 3: Implement protocol formatting**

Create `cli/src/lib.rs`:

```rust
pub mod protocol {
    pub fn list_command() -> String {
        "LIST\n".to_owned()
    }

    pub fn delete_command(name: &str) -> String {
        format!("DELETE {name}\n")
    }

    pub fn upload_command(name: &str, size: u64) -> String {
        format!("UPLOAD {name} {size}\n")
    }
}
```

`cli/src/main.rs` can remain a skeleton until real serial I/O is added.

- [ ] **Step 4: Run tests**

```bash
cd cli
cargo test --test protocol
cargo check
```

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add cli/src/lib.rs cli/src/main.rs cli/tests/protocol.rs cli/Cargo.toml cli/Cargo.lock
git commit -m "feat(cli): add serial protocol formatting helpers"
```

---

## Task 7: Document Host Dependency Requirements

**Goal:** Make verification reproducible by documenting why Python and serial-device checks may fail on hosts without GUI/udev development headers.

**Files:**
- Modify: `AGENTS.md`
- Modify: `docs/specs/2026-06-24-xteink-firmware-design.md`

- [ ] **Step 1: Add host dependency notes**

In `AGENTS.md`, under “Firmware Build Commands”, add:

```markdown
- Build CLI with serial backend: `cd cli && cargo check --features serial-device`
  - Linux requires `libudev.pc` available through `pkg-config`.
- Full Python tests require pygame; if pygame must build from source, the host needs SDL2/X11 development headers.
```

In the spec, add a short “Host Build Dependencies” subsection under implementation notes:

```markdown
### Host Build Dependencies

Default firmware and CLI checks avoid host serial backends. The `cli` crate keeps `serialport` behind the `serial-device` feature because Linux builds require `libudev.pc`.

The Python viewer tests depend on pygame. On hosts without prebuilt pygame wheels, building pygame requires SDL2 and X11 development headers.
```

- [ ] **Step 2: Verify docs references**

```bash
rg 'serial-device|libudev|pygame|X11' AGENTS.md docs/specs/2026-06-24-xteink-firmware-design.md
```

Expected: each term appears in the newly added docs.

- [ ] **Step 3: Commit**

```bash
git add AGENTS.md docs/specs/2026-06-24-xteink-firmware-design.md
git commit -m "docs: document firmware host build dependencies"
```

---

## Task 8: Final Verification and Handoff

**Goal:** Re-run the checks that prove the current firmware foundation is usable, and record any host blockers honestly.

- [ ] **Step 1: Run firmware tests**

```bash
cd firmware
cargo test --workspace
```

Expected: pass.

- [ ] **Step 2: Run firmware clippy**

```bash
cd firmware
cargo clippy --workspace --all-targets
```

Expected: exit 0. Existing warnings from the shared `rust/` BinBook crate may appear; do not claim warning-free unless they are fixed.

- [ ] **Step 3: Build target firmware binary**

Prefer this command on the current host:

```bash
cd firmware
RUSTC="$(rustup which --toolchain nightly rustc)" \
  rustup run nightly cargo build \
  -p binbook-fw \
  --features firmware-bin \
  --target riscv32imc-unknown-none-elf \
  --release
```

Expected: pass.

- [ ] **Step 4: Check target binary size**

```bash
ls -lh firmware/target/riscv32imc-unknown-none-elf/release/binbook-fw
```

Expected: under 100KB. It may remain very small until hardware wiring is implemented.

- [ ] **Step 5: Run CLI checks**

```bash
cd cli
cargo check
cargo test
```

Expected: pass with default features.

- [ ] **Step 6: Run Python tests or record host blocker**

```bash
uv run pytest -q
```

Expected on a fully provisioned host: pass. On the current host, this may fail because pygame cannot build without X11 headers. If it fails, also run:

```bash
UV_NO_SYNC=1 uv run pytest -q
```

Record the exact result in the handoff.

- [ ] **Step 7: Inspect worktree status**

```bash
git status --short --untracked-files=all
```

Expected: only intended files plus any pre-existing `.omo/run-continuation/*` files. Do not delete `.omo/run-continuation/*` unless the user explicitly asks.

- [ ] **Step 8: Commit or hand off**

If the user asks to commit, stage specific files only. Do not use `git add .` or `git add -A`.

Suggested staged paths for the current firmware continuation work:

```bash
git add .gitignore AGENTS.md \
  docs/plans/2026-06-24-xteink-firmware.md \
  docs/specs/2026-06-24-xteink-firmware-design.md \
  docs/reference/squidscript-and-xteink-reference.md \
  firmware/.cargo/config.toml firmware/Cargo.lock firmware/Cargo.toml firmware/rust-toolchain.toml \
  firmware/crates/xteink-hal/Cargo.toml firmware/crates/xteink-hal/src/lib.rs \
  firmware/crates/ssd1677-driver/Cargo.toml firmware/crates/ssd1677-driver/src/lib.rs \
  firmware/crates/binbook-fw/Cargo.toml firmware/crates/binbook-fw/src/lib.rs \
  firmware/crates/binbook-fw/src/main.rs firmware/crates/binbook-fw/src/input.rs \
  firmware/crates/binbook-fw/src/display.rs firmware/crates/binbook-fw/src/flash.rs \
  firmware/crates/binbook-fw/src/serial.rs firmware/crates/binbook-fw/tests/firmware_logic.rs \
  cli/Cargo.lock cli/Cargo.toml cli/src/main.rs
```

Suggested commit message:

```bash
git commit -m "feat: add xteink firmware workspace foundation"
```

---

## Acceptance Criteria for the Next Milestone

The next milestone is complete when:

- Firmware workspace tests pass.
- SSD1677 driver behavior has tests for command sequencing and addressing.
- `binbook-fw` has host tests for:
  - ADC button decoding.
  - PackBits row decompression.
  - row streaming without a framebuffer.
  - portrait-to-physical coordinate mapping.
  - flash file lookup/read windows.
  - serial command parsing.
- Target firmware binary builds for `riscv32imc-unknown-none-elf`.
- CLI default checks/tests pass without requiring `libudev.pc`.
- Any Python test blocker is documented with exact dependency/error output.

## Hardware Bring-Up Path

This is the intended path from the current scaffold to flashing and testing on the Xteink X4 hardware.

### Milestone A: Host-proven data path

Objective: prove the page data path without hardware.

- Encode a small GRAY1 `.binbook` with a distinctive orientation probe pattern: black corners, labeled/numbered edge bands if text rendering is available, or asymmetric blocks if not.
- Verify `DISPLAY_PROFILE.logical_to_physical_rotation = 270`.
- Decode the page and inspect the physical storage orientation. A logical black pixel at `(10, 20)` must appear at physical `(779, 10)`.
- Keep this as a regression fixture before firmware flashing.

### Milestone B: Hardware-safe display smoke firmware

Objective: flash the smallest firmware that proves the SPI, GPIO, reset, BUSY, and SSD1677 command path.

- Add real ESP32-C3 HAL implementations for SPI, output pins, BUSY input, and millisecond delay.
- Keep hardware pin constants in `xteink-hal` board modules or `binbook-fw`, not in `ssd1677-driver`.
- Firmware behavior: reset display, clear to white, draw a static asymmetric test pattern directly in physical coordinates, refresh, then idle.
- Do not add BinBook parsing yet. This isolates display wiring and SSD1677 command behavior.
- Verification: flashed device visibly shows the asymmetric pattern in the expected physical orientation and serial/debug output reports no BUSY timeout.

### Milestone C: Logical orientation probe firmware

Objective: prove the verified 270° logical-to-physical transform on hardware.

- Add a logical-coordinate pattern renderer using `logical_to_physical()`.
- Draw logical markers at `(0,0)`, `(479,0)`, `(0,799)`, and `(479,799)`.
- Expected physical positions for rotation 270:
  - logical `(0,0)` → physical `(799,0)`
  - logical `(479,0)` → physical `(799,479)`
  - logical `(0,799)` → physical `(0,0)`
  - logical `(479,799)` → physical `(0,479)`
- Verification: markers appear at the physical panel corners matching the mapping above. If they do not, stop and correct the transform before adding BinBook I/O.

### Milestone D: Flash-backed GRAY1 BinBook page display

Objective: display page 0 of a stored GRAY1 BinBook.

- Implement or adapt a flash reader that can read the raw storage partition without copying the whole file into RAM.
- Use the existing Rust `binbook` crate to read header, section table, and page index metadata.
- Stream one page through the tested PackBits row decompressor and `ssd1677-driver::write_row()`.
- Use the orientation probe `.binbook` from Milestone A as the first hardware fixture.
- Verification: flashed firmware displays page 0 with the same orientation expected from the host decoded physical image.

### Milestone E: Button navigation and serial upload loop

Objective: turn the one-page display into a minimal reader.

- Wire ADC button polling to page navigation:
  - RIGHT/DOWN: next page
  - LEFT/UP: previous page
- Keep page turns saturating at first/last page.
- Add serial commands one at a time: `INFO`, `LIST`, `UPLOAD`, `DELETE`.
- Verification: upload a two-page GRAY1 BinBook, list it, display page 0, navigate to page 1 and back, then delete/reupload.

### Milestone F: Repeatable flash procedure

Objective: make flashing and monitoring reproducible.

- Add scripts only after the manual commands are proven.
- Keep flash and serial monitor as separate commands by default because USB reset/re-enumeration can break a live monitor session.
- Document the exact command, expected serial output, and visible display result for each firmware milestone.

## Out of Scope for the Current Scaffold Tasks

These are not part of the already-implemented scaffold. They are handled by the hardware bring-up milestones above.

- Real ESP32-C3 peripheral initialization.
- Real GPIO/SPI/ADC/flash implementations.
- Flashing the physical Xteink X4.
- Custom SSD1677 LUT tuning.
- GRAY2 rendering.
- UI file picker.
- Power management and deep sleep.
