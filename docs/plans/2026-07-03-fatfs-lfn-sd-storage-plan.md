# FAT Long Filename SD Storage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Repo override: do not dispatch subagents for this repository; execute sequentially in the current branch/worktree.

**Goal:** Replace the temporary `.BBK` short-name workaround with real FAT long filename support so uploaded books are stored and listed as normal `.binbook` files on SD cards.

**Architecture:** Keep `binbook-storage` and the diagnostic storage protocol unchanged. Refactor `embedded-sd-storage` so FAT implementation details are behind a small directory backend boundary, then spike `fatfs` as the FAT directory implementation. If `fatfs` cannot pass the firmware no-global-allocator build gate, stop the replacement and patch `embedded-sdmmc` LFN create/open/delete instead.

**Tech Stack:** Rust, `embedded-sdmmc` for SD block-device access, candidate `fatfs` for FAT12/16/32 + LFN, `binbook-storage::WritableFilesystem`, cargo host tests, Xteink X4 live diagnostic firmware.

---

## Current Constraints

- Work from repo root: `/var/home/tristan/Documents/parasquid/binbook`.
- Follow `AGENTS.md`: plans live in `docs/plans`, use TDD, run `cargo test --workspace`, run `uv run pytest -q`, and perform live hardware verification for firmware changes.
- Do not create or switch branches unless the user asks.
- Do not stage or commit `HANDOFF.md`; it is local state only.
- Do not keep the current logical-to-physical `.binbook` -> `.BBK` mapping after this plan completes.
- The public firmware-facing interface remains `binbook_storage::Filesystem` and `WritableFilesystem`.
- The hardware acceptance gate is unchanged: upload `booka.binbook`, list exactly `booka.binbook`, read back byte-identical content, delete it, and confirm list is empty.

## File Structure

- Modify `Cargo.toml`: add workspace dependency for the selected FAT crate only after the spike proves viable.
- Modify `crates/embedded-sd-storage/Cargo.toml`: depend on `fatfs` if viable; keep `embedded-sdmmc` for SD card block access.
- Create `crates/embedded-sd-storage/src/block_stream.rs`: converts an `embedded_sdmmc::BlockDevice` into a seek/read/write stream usable by the FAT implementation. This isolates crate-specific I/O glue.
- Create `crates/embedded-sd-storage/src/fat_directory.rs`: owns `/BOOKS` operations (`for_each_entry`, `read_at`, `file_size`, `create_or_truncate`, `write_at`, `flush`, `delete_file`) against the selected FAT crate.
- Modify `crates/embedded-sd-storage/src/sd_filesystem.rs`: keep public `SdStorage<D, TIME>` shape if possible, but delegate FAT behavior to `fat_directory.rs`; remove `physical_name`, `logical_name`, and `format_short_name`.
- Modify `crates/embedded-sd-storage/src/lib.rs`: export only stable public types, not backend-specific implementation details.
- Modify `crates/embedded-sd-storage/tests/fat_image.rs`: add tests that prove real LFN creation, listing, readback, deletion, and no `.BBK` artifact.
- Modify `firmware/crates/binbook-fw/src/storage.rs` only if the error type or constructor needs a small adapter update.
- Modify `HANDOFF.md`: record verified behavior, command outputs, and any fallback decision.

## Task 1: Add Failing Tests For Real `.binbook` Long Filenames

**Files:**
- Modify: `crates/embedded-sd-storage/tests/fat_image.rs`

- [ ] **Step 1: Refactor test block storage so tests can inspect the image after writes**

Replace the current `RamBlockDevice` helper with a cloneable shared block store:

