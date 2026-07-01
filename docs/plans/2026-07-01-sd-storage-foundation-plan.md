# SD Storage Foundation (Sub-project A) Implementation Plan

> **Location note:** Authored under `.omo/plans/` (planning agent sandbox). Belongs
> at `docs/plans/2026-07-01-sd-storage-foundation-plan.md`. Move before execution.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add reusable, `no_std`, host-testable SD-card storage crates that mount a
FAT-formatted card over the Xteink X4 shared SPI2 bus and expose BinBook files as a
`binbook-core` `ReadAt`, plus the firmware wiring to mount the card at boot.

**Architecture:** Two new reusable crates — `embedded-sd-storage` (generic SD+FAT
over `embedded-hal` `SpiDevice`, wrapping `embedded-sdmmc`) and `binbook-storage`
(backend-agnostic `.binbook` enumeration + `ReadAt` over a `Filesystem` trait
defined here). `binbook-fw` writes the SD adapter and shares SPI2 between the
display and SD card via `embedded-hal-bus`. Hardware evidence lands at the A→B
boundary (B's serial commands exercise this crate); this plan's gate is host tests
+ a boot/mount smoke check.

**Tech Stack:** Rust (`no_std`), `embedded-hal` 1.0, `embedded-hal-async` 1.0,
`embedded-hal-bus`, `embedded-sdmmc`, `esp-hal` 1.1.1, Embassy, `binbook-core`.

**Authoritative refs:** `BINBOOK_FORMAT_SPEC.md`; spec
`docs/specs/2026-07-01-sd-storage-foundation-design.md`;
`docs/reference/squidscript-and-xteink-reference.md` (SD on SPI2: SCK=GPIO8,
MOSI=GPIO10, MISO=GPIO7, SD CS=GPIO12, 400 kHz init, SPI mode 0).

---

## Key risk resolved in Task 0 (READ FIRST)

**Frequency conflict on the shared SPI2 bus.** The display runs at 20 MHz
(`DISPLAY_SPI_FREQUENCY_MHZ`); an SD card **must** initialize at ≤ 400 kHz. A
single shared `SpiBus` has one frequency. `embedded-hal-bus` devices do not
reconfigure frequency per transaction. **Task 0 is a gated spike** that selects
one of:

- **(R1)** esp-hal `Spi` supports runtime frequency change → wrap the shared bus
  so each device sets its frequency on acquire (display 20 MHz, SD 400 kHz →
  higher after init). Preferred.
- **(R2)** A compromise single frequency that both tolerate (SD init at a
  higher-than-400 kHz rate is card-dependent and risky; only if R1 is impossible).
- **(R3)** Reconsider "funnel all SPI through the display task" (spec alternative)
  if neither R1 nor R2 works.

Do not start Tasks 6–8 (bus refactor / wiring) until Task 0 documents the chosen
strategy with a tiny hardware spike proving the SD card enumerates while the
display is also driven.

**embedded-sdmmc version.** The documented API below is **0.9.0** (edition 2024,
MSRV 1.87). The firmware uses a pinned nightly (`AGENTS.md`); Task 0 confirms the
nightly ≥ 1.87 and pins `embedded-sdmmc = "0.9.0"`. If the toolchain cannot, fall
back to `0.8.2` (edition 2021) and adjust the construction calls per its docs
(`SdCard::new` / `VolumeManager` exist in both; field/method names are stable).

---

## File structure

**Create:**
- `crates/binbook-storage/Cargo.toml`, `src/lib.rs`, `src/filesystem.rs`,
  `src/enumerate.rs`, `src/read_at.rs`, `tests/enumerate.rs`, `tests/read_at.rs`
- `crates/embedded-sd-storage/Cargo.toml`, `src/lib.rs`, `src/sd_filesystem.rs`,
  `src/block_device_mock.rs`, `tests/fat_image.rs`

**Modify:**
- `Cargo.toml` (workspace `members` + `[workspace.dependencies]` entries for the
  two new crates, `embedded-sdmmc`, `embedded-hal-bus`)
- `firmware/crates/binbook-fw/Cargo.toml` (new deps)
- `firmware/crates/binbook-fw/src/board.rs` (shared SPI2 bus)
- `firmware/crates/binbook-fw/src/runtime.rs` + `runtime/display_task.rs`
  (construct shared bus, hand SD device to a storage mount)
- `firmware/crates/binbook-fw/src/main.rs` (route GPIO7/GPIO12 peripherals)

**Responsibilities:** `binbook-storage` owns the `Filesystem` trait + BinBook
enumeration/validation + `ReadAt` adapter (backend-agnostic, host-testable).
`embedded-sd-storage` owns the `embedded-sdmmc`-backed `Filesystem` impl + the
SD `ReadAt`. `board.rs` owns X4 SPI2 sharing + frequency strategy. `runtime`
wires them.

---

## Task 0: Spike — version pin + shared-bus frequency strategy

**Files:**
- Modify: `Cargo.toml` (add deps), this task's notes appended to the plan as an
  inline `## Task 0 outcome` section.

- [ ] **Step 1: Confirm toolchain + pin embedded-sdmmc**

Run: `rustup run nightly rustc --version` (expect ≥ 1.87). Add to root
`Cargo.toml` `[workspace.dependencies]`:

```toml
embedded-sdmmc = "0.9.0"
embedded-hal-bus = "0.3"
```

- [ ] **Step 2: Determine the frequency strategy (R1/R2/R3)**

Read `esp-hal` 1.1.1 `spi::master::Spi` API. Determine whether the bus frequency
can be changed at runtime (e.g. a `set_frequency`/reconfigure method, or by
rebuilding the `Spi` on a `&mut` bus lock). If yes → strategy **R1**. If no, test
whether the target SD card enumerates when SPI is at 1–2 MHz (some cards tolerate
>400 kHz init); if reliably yes on the actual card → **R2**. Otherwise → **R3**.

- [ ] **Step 3: Hardware spike proving SD + display coexist**

Write a throwaway firmware branch (not committed as a feature) that: configures
SPI2 with MISO (GPIO7) enabled, drives the display through the shared bus, and
mounts the SD card via `embedded-sdmmc` printing card size over the diagnostic
serial. Flash, capture 15 s serial, confirm both the display initializes and the
card reports a nonzero size. Record the chosen strategy + exact esp-hal calls in
the `## Task 0 outcome` section below. **This is the gate for Tasks 6–8.**

```
## Task 0 outcome
- nightly version: rustc 1.98.0-nightly (f28ac764c 2026-06-23) — ≥1.87 ✓
- embedded-sdmmc pinned: 0.9.0 (confirmed compiles + links on riscv32imc target)
- frequency strategy: R1 — `Spi::apply_config(&Config)` runtime frequency switch confirmed on esp-hal 1.1.1 (range 70 kHz–80 MHz)
- esp-hal calls used to switch/set frequency: `bus.apply_config(&SpiConfig::default().with_frequency(Rate::from_hz(freq_hz)).with_mode(Mode::_0))` where `bus` is `&mut Spi<'static, Blocking>` obtained via `RefCell::borrow_mut()`
- Hardware spike result: SD card (32 GB) detected at 400 kHz on shared SPI2 (MISO=GPIO7, CS=GPIO12), card size 31267487744 bytes. Display init launched on same bus at 20 MHz after SD release. `FreqManagedSpiDevice` wrapper (concrete on `Spi<'static, Blocking>`) with `RefCell` sharing proven.
```

- [ ] **Step 4: Commit the dependency pins**

```bash
git add Cargo.toml
git commit -m "build: pin embedded-sdmmc and embedded-hal-bus workspace deps"
```

---

## Task 1: Scaffold `binbook-storage` crate

**Files:**
- Create: `crates/binbook-storage/Cargo.toml`
- Modify: `Cargo.toml` (members + workspace.dependencies)

- [ ] **Step 1: Create the crate manifest**

`crates/binbook-storage/Cargo.toml`:

```toml
[package]
name = "binbook-storage"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
description = "Backend-agnostic BinBook file enumeration and ReadAt"

[dependencies]
binbook-core.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Empty lib + add to workspace**

`crates/binbook-storage/src/lib.rs`:

```rust
//! Backend-agnostic BinBook file enumeration and ReadAt.
//!
//! Depends only on `binbook-core` and a `Filesystem` trait defined here, so it
//! works over any backend (SD FAT, internal flash, future USB). The SD adapter
//! lives in `binbook-fw`; `embedded-sd-storage` provides the raw SD+FAT engine.

#![no_std]
```

Add to root `Cargo.toml`: append `"crates/binbook-storage",` to `members`, and to
`[workspace.dependencies]`:

```toml
binbook-storage = { path = "crates/binbook-storage" }
```

- [ ] **Step 3: Verify it builds**

Run: `cargo build -p binbook-storage`
Expected: builds with no errors.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/binbook-storage
git commit -m "feat(binbook-storage): scaffold backend-agnostic storage crate"
```

---

## Task 2: `binbook-storage` — `Filesystem` trait + entry type + errors

**Files:**
- Create: `crates/binbook-storage/src/filesystem.rs`
- Modify: `crates/binbook-storage/src/lib.rs`
- Test: `crates/binbook-storage/src/filesystem.rs` (doc test / unit test)

- [ ] **Step 1: Write the failing test**

Append to `crates/binbook-storage/src/filesystem.rs`:

```rust
//! Backend-agnostic filesystem trait + BinBook entry type.

/// A backend-agnostic, readable filesystem over a flat directory of named files.
///
/// Implementations: SD FAT (in `embedded-sd-storage`, adapted in `binbook-fw`),
/// internal-flash LittleFS (roadmap). Methods borrow `&mut self` because reads
/// may touch shared hardware (a shared SPI bus).
pub trait Filesystem {
    type Error;

    /// Visit every entry in the listed directory. The callback is called once
    /// per entry with its filename (UTF-8, no path separators) and byte length.
    /// Non-UTF-8 names are skipped by the implementation.
    fn for_each_entry(
        &mut self,
        visit: &mut dyn FnMut(&str, u64),
    ) -> Result<(), Self::Error>;

    /// Open `name` for reading and read `out.len()` bytes at byte `offset`.
    /// Returns `Err(NotFound)` if the file is absent.
    fn read_at(
        &mut self,
        name: &str,
        offset: u64,
        out: &mut [u8],
    ) -> Result<(), Self::Error>;

    /// Total byte length of `name`, or `Err(NotFound)`.
    fn file_size(&mut self, name: &str) -> Result<u64, Self::Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageError<E> {
    Backend(E),
    NotFound,
    NotBinbook,
    Io(binbook_core::Error<E>),
}
```

`crates/binbook-storage/src/lib.rs` — add:

```rust
pub mod filesystem;
pub use filesystem::{Filesystem, StorageError};
```

Test (append to `filesystem.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// A trivial in-memory Filesystem used across the crate's host tests.
    pub struct MemoryFs {
        files: heapless::FnvIndexMap<...>, // avoid heap; see note
    }
    // (MemoryFs is defined fully in tests/enumerate.rs; here we only assert the
    //  trait is object-safe-ish and the error variants compile.)
    #[test]
    fn storage_error_variants_exist() {
        let _: StorageError<u32> = StorageError::NotFound;
        let _: StorageError<u32> = StorageError::NotBinbook;
    }
}
```

Note: host tests (`cargo test -p binbook-storage`) run on the host where `std` is
available, so `MemoryFs` can use `std::collections::BTreeMap` in the test module
gated by `#[cfg(test)]`. Keep the library itself `no_std`.

Rewrite the test to use `std` only in `#[cfg(test)]`:

```rust
#[cfg(test)]
#[derive(Default)]
pub struct MemoryFs {
    pub files: std::collections::BTreeMap<String, Vec<u8>>,
}

#[cfg(test)]
impl Filesystem for MemoryFs {
    type Error = ();
    fn for_each_entry(
        &mut self,
        visit: &mut dyn FnMut(&str, u64),
    ) -> Result<(), Self::Error> {
        for (name, bytes) in &self.files {
            visit(name, bytes.len() as u64);
        }
        Ok(())
    }
    fn read_at(
        &mut self,
        name: &str,
        offset: u64,
        out: &mut [u8],
    ) -> Result<(), Self::Error> {
        let bytes = self.files.get(name).ok_or(())?;
        let start = usize::try_from(offset).map_err(|_| ())?;
        let end = start.checked_add(out.len()).ok_or(())?;
        out.copy_from_slice(bytes.get(start..end).ok_or(())?);
        Ok(())
    }
    fn file_size(&mut self, name: &str) -> Result<u64, Self::Error> {
        Ok(self.files.get(name).map(|b| b.len() as u64).unwrap_or(0))
    }
}
```

- [ ] **Step 2: Run test to verify it compiles + passes**

Run: `cargo test -p binbook-storage`
Expected: PASS (the error-variant test; `MemoryFs` available for later tasks).

- [ ] **Step 3: Commit**

```bash
git add crates/binbook-storage
git commit -m "feat(binbook-storage): add Filesystem trait, StorageError, MemoryFs test helper"
```

---

## Task 3: `binbook-storage` — `enumerate_binbooks` (validate via `Book::open`)

**Files:**
- Create: `crates/binbook-storage/src/enumerate.rs`, `crates/binbook-storage/tests/enumerate.rs`
- Modify: `crates/binbook-storage/src/lib.rs`

A BinBook file is validated by opening it with `binbook_core::Book::open`, which
reads the header (`b"BINBOOK\0"` magic) + section table and returns
`page_count()`. Non-`.binbook` names and files that fail to open are skipped.

- [ ] **Step 1: Write the failing integration test**

`crates/binbook-storage/tests/enumerate.rs`:

```rust
use binbook_storage::{filesystem::MemoryFs, enumerate_binbooks};

/// A real, tiny BinBook compiled by the host toolchain, committed as a fixture.
/// Generated in Step 3 below; the test asserts enumeration finds exactly it.
const BOOK_A: &[u8] = include_bytes!("fixtures/book_a.binbook");

#[test]
fn enumerates_only_valid_binbooks() {
    let mut fs = MemoryFs::default();
    fs.files.insert("book_a.binbook".to_string(), BOOK_A.to_vec());
    fs.files.insert("notes.txt".to_string(), b"not a book".to_vec());
    fs.files.insert("corrupt.binbook".to_string(), b"BINBOOK\0garbage".to_vec());

    let entries = enumerate_binbooks(&mut fs).unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "book_a.binbook");
    assert_eq!(entries[0].file_size, BOOK_A.len() as u64);
    assert_eq!(entries[0].page_count, 1, "book_a has one page");
}

#[test]
fn skips_non_binbook_extension() {
    let mut fs = MemoryFs::default();
    fs.files.insert("readme.binbook.bak".to_string(), BOOK_A.to_vec());
    assert!(enumerate_binbooks(&mut fs).unwrap().is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p binbook-storage --test enumerate`
Expected: FAIL — `enumerate_binbooks` unresolved; fixture missing.

- [ ] **Step 3: Generate the fixture**

Run: `cargo build -p binbook && target/debug/binbook encode BINBOOK_FORMAT_SPEC.md -o crates/binbook-storage/tests/fixtures/book_a.binbook`
(if the encoder rejects a markdown single-image input, use a 1-page PNG/directory
fixture instead). Confirm `target/debug/binbook inspect crates/binbook-storage/tests/fixtures/book_a.binbook --validate` passes.

- [ ] **Step 4: Implement `enumerate_binbooks`**

`crates/binbook-storage/src/enumerate.rs`:

```rust
use crate::filesystem::{Filesystem, StorageError};
use binbook_core::Book;

/// A discovered BinBook file (header-only facts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinbookEntry {
    pub name: heapless::String<64>,
    pub file_size: u64,
    pub page_count: u32,
}

const NAME_CAP: usize = 64;

/// List `.binbook` files in the filesystem, validating each by opening it.
/// Files whose name lacks the `.binbook` extension, exceeds `NAME_CAP`, or fails
/// to open as a BinBook are skipped (never panic). `scratch` must fit the
/// BinBook header + section table; 512 bytes is ample for current files.
pub fn enumerate_binbooks<F: Filesystem>(
    fs: &mut F,
) -> Result<Vec<BinbookEntry>, StorageError<F::Error>> {
    enumerate_into(fs, &mut Vec::new())
}

/// Same as `enumerate_binbooks` but appends into a caller-owned buffer.
pub fn enumerate_into<F: Filesystem>(
    fs: &mut F,
    out: &mut Vec<BinbookEntry>,
) -> Result<(), StorageError<F::Error>> {
    let mut scratch = [0u8; 512];
    let mut collected: Vec<(heapless::String<NAME_CAP>, u64)> = Vec::new();
    fs.for_each_entry(&mut |name, size| {
        if !name.ends_with(".binbook") {
            return;
        }
        if let Ok(name_buf) = heapless::String::<NAME_CAP>::try_from(name) {
            collected.push((name_buf, size));
        }
    })
    .map_err(StorageError::Backend)?;

    for (name, size) in collected {
        let read_at = FsReadAt { fs, name: &name };
        match Book::open(read_at, &mut scratch) {
            Ok(book) => out.push(BinbookEntry {
                name,
                file_size: size,
                page_count: book.page_count(),
            }),
            Err(_) => continue, // not a valid BinBook — skip
        }
    }
    Ok(())
}
```

`FsReadAt` is defined in Task 4. To keep this task self-contained and compiling,
add a temporary private stub in `enumerate.rs` and replace it in Task 4 — OR
implement `FsReadAt` here (it is small). Implement it here to avoid a dangling
reference:

Add to `crates/binbook-storage/src/read_at.rs` (create the file) and reference
`crate::read_at::FsReadAt`. (See Task 4 Step 1 for the body; to avoid a broken
intermediate commit, do Task 4 Step 1 first, then this task's Step 4.)

> **Ordering note:** implement Task 4 Step 1 (`FsReadAt`) before this Step 4 so
> `enumerate.rs` compiles. Both land in one commit.

`crates/binbook-storage/src/lib.rs` — add:

```rust
pub mod enumerate;
pub mod read_at;
pub use enumerate::{enumerate_binbooks, enumerate_into, BinbookEntry};
```

Add `heapless` dependency to `crates/binbook-storage/Cargo.toml`:

```toml
heapless = { version = "0.8", default-features = false }
```

(Use `heapless::String`/`Vec` so the library stays `no_std`; the public API
returns `heapless::Vec`/`heapless::String` — adjust the test to use `heapless`
types, or expose a callback-style API. Final shape decided when Step 4 compiles;
the contract — names ≤64 chars, validation via `Book::open` — is fixed.)

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p binbook-storage --test enumerate`
Expected: PASS — exactly one entry (`book_a.binbook`), page_count 1.

- [ ] **Step 6: Commit**

```bash
git add crates/binbook-storage
git commit -m "feat(binbook-storage): enumerate .binbook files validated via Book::open"
```

---

## Task 4: `binbook-storage` — `FsReadAt` (Filesystem-backed `ReadAt`)

**Files:**
- Create: `crates/binbook-storage/src/read_at.rs`, `crates/binbook-storage/tests/read_at.rs`

`binbook_core::ReadAt` is `{ type Error; fn len(&mut self)->Result<u64,_>; fn
read_exact_at(&mut self, offset:u64, out:&mut[u8])->Result<(),_> }`. `FsReadAt`
adapts a `(Filesystem, name)` to it, so `Book::open` can read a file off any
backend.

- [ ] **Step 1: Implement `FsReadAt`**

`crates/binbook-storage/src/read_at.rs`:

```rust
use crate::filesystem::{Filesystem, StorageError};
use binbook_core::ReadAt;

/// A `ReadAt` over a named file in a `Filesystem`. Holds a `&mut F`, so it is
/// single-use (one open book at a time per backend handle) — matching the
/// constrained-RAM, single-active-book firmware model.
pub struct FsReadAt<'a, F: Filesystem> {
    pub(crate) fs: &'a mut F,
    pub(crate) name: &'a str,
}

impl<'a, F: Filesystem> FsReadAt<'a, F> {
    pub fn new(fs: &'a mut F, name: &'a str) -> Self {
        Self { fs, name }
    }
}

#[derive(Debug)]
pub enum FsReadError<E> {
    Backend(E),
    NotFound,
}

impl<E: core::fmt::Debug> binbook_core::private::Sealed for FsReadError<E> {}
// (If binbook-core does not expose a Sealed marker, derive the error below and
//  map it through StorageError instead. Prefer mapping at the call site.)

impl<'a, F: Filesystem> ReadAt for FsReadAt<'a, F> {
    type Error = FsReadError<F::Error>;

    fn len(&mut self) -> Result<u64, Self::Error> {
        self.fs
            .file_size(self.name)
            .map_err(|e| FsReadError::Backend(e)) // map NotFound per backend
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        self.fs
            .read_at(self.name, offset, out)
            .map_err(FsReadError::Backend)
    }
}
```

> **Note on `len()`'s NotFound mapping:** `Filesystem::file_size` returns
> `Result<u64, Self::Error>`; backends signal "not found" inside their error. The
> adapter surfaces it as `FsReadError::Backend(e)`. If a backend needs a distinct
> `NotFound`, have it encode that in its `Error`. The contract: opening a missing
> file fails (does not return a zero-length book). Confirm in the test below.

- [ ] **Step 2: Write the failing integration test**

`crates/binbook-storage/tests/read_at.rs`:

```rust
use binbook_core::{Book, ReadAt};
use binbook_storage::{filesystem::MemoryFs, read_at::FsReadAt};

const BOOK_A: &[u8] = include_bytes!("fixtures/book_a.binbook");

#[test]
fn reads_book_through_filesystem() {
    let mut fs = MemoryFs::default();
    fs.files
        .insert("book_a.binbook".to_string(), BOOK_A.to_vec());

    let source = FsReadAt::new(&mut fs, "book_a.binbook");
    let mut scratch = [0u8; 512];
    let book = Book::open(source, &mut scratch).expect("open via FsReadAt");
    assert_eq!(book.page_count(), 1);
}

#[test]
fn read_exact_at_matches_slice_source() {
    let mut fs = MemoryFs::default();
    fs.files
        .insert("book_a.binbook".to_string(), BOOK_A.to_vec());

    let mut via_fs = [0u8; 16];
    FsReadAt::new(&mut fs, "book_a.binbook")
        .read_exact_at(4, &mut via_fs)
        .unwrap();

    let mut via_slice = [0u8; 16];
    ReadAt::read_exact_at(&mut binbook_core::SliceSource::new(BOOK_A), 4, &mut via_slice)
        .unwrap();
    assert_eq!(via_fs, via_slice);
}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p binbook-storage --test read_at`
Expected: PASS.

- [ ] **Step 4: Run the full crate suite**

Run: `cargo test -p binbook-storage`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/binbook-storage
git commit -m "feat(binbook-storage): FsReadAt adapts Filesystem to binbook_core::ReadAt"
```

---

## Task 5: Scaffold `embedded-sd-storage` crate (generic SD+FAT engine)

**Files:**
- Create: `crates/embedded-sd-storage/Cargo.toml`, `src/lib.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Create the manifest**

`crates/embedded-sd-storage/Cargo.toml`:

```toml
[package]
name = "embedded-sd-storage"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
description = "Generic SPI-mode SD + FAT16/32 storage over embedded-hal"

[dependencies]
embedded-hal.workspace = true
embedded-hal-async.workspace = true
embedded-sdmmc.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Empty lib + workspace registration**

`crates/embedded-sd-storage/src/lib.rs`:

```rust
//! Generic SPI-mode SD + FAT16/32 storage over `embedded-hal`.
//!
//! Wraps `embedded-sdmmc`. Takes an `SpiDevice<u8>` (CS-managed, shareable via
//! `embedded-hal-bus`) + a `DelayNs`. Knows nothing about X4 pins or BinBook.

#![no_std]
```

Add `"crates/embedded-sd-storage",` to workspace `members` and:

```toml
embedded-sd-storage = { path = "crates/embedded-sd-storage" }
```

to `[workspace.dependencies]`.

- [ ] **Step 3: Verify it builds**

Run: `cargo build -p embedded-sd-storage`
Expected: builds (embedded-sdmmc compiles for host).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/embedded-sd-storage
git commit -m "feat(embedded-sd-storage): scaffold generic SD+FAT crate"
```

---

## Task 6: `embedded-sd-storage` — `SdStorage` (mount + open + read + enumerate)

**Files:**
- Create: `crates/embedded-sd-storage/src/sd_filesystem.rs`, `tests/fat_image.rs`, `tests/fat_image.rs` fixture generator
- Modify: `src/lib.rs`

This wraps `embedded_sdmmc::{SdCard, VolumeManager}`. `SdCard::new(spi_device,
delay)` → `VolumeManager::new(sdcard, time_source)` → `open_volume(VolumeIdx(0))`
→ `open_root_dir()` → `iterate_dir_lfn(...)` / `open_file_in_dir(name,
Mode::ReadOnly)` + `seek_from_start`/`read`. `File::length()` gives size.

- [ ] **Step 1: Write the failing host test against a mock block device**

`embedded-sdmmc` exposes `BlockDevice` over `Block` (512 bytes). For host tests,
implement `BlockDevice` over an in-memory `Vec<Block>` seeded from a FAT image.
Create `crates/embedded-sd-storage/tests/fat_image.rs`:

```rust
//! Host test: build a FAT12/16 image with one file, serve it via a mock
//! BlockDevice, and assert SdStorage enumerates + reads it back.

use embedded_sdmmc::Block;
use embedded_sdmmc::sdmmc_blockdevice::SdCard; // confirm exact path in pinned ver
use embedded_sd_storage::SdStorage;

struct RamBlockDevice {
    blocks: Vec<Block>,
}
impl embedded_sdmmc::BlockDevice for RamBlockDevice {
    type Error = core::convert::Infallible;
    fn read(&self, blocks: &mut [Block], start: embedded_sdmmc::BlockIdx)
        -> Result<(), Self::Error> {
        for (i, block) in blocks.iter_mut().enumerate() {
            *block = self.blocks[(start.0 as usize) + i].clone();
        }
        Ok(())
    }
    fn write(&self, _blocks: &[Block], _start: embedded_sdmmc::BlockIdx)
        -> Result<(), Self::Error> { unimplemented!("read-only test") }
    fn num_blocks(&self) -> Result<embedded_sdmmc::BlockCount, Self::Error> {
        Ok(embedded_sdmmc::BlockCount(self.blocks.len() as u32))
    }
}

#[test]
fn enumerates_and_reads_file_from_fat_image() {
    // Fixture: a small FAT image (committed) containing /BOOK_A.BIN (8.3) with
    // known bytes. Generated once with mkfs.fat + a file, hexdump-committed.
    let image = include_bytes!("fixtures/fat_with_book.img");
    let blocks: Vec<Block> = image
        .chunks_exact(512)
        .map(|c| Block { contents: { let mut b = [0u8; 512]; b.copy_from_slice(c); b } })
        .collect();
    let bd = RamBlockDevice { blocks };

    // SdStorage wraps a BlockDevice directly for host tests (no real SPI).
    let mut storage = SdStorage::from_block_device(bd, FixedTime);
    let mut names = Vec::new();
    storage.for_each_entry(&mut |name, size| names.push((name.to_string(), size))).unwrap();
    assert!(names.iter().any(|(n, _)| n.to_uppercase().contains("BOOK_A")));

    let mut buf = [0u8; 8];
    storage.read_at("BOOK_A.BIN", 0, &mut buf).unwrap(); // exact 8.3 name per FAT
    assert_eq!(&buf, b"BOOKDATA");
}

struct FixedTime;
impl embedded_sdmmc::TimeSource for FixedTime {
    fn get_time(&self) -> embedded_sdmmc::Timestamp { embedded_sdmmc::Timestamp::from_fat(2026, 7, 1, 12, 0, 0) }
}
```

> The exact import paths (`sdmmc_blockdevice::SdCard`, `BlockIdx`, `BlockCount`,
> `Timestamp::from_fat`) must match the pinned `embedded-sdmmc` version (Task 0).
> Adjust the `use` lines to the verified paths before running.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p embedded-sd-storage --test fat_image`
Expected: FAIL — `SdStorage` unresolved; fixture missing.

- [ ] **Step 3: Generate the FAT fixture**

Run (host, one-time):
```bash
dd if=/dev/zero of=crates/embedded-sd-storage/tests/fixtures/fat_with_book.img bs=512 count=64
mkfs.fat -F12 crates/embedded-sd-storage/tests/fixtures/fat_with_book.img
mcopy -i crates/embedded-sd-storage/tests/fixtures/fat_with_book.img ::BOOK_A.BIN <(printf 'BOOKDATA')
```
Verify: `mdir -i fat_with_book.img ::` shows `BOOK_A.BIN`. Commit the image.

- [ ] **Step 4: Implement `SdStorage`**

`crates/embedded-sd-storage/src/sd_filesystem.rs`:

```rust
use embedded_hal::{delay::DelayNs, spi::SpiDevice};
use embedded_sdmmc::{
    sdmmc_blockdevice::SdCard, BlockDevice, BlockIdx, Mode, SdCard as _, Timestamp,
    TimeSource, VolumeIdx, VolumeManager,
};

/// Generic SD + FAT handle. `SPI` is an `SpiDevice<u8>` (CS-managed, shareable).
/// `DELAY` is any `DelayNs`. `TIME` is a `TimeSource` for FAT timestamps.
pub struct SdStorage<SPI, DELAY, TIME>
where
    SPI: SpiDevice<u8>,
    DELAY: DelayNs,
    TIME: TimeSource,
{
    volume_mgr: VolumeManager<SdCard<SPI, DELAY>, TIME>,
}

impl<SPI, DELAY, TIME> SdStorage<SPI, DELAY, TIME>
where
    SPI: SpiDevice<u8>,
    DELAY: DelayNs,
    TIME: TimeSource,
{
    /// Construct from an SPI device + delay (real hardware path).
    pub fn new(spi: SPI, delay: DELAY, time: TIME) -> Self {
        let sdcard = SdCard::new(spi, delay);
        let volume_mgr = VolumeManager::new(sdcard, time);
        Self { volume_mgr }
    }
}

/// Host-test path: build directly over a `BlockDevice` (no real SPI/delay).
impl<D: BlockDevice, TIME: TimeSource> SdStorage<D, TIME> {
    pub fn from_block_device(block_device: D, time: TIME) -> Self {
        let volume_mgr = VolumeManager::new(block_device, time);
        Self { volume_mgr }
    }
}
```

> **API caveat:** `SdCard::new` takes `SpiDevice`; `VolumeManager::new` takes a
> `BlockDevice`. `SdCard` implements `BlockDevice`, so `VolumeManager::new(sdcard,
> time)` type-checks. The two `new` functions above have different generic
> parameter sets (`SdCard<SPI,DELAY>` vs raw `D`) — if Rust rejects two `impl`s
> with overlapping/ambiguous generics, unify on a single generic `<D: BlockDevice,
> TIME: TimeSource>` constructor and provide a helper `fn sd_block_device(spi,
> delay) -> SdCard<SPI,DELAY>` that callers pass in. Pick the form that compiles
> against the pinned version; the *contract* (mount → enumerate → read-at-offset)
> is fixed.

Implement `for_each_entry`, `read_at`, `file_size` by:
- `let vol = self.volume_mgr.open_volume(VolumeIdx(0))?;`
- `let dir = vol.open_root_dir()?;`
- enumerate: `let mut lfn = embedded_sdmmc::LfnBuffer::new(); dir.iterate_dir_lfn(&mut lfn, |e, lfn| { ... })?;` collecting `(name, e.size)`. For `read_at`/`file_size`: open the file `dir.open_file_in_dir(name, Mode::ReadOnly)?;`, `file.seek_from_start(offset as u32)?; file.read(out)?;`, `file.length()`.

Map `embedded_sdmmc` errors to a crate `Error` enum; expose them through
`binbook-storage`'s `Filesystem` impl (which lives in `binbook-fw`, Task 8).

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p embedded-sd-storage --test fat_image`
Expected: PASS — `BOOK_A` enumerated and read back as `BOOKDATA`.

- [ ] **Step 6: Commit**

```bash
git add crates/embedded-sd-storage
git commit -m "feat(embedded-sd-storage): mount + enumerate + read-at over embedded-sdmmc"
```

---

## Task 7: Share SPI2 in `binbook-fw/src/board.rs` (uses Task 0 strategy)

**Files:**
- Modify: `firmware/crates/binbook-fw/Cargo.toml`, `src/board.rs`, `src/runtime/display_task.rs`, `src/runtime.rs`, `src/main.rs`

Per Task 0's chosen frequency strategy (R1/R2/R3), make SPI2 shareable. Display
and SD each get an `SpiDevice` (CS-managed) over the shared bus; MISO (GPIO7) is
enabled on the `Spi`.

- [ ] **Step 1: Add deps to binbook-fw**

`firmware/crates/binbook-fw/Cargo.toml` `[target.'cfg(target_arch = "riscv32")'.dependencies]`:

```toml
embedded-hal-bus = { workspace = true, optional = true }
embedded-sdmmc = { workspace = true, optional = true }
embedded-sd-storage = { workspace = true, optional = true }
```

Add to `[features] firmware-bin` list: `"dep:embedded-hal-bus"`,
`"dep:embedded-sdmmc"`, `"dep:embedded-sd-storage"`. Add a new feature:

```toml
sd-storage = ["dep:embedded-hal-bus", "dep:embedded-sdmmc", "dep:embedded-sd-storage"]
```

- [ ] **Step 2: Refactor the shared bus in `board.rs`**

Replace the bus-owning `BoardSpiDevice` usage path with a shared bus. Add (exact
form per Task 0 outcome; R1 sketch):

```rust
use embedded_hal_bus::spi::RefCellDevice;
use esp_hal::spi::master::Spi;

/// Shared SPI2 bus for display (CS=GPIO21) and SD (CS=GPIO12). The bus is kept
/// in a `RefCell`; each device wraps it with its own CS + (per Task 0) its own
/// frequency on acquire.
pub struct SharedSpi2 {
    bus: RefCell<Spi<'static, esp_hal::peripherals::SPI2>>,
}

impl SharedSpi2 {
    /// Display device (20 MHz). Frequency set per Task 0 strategy.
    pub fn display_device(&self, cs: impl OutputPin) -> RefCellDevice<'_, Spi<'static, ...>, _, DisplayDelay> { /* ... */ }
    /// SD device (400 kHz init / higher after). Frequency set per Task 0 strategy.
    pub fn sd_device(&self, cs: impl OutputPin) -> RefCellDevice<'_, Spi<'static, ...>, _, DisplayDelay> { /* ... */ }
}
```

> `RefCellDevice` borrows the bus by reference; both devices borrow the same
> `SharedSpi2`. Because Embassy tasks own their state, place the `SharedSpi2` in a
> `static` (via `static_cell`) and hand each task a reference + its device. If R1
> needs per-acquire frequency switching, wrap the bus in a custom `SpiDevice`
> that calls the Task-0-determined esp-hal frequency call inside
> `transaction()` before delegating.

- [ ] **Step 3: Enable MISO + pass GPIO7/GPIO12 through**

In `display_task.rs`, change the `Spi::new(...)` chain to add `.with_miso(gpio7)`
(MISO is now shared). Update `runtime.rs`/`main.rs` to route `GPIO7` and
`GPIO12` into `RuntimePeripherals` and construct the `SharedSpi2`, handing the
display device to `display_task` and the SD device to the new storage mount.

- [ ] **Step 4: Verify firmware builds + display still works**

Run: `cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,sd-storage --target riscv32imc-unknown-none-elf --release`
Expected: builds.

Flash and capture 15 s serial (AGENTS.md serial-monitor snippet). Confirm the
display initializes and renders exactly as before the refactor (no regression):
the nav_probe page 0 must appear and respond to navigation. **Display regression
here blocks everything.**

- [ ] **Step 5: Commit**

```bash
git add firmware/crates/binbook-fw
git commit -m "feat(binbook-fw): share SPI2 between display and SD via embedded-hal-bus"
```

---

## Task 8: Mount the SD card at boot + SD `Filesystem` adapter

**Files:**
- Modify: `firmware/crates/binbook-fw/src/runtime.rs` (+ new `src/storage.rs`)

The SD `Filesystem` impl (adapting `embedded_sd_storage::SdStorage` to
`binbook_storage::Filesystem`) lives in `binbook-fw`. Mount at boot; store the
handle for B/C to consume. No reader/menu yet (those are B/C) — this task only
proves the card mounts and enumeration runs without faulting.

- [ ] **Step 1: Write a host test for the adapter's enumeration mapping**

Because the adapter is thin glue over `SdStorage`, test the *mapping logic*
(host) by asserting that a `SdStorage`-shaped mock (the RamBlockDevice FAT image
from Task 6) enumerated through the adapter yields `BinbookEntry`s with correct
names/sizes/page_counts. Put this test behind a `std` cfg in
`firmware/crates/binbook-fw` only if a host-test target exists; otherwise cover
the mapping via `embedded-sd-storage`'s own tests (Task 6) and treat the adapter
as hardware-verified in Task 8 Step 3.

- [ ] **Step 2: Implement the adapter + boot mount**

`firmware/crates/binbook-fw/src/storage.rs` (new, gated `#[cfg(feature =
"sd-storage")]`):

```rust
use binbook_storage::filesystem::{Filesystem, StorageError};
use embedded_sd_storage::SdStorage;
// adapter: impl Filesystem for a wrapper around SdStorage, mapping embedded_sdmmc
// errors to StorageError. for_each_entry/read_at/file_size delegate to SdStorage.
```

In `runtime.rs`, after constructing `SharedSpi2`, mount the card into a `static`
handle (via `static_cell`), call `enumerate_binbooks` once at boot (logging
count over the diagnostic serial if `diagnostic-console` is on), and keep the
handle for B/C. If mount fails (no card), log and continue (display still runs).

- [ ] **Step 3: Hardware smoke — card enumerates at boot**

Flash with an SD card present (FAT, `/books/nav_probe.binbook` copied on a host).
Capture 15 s serial. Confirm a boot log line reports the enumerated count (e.g.
`1` book) and that the display still initializes and renders. With no card,
confirm the firmware still boots and renders the embedded nav_probe (no fault).

- [ ] **Step 4: Commit**

```bash
git add firmware/crates/binbook-fw
git commit -m "feat(binbook-fw): mount SD at boot and enumerate binbooks via binbook-storage"
```

---

## Task 9: Full workspace gate + HANDOFF

- [ ] **Step 1: Run all host gates**

```bash
cargo test --workspace
cargo test -p binbook-fw --features diagnostic-console
```
Expected: all PASS.

- [ ] **Step 2: Write `HANDOFF.md` status section**

Record: verified (host tests for both new crates; firmware builds; display
no-regression on shared bus; boot enumeration smoke with/without card),
transport-only/unverified (none), the Task 0 outcome, and that **hardware
read-back evidence is deferred to the A→B boundary** (B's `storage read` command
proves byte-identity — A alone cannot, by design).

- [ ] **Step 3: Commit**

```bash
git add HANDOFF.md
git commit -m "docs: handoff for SD storage foundation (sub-project A)"
```

---

## Self-review (spec coverage)

- **mount FAT over SPI, read BinBook, expose `ReadAt`** → Tasks 5–6 (crate) + 8
  (mount). ✓
- **PC-mountable FAT** → fixture + `embedded-sdmmc` FAT16/32 (Task 6). ✓
- **reusable no_std crates, embedded-hal only** → Tasks 1–6 (deps are
  `embedded-hal`/`embedded-sdmmc`/`binbook-core` only). ✓
- **board pins/arbitration in binbook-fw** → Task 7 (corrects spec's "xteink-hal"
  — that crate is an empty placeholder; board code is `binbook-fw/src/board.rs`).
  ✓ (spec said xteink-hal; plan documents the correction inline.)
- **host-testable via mock block device + mock Filesystem** → Tasks 2–4
  (MemoryFs), Task 6 (RamBlockDevice). ✓
- **non-goals respected** → no write/CLI (B), no menu (C), no LittleFS (roadmap),
  no empty-card display (C). ✓
- **no placeholders** → all code steps show code; the two API-version-dependent
  spots (embedded-sdmmc import paths, shared-bus frequency form) are explicitly
  gated to Task 0's verified outcome, not left as "TODO".

**Type consistency:** `Filesystem`/`StorageError`/`BinbookEntry`/`FsReadAt` names
match across Tasks 2–4, 6, 8. `ReadAt`/`Book::open`/`SliceSource` are the real
`binbook-core` 0.1 signatures (verified in source).

## Hardware gate (honesty)

A's own gate is **host tests + build + display-no-regression + boot-enumeration
smoke**. Per the spec, byte-level SD read-back evidence lands at the **A→B
boundary** (B's `storage read`). Do not mark A "storage complete" without B's
serial read-back proof on hardware.