```rust
#[derive(Debug, Clone)]
struct SharedBlocks(std::rc::Rc<RefCell<Vec<Block>>>);

impl SharedBlocks {
    fn from_fixture() -> Self {
        let blocks = FAT_IMAGE
            .chunks_exact(512)
            .map(|c| {
                let mut b = Block::new();
                b.contents.copy_from_slice(c);
                b
            })
            .collect();
        Self(std::rc::Rc::new(RefCell::new(blocks)))
    }
}

impl BlockDevice for SharedBlocks {
    type Error = core::convert::Infallible;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), Self::Error> {
        let source = self.0.borrow();
        for (i, block) in blocks.iter_mut().enumerate() {
            *block = source[start.0 as usize + i].clone();
        }
        Ok(())
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), Self::Error> {
        let mut target = self.0.borrow_mut();
        for (i, block) in blocks.iter().enumerate() {
            target[start.0 as usize + i] = block.clone();
        }
        Ok(())
    }

    fn num_blocks(&self) -> Result<BlockCount, Self::Error> {
        Ok(BlockCount(self.0.borrow().len() as u32))
    }
}

fn storage_from_fixture() -> (SdStorage<SharedBlocks, FixedTime>, SharedBlocks) {
    let blocks = SharedBlocks::from_fixture();
    (SdStorage::from_block_device(blocks.clone(), FixedTime), blocks)
}
```

- [ ] **Step 2: Add a failing test for a long stem `.binbook`**

Add this test:

```rust
#[test]
fn long_binbook_filename_roundtrips_as_real_directory_name() {
    let (mut storage, _blocks) = storage_from_fixture();

    storage.create_or_truncate("long-book-title.binbook").unwrap();
    storage
        .write_at("long-book-title.binbook", 0, b"BOOKDATA")
        .unwrap();
    storage.flush("long-book-title.binbook").unwrap();

    let mut names: Vec<(String, u64)> = Vec::new();
    storage
        .for_each_entry(&mut |name, size| names.push((name.to_string(), size)))
        .unwrap();

    assert_eq!(names, vec![("long-book-title.binbook".to_string(), 8)]);

    let mut buf = [0u8; 8];
    storage
        .read_at("long-book-title.binbook", 0, &mut buf)
        .unwrap();
    assert_eq!(&buf, b"BOOKDATA");

    storage.delete_file("long-book-title.binbook").unwrap();
    names.clear();
    storage
        .for_each_entry(&mut |name, size| names.push((name.to_string(), size)))
        .unwrap();
    assert!(names.is_empty(), "expected empty /BOOKS, got {names:?}");
}
```

- [ ] **Step 3: Replace the existing workaround test**

Replace `binbook_extension_maps_to_short_fat_name` with a test named `short_stem_binbook_is_not_rewritten_to_bbk`. It should create `small.binbook`, list it as `small.binbook`, and assert no entry ending in `.BBK` appears:

```rust
#[test]
fn short_stem_binbook_is_not_rewritten_to_bbk() {
    let (mut storage, _blocks) = storage_from_fixture();

    storage.create_or_truncate("small.binbook").unwrap();
    storage.write_at("small.binbook", 0, b"BOOKDATA").unwrap();
    storage.flush("small.binbook").unwrap();

    let mut names: Vec<(String, u64)> = Vec::new();
    storage
        .for_each_entry(&mut |name, size| names.push((name.to_string(), size)))
        .unwrap();

    assert_eq!(names, vec![("small.binbook".to_string(), 8)]);
    assert!(!names.iter().any(|(name, _)| name.ends_with(".BBK")));
}
```

- [ ] **Step 4: Run tests and verify failure**

Run:

```bash
HOME=/var/home/tristan RUSTUP_TOOLCHAIN=stable cargo test -p embedded-sd-storage --test fat_image -- --nocapture
```

Expected before implementation: `long_binbook_filename_roundtrips_as_real_directory_name` fails because the current `embedded-sdmmc` path cannot create/open long `.binbook` names.

## Task 2: Spike `fatfs` Behind A Block Stream Adapter

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/embedded-sd-storage/Cargo.toml`
- Create: `crates/embedded-sd-storage/src/block_stream.rs`
- Modify: `crates/embedded-sd-storage/src/lib.rs`
- Test: `crates/embedded-sd-storage/tests/fat_image.rs`

- [ ] **Step 1: Add the candidate dependency**

In root `Cargo.toml`, add:

```toml
fatfs = { version = "0.3.6", default-features = false, features = ["alloc"] }
```

In `crates/embedded-sd-storage/Cargo.toml`, add:

```toml
fatfs.workspace = true
```

Do not remove `embedded-sdmmc`; it is still the SD card block-device layer.

- [ ] **Step 2: Create the block stream adapter**

Create `crates/embedded-sd-storage/src/block_stream.rs`:

```rust
use core::cmp::min;

use embedded_sdmmc::{Block, BlockDevice, BlockIdx};

pub struct BlockDeviceStream<D>
where
    D: BlockDevice,
{
    device: D,
    position: u64,
    scratch: Block,
}

impl<D> BlockDeviceStream<D>
where
    D: BlockDevice,
{
    pub fn new(device: D) -> Self {
        Self {
            device,
            position: 0,
            scratch: Block::new(),
        }
    }

    pub fn into_inner(self) -> D {
        self.device
    }

    fn read_bytes(&mut self, mut out: &mut [u8]) -> Result<usize, D::Error> {
        let total = out.len();
        while !out.is_empty() {
            let block_idx = BlockIdx((self.position / 512) as u32);
            let block_off = (self.position % 512) as usize;
            self.device
                .read(core::slice::from_mut(&mut self.scratch), block_idx)?;
            let len = min(out.len(), 512 - block_off);
            out[..len].copy_from_slice(&self.scratch.contents[block_off..block_off + len]);
            self.position += len as u64;
            out = &mut out[len..];
        }
        Ok(total)
    }

    fn write_bytes(&mut self, mut input: &[u8]) -> Result<usize, D::Error> {
        let total = input.len();
        while !input.is_empty() {
            let block_idx = BlockIdx((self.position / 512) as u32);
            let block_off = (self.position % 512) as usize;
            self.device
                .read(core::slice::from_mut(&mut self.scratch), block_idx)?;
            let len = min(input.len(), 512 - block_off);
            self.scratch.contents[block_off..block_off + len].copy_from_slice(&input[..len]);
            self.device
                .write(core::slice::from_ref(&self.scratch), block_idx)?;
            self.position += len as u64;
            input = &input[len..];
        }
        Ok(total)
    }
}
```

- [ ] **Step 3: Add std-host trait impls first**

Add this to `block_stream.rs` behind `#[cfg(feature = "std")]` only if needed for the initial host spike. If `embedded-sd-storage` has no `std` feature, add one in its `Cargo.toml` for tests only:

```rust
#[cfg(feature = "std")]
impl<D> std::io::Read for BlockDeviceStream<D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        self.read_bytes(out)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, format!("{error:?}")))
    }
}

#[cfg(feature = "std")]
impl<D> std::io::Write for BlockDeviceStream<D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
        self.write_bytes(input)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, format!("{error:?}")))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(feature = "std")]
impl<D> std::io::Seek for BlockDeviceStream<D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        let next = match pos {
            std::io::SeekFrom::Start(offset) => offset,
            std::io::SeekFrom::Current(delta) => self.position.checked_add_signed(delta).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "negative seek")
            })?,
            std::io::SeekFrom::End(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "SeekFrom::End is not supported for block devices",
                ));
            }
        };
        self.position = next;
        Ok(self.position)
    }
}
```

- [ ] **Step 4: Run the host spike compile**

Run:

```bash
HOME=/var/home/tristan RUSTUP_TOOLCHAIN=stable cargo test -p embedded-sd-storage --test fat_image --no-run
```

Expected: compile reaches test code. If the compile fails because `fatfs` requires `std`, fix dependency flags. If it fails because `fatfs` APIs require `alloc`, continue the spike but keep Task 3 as a hard firmware gate.

## Task 3: Firmware Build Gate For `fatfs`

**Files:**
- Modify only dependency/features if required by the spike.

- [ ] **Step 1: Attempt the no-std firmware release build before rewriting storage**

Run:

```bash
cd firmware && HOME=/var/home/tristan RUSTC="$(HOME=/var/home/tristan rustup which --toolchain nightly rustc)" HOME=/var/home/tristan rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console,sd-storage --target riscv32imc-unknown-none-elf --release
```

Expected viable result: the build reaches `binbook-fw` code or succeeds without introducing a global allocator.

- [ ] **Step 2: Decide the path**

If the build fails with allocator/global-alloc errors from `fatfs`, do not add a firmware allocator as part of this task. Record in `HANDOFF.md` that `fatfs` is not viable under current no-allocator firmware constraints, revert only the `fatfs` dependency/spike edits, and switch to Task 8.

If the build does not fail for allocator reasons, continue to Task 4.

## Task 4: Implement `fat_directory` And Remove Name Mapping

**Files:**
- Create: `crates/embedded-sd-storage/src/fat_directory.rs`
- Modify: `crates/embedded-sd-storage/src/sd_filesystem.rs`
- Modify: `crates/embedded-sd-storage/src/lib.rs`

- [ ] **Step 1: Create `fat_directory.rs`**

Create a module that owns `/BOOKS` operations. The exact type names may change to satisfy `fatfs` lifetimes, but the public methods must match this shape:

```rust
pub const BOOKS_DIR: &str = "BOOKS";

pub struct FatDirectory<D> {
    stream: crate::block_stream::BlockDeviceStream<D>,
}

impl<D> FatDirectory<D>
where
    D: embedded_sdmmc::BlockDevice,
    D::Error: core::fmt::Debug,
{
    pub fn new(device: D) -> Self {
        Self {
            stream: crate::block_stream::BlockDeviceStream::new(device),
        }
    }

    pub fn into_inner(self) -> D {
        self.stream.into_inner()
    }

    pub fn for_each_entry(
        &mut self,
        visit: &mut dyn FnMut(&str, u64),
    ) -> Result<(), StorageError<D>> {
        // Mount the FAT filesystem, open /BOOKS if present, and call visit with
        // each regular file's exact FAT filename and byte length.
    }

    pub fn read_at(
        &mut self,
        name: &str,
        offset: u64,
        out: &mut [u8],
    ) -> Result<(), StorageError<D>> {
        // Mount, open /BOOKS/name, seek, read exactly out.len() bytes or zero-fill
        // short reads to preserve current FsReadAt behavior.
    }

    pub fn file_size(&mut self, name: &str) -> Result<u64, StorageError<D>> {
        // Mount, open /BOOKS/name, return length.
    }

    pub fn create_or_truncate(&mut self, name: &str) -> Result<(), StorageError<D>> {
        // Mount, create /BOOKS if missing, create/open file, truncate to zero.
    }

    pub fn write_at(
        &mut self,
        name: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), StorageError<D>> {
        // Mount, open /BOOKS/name, seek, write all bytes, flush.
    }

    pub fn flush(&mut self, name: &str) -> Result<(), StorageError<D>> {
        // Mount, open /BOOKS/name, flush file/directory metadata if the backend exposes it.
    }

    pub fn delete_file(&mut self, name: &str) -> Result<(), StorageError<D>> {
        // Mount, remove /BOOKS/name.
    }
}
```

Use exact `fatfs` APIs after resolving the lifetimes. Do not expose `fatfs` types from this module.

- [ ] **Step 2: Remove the short-name workaround**

Delete these functions from `sd_filesystem.rs`:

```rust
fn format_short_name(...)
fn physical_name(...)
fn logical_name(...)
```

Remove all call sites of `physical_name` and `logical_name`.

- [ ] **Step 3: Delegate `SdStorage` to `FatDirectory`**

Keep `SdStorage::new(spi, delay, time)` and `SdStorage::from_block_device(block_device, time)` call sites stable if possible. Internally, `time` may become unused for the `fatfs` path; keep it as `PhantomData<TIME>` to avoid rippling firmware constructors:

```rust
pub struct SdStorage<D, TIME>
where
    D: BlockDevice,
    TIME: TimeSource,
{
    directory: crate::fat_directory::FatDirectory<D>,
    _time: core::marker::PhantomData<TIME>,
}
```

- [ ] **Step 4: Run the failing host tests**

Run:

```bash
HOME=/var/home/tristan RUSTUP_TOOLCHAIN=stable cargo test -p embedded-sd-storage --test fat_image -- --nocapture
```

Expected after Task 4: both `long_binbook_filename_roundtrips_as_real_directory_name` and `short_stem_binbook_is_not_rewritten_to_bbk` pass.

## Task 5: Verify Firmware Integration Against Existing Diagnostic Storage Tests

**Files:**
- Modify: `firmware/crates/binbook-fw/src/storage.rs` only if error type adaptation is needed.
- Test: `firmware/crates/binbook-fw/tests/diagnostic_storage.rs`

- [ ] **Step 1: Run feature-gated firmware storage tests**

Run:

```bash
HOME=/var/home/tristan RUSTUP_TOOLCHAIN=stable cargo test -p binbook-fw --features diagnostic-console,sd-storage --test diagnostic_storage -- --nocapture
```

Expected: storage upload/list/read/delete and CRC mismatch tests pass without `.BBK` assumptions.

- [ ] **Step 2: Run full feature-gated firmware tests**

Run:

```bash
HOME=/var/home/tristan RUSTUP_TOOLCHAIN=stable cargo test -p binbook-fw --features diagnostic-console,sd-storage
```

Expected: all tests pass.

- [ ] **Step 3: Run firmware target build**

Run:

```bash
cd firmware && HOME=/var/home/tristan RUSTC="$(HOME=/var/home/tristan rustup which --toolchain nightly rustc)" HOME=/var/home/tristan rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console,sd-storage --target riscv32imc-unknown-none-elf --release
```

Expected: release build succeeds. If it fails with dependency-level no-std/alloc issues, stop and execute Task 8 instead of working around with a heap.

## Task 6: Host CLI And Workspace Verification

**Files:**
- No code changes expected unless tests expose a regression.

- [ ] **Step 1: Run CLI protocol tests**

Run:

```bash
HOME=/var/home/tristan RUSTUP_TOOLCHAIN=stable cargo test -p binbook --features serial-device
```

Expected: all non-ignored tests pass.

- [ ] **Step 2: Run workspace tests**

Run:

```bash
HOME=/var/home/tristan RUSTUP_TOOLCHAIN=stable cargo test --workspace
```

Expected: all workspace tests pass.

- [ ] **Step 3: Run Python support tests**

Run:

```bash
HOME=/var/home/tristan RUSTUP_TOOLCHAIN=stable /home/linuxbrew/.linuxbrew/bin/uv run pytest -q
```

Expected: `70 passed, 26 skipped` or the current equivalent count with no failures.

## Task 7: Live Hardware Verification

**Files:**
- Modify: `HANDOFF.md`

- [ ] **Step 1: Flash diagnostic SD-storage firmware**

Run:

```bash
PORT="${PORT:-/dev/ttyACM0}" HOME=/var/home/tristan ESPFLASH_PORT="$PORT" FW_FEATURES="firmware-bin,diagnostic-console,sd-storage" firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Expected: `Flashing has completed!`.

- [ ] **Step 2: Capture 15 seconds of serial output**

Run:

```bash
HOME=/var/home/tristan /home/linuxbrew/.linuxbrew/bin/uv run --with pyserial --no-project python3 -c '
import serial, time, sys
ser = serial.Serial("/dev/ttyACM0", 115200, timeout=1)
ser.dtr = False; ser.rts = False; time.sleep(0.05)
ser.rts = True; time.sleep(0.05); ser.rts = False; time.sleep(0.1)
start = time.time()
while time.time() - start < 15:
    data = ser.read(ser.in_waiting or 1)
    if data:
        sys.stdout.buffer.write(data)
        sys.stdout.flush()
ser.close()
'
```

Expected: boot output showing the flashed app loaded.

- [ ] **Step 3: Run the diagnostic storage acceptance sequence**

Run:

```bash
HOME=/var/home/tristan RUSTUP_TOOLCHAIN=stable cargo build -p binbook --features serial-device
PORT=/dev/ttyACM0
OUT=/tmp/binbook-readback-booka-binbook
rm -f "$OUT"
target/debug/binbook diag hello --port "$PORT"
target/debug/binbook diag storage list --port "$PORT"
target/debug/binbook upload --port "$PORT" --name booka.binbook --file crates/binbook-storage/tests/fixtures/book_a.binbook
target/debug/binbook diag storage list --port "$PORT"
target/debug/binbook diag storage read --port "$PORT" --path booka.binbook --output "$OUT"
sha256sum crates/binbook-storage/tests/fixtures/book_a.binbook "$OUT"
target/debug/binbook diag storage delete --port "$PORT" --path booka.binbook
target/debug/binbook diag storage list --port "$PORT"
```

Expected:

```text
capabilities=KEY,PAGE,STATUS,LOG,CRASH,DISPLAY_PROBE,STORAGE,REFRESH_MODE
entry_count=0
Uploaded crates/binbook-storage/tests/fixtures/book_a.binbook (...) as booka.binbook
entry_count=1
  booka.binbook size=67876 pages=1
Read 67876 bytes from booka.binbook into /tmp/binbook-readback-booka-binbook
<same sha256>  crates/binbook-storage/tests/fixtures/book_a.binbook
<same sha256>  /tmp/binbook-readback-booka-binbook
ok
entry_count=0
```

- [ ] **Step 4: Update `HANDOFF.md`**

Replace the relevant current-state sections with:

```markdown
## Current Status

Diagnostic SD storage stores real `.binbook` long filenames on the FAT volume.
The previous `.BBK` short-name workaround has been removed.

## Live Hardware Evidence

<paste flash, serial, hello, upload/list/read/delete evidence>

## Remaining Risk

Long filename support is verified for the tested ASCII `.binbook` names. If
non-ASCII filenames are desired, add separate tests and hardware evidence.
```

## Task 8: Fallback If `fatfs` Is Not Firmware-Viable

**Files:**
- Revert spike-only `fatfs` dependency edits.
- Modify or vendor/patch `embedded-sdmmc` only if Task 3 or Task 5 proves `fatfs` cannot satisfy no-allocator firmware constraints.

- [ ] **Step 1: Record the failure mode**

In `HANDOFF.md`, record the exact failing command and error class, for example:

```text
fatfs fallback reason: firmware target build failed because fatfs LFN support requires alloc/global allocator.
```

- [ ] **Step 2: Remove the `fatfs` spike edits**

Remove `fatfs` from root `Cargo.toml` and `crates/embedded-sd-storage/Cargo.toml`. Delete `block_stream.rs` and `fat_directory.rs` if they are unused.

- [ ] **Step 3: Patch `embedded-sdmmc` through a local crate patch**

Create a local patched copy under `vendor/embedded-sdmmc-lfn` or a workspace crate only after confirming repo convention for vendored crates. Add `[patch.crates-io]` in root `Cargo.toml`.

Patch requirements:

- Add `open_file_lfn_in_dir(directory, name, mode, lfn_buffer)` or equivalent.
- Add LFN-aware lookup by matching `iterate_dir_lfn` names and short names.
- Add LFN create support that writes `ceil(utf16_name_len / 13)` LFN entries followed by the short entry.
- Add LFN delete support that marks both LFN entries and the short entry deleted.
- Keep caller-owned buffers; do not require a global allocator.

- [ ] **Step 4: Re-run Tasks 4 through 7**

The acceptance criteria are identical: real `.binbook` names on disk, no `.BBK` workaround, host tests, firmware tests, target build, and live hardware upload/list/read/delete evidence.

## Completion Matrix

Before declaring completion, fill this matrix in `HANDOFF.md`:

| Requirement | Implementation Path | Automated Test | Observed Evidence |
| --- | --- | --- | --- |
| Real `.binbook` long filenames are created on FAT | `embedded-sd-storage` FAT backend | `long_binbook_filename_roundtrips_as_real_directory_name` | Hardware upload/list shows `booka.binbook` |
| No `.BBK` workaround remains | Removed mapping functions | `short_stem_binbook_is_not_rewritten_to_bbk` | Hardware list shows `.binbook`, no `.BBK` |
| Upload commit remains CRC-verified | `binbook-fw` diagnostic storage | `diagnostic_storage` CRC tests | CLI upload reports CRC and readback hash matches |
| Readback is byte-identical | `StoreRead` paginated CLI path | `diagnostic_storage` roundtrip | `sha256sum` hashes match |
| Delete removes the file | FAT backend delete | `fat_image` delete test | Final hardware list is empty |
| Firmware has no unverified allocator regression | target build | firmware release build | `Finished release profile` |

## Final Hygiene

- [ ] Run `cargo fmt --all`.
- [ ] Run `git diff --check`.
- [ ] Grep `docs/ROADMAP.md` for `fatfs`, `embedded-sdmmc`, `binbook`, `upload`, `StoreList`, `.BBK`, and `LFN`; remove or rewrite stale completed entries if present.
- [ ] Confirm `HANDOFF.md` is not tracked: `git ls-files --error-unmatch HANDOFF.md` should fail.
- [ ] Final response must mention whether `fatfs` was adopted or `embedded-sdmmc` was patched, and must include the live hardware result.
