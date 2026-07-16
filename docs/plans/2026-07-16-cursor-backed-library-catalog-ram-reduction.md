# Cursor-Backed Library Catalog RAM Reduction Plan

## Status

Completed on 2026-07-16 on branch `wip/cursor-backed-library-catalog`.

The mounted production runtime uses a version-2 transactional A/B index,
bounded external-sort workspace, exact-slot cursor, and 15-entry viewport
without retaining `Library<200>`. The final flashed exact-feature image is
292,192 bytes and links 240,460 bytes of `.data + .bss`, a 35,272-byte
reduction from the 275,732-byte baseline.

All host, feature-gated, fault-feature, Clippy, Python, and target-build gates
pass. Live X4 evidence covers a 21-entry multi-viewport catalog, stable
filename-hash selection across insertion and reboot, inactive-slot retention
under injected publication failure, normal recovery, exact restoration of the
initial ten-file inventory, and direct user inspection of the rendered menu.

The rapid post-publication timeout discovered during acceptance is fixed with
a refresh barrier that remains pending through display-side cursor adoption
and host retries for transient list status. Five consecutive immediate uploads
completed without sleeps and were independently listed before cleanup.

## Committed resume state

Implementation checkpoint `8611b9d` and documentation checkpoint `1aec918`
precede the final acceptance fixes recorded by this completed plan.
## Goal and completion definition

Remove the full-capacity 200-entry library catalog from the production Xteink
X4 firmware without reducing the 200-book limit. Menu navigation must retain
only a 15-entry viewport and use a validated cursor into a transactional on-SD
index. Catalog refresh must use bounded RAM, preserve alphabetical ordering,
and never expose a partially rebuilt index.

This work is complete only when all of the following are true:

- no production firmware path allocates or retains `Library<200>`;
- the firmware does not add a heap allocator or use `alloc`, `Box`, or an
  allocator-backed collection;
- the 200-book limit and exact existing filename ordering are preserved;
- interrupted refresh, media removal, malformed indexes, CRC failure, and
  write failure leave the previous committed catalog authoritative;
- navigation, diagnostic listing, state pruning, and book opening all consume
  the cursor-backed index rather than reconstructing a full catalog;
- identical-feature target builds show at least a 24 KiB reduction in linked
  `.data + .bss` compared with the pre-change baseline;
- all host, feature-gated, target-build, live-device, serial, and webcam gates
  in this plan pass;
- `HANDOFF.md` contains the current evidence and `docs/ROADMAP.md` no longer
  lists this work as remaining.

## Cold-start context

Read these sources before editing:

1. `AGENTS.md` and the host-local `AGENTS.local.md`, if present.
2. `HANDOFF.md`, if present. It is host-local and must never be staged.
3. `docs/reference/rust-development-standards.md`.
4. `docs/reference/rust-crate-architecture.md`.
5. `docs/reference/xteink-x4-agent-device-verification.md`.
6. The `Cursor-backed library catalog RAM reduction` section of
   `docs/ROADMAP.md`.

The relevant production code is concentrated in:

- `firmware/crates/binbook-fw/src/library.rs` for entries, index codecs,
  cursors, and the viewport;
- `firmware/crates/binbook-fw/src/library_refresh.rs`,
  `library_index_save.rs`, and `library_index_load.rs` for correlated storage
  state machines;
- `firmware/crates/binbook-fw/src/storage_owner.rs` for scan, read, upload, and
  mounted-file ownership;
- `firmware/crates/binbook-fw/src/runtime/library_refresh_task.rs` and
  `runtime/mounted_display_task.rs` for refresh and menu integration;
- `firmware/crates/binbook-fw/src/runtime/diagnostic_owner_storage.rs` for
  paginated device listing;
- `crates/embedded-sd-storage/src/sd_filesystem.rs` for mounted FAT sessions
  and the `BINBOOK`, `BOOKS`, and `TMP` directories.

Current measured target layouts, obtained with pinned nightly and
`-Zprint-type-sizes`, are:

| Type | `riscv32imc-unknown-none-elf` size |
|---|---:|
| `Library` / `heapless::Vec<LibraryEntry, 200>` | 30,416 bytes |
| `LibraryViewport` | 2,304 bytes |
| `LibraryEntry` | 152 bytes |

The active mounted-storage runtime retains one 30,416-byte full refresh
catalog plus one 2,304-byte viewport. The other full `Library` declarations in
the source are in a mutually exclusive legacy runtime path; do not claim that
the selected mounted build currently retains two full catalogs. The roadmap's
older approximate 100 KiB statement must be replaced by final linked-image
measurements.

The current index is `BINBOOK.LIBRARY.INDEX`, with a 16-byte version-1 header,
up to 200 fixed 139-byte records, and one CRC over the complete index. The
current refresh scans all entries into `Library`, sorts them, then streams that
object through `LibraryIndexSaveClient`. The current viewport already retains
only 15 entries. Crossing a viewport boundary currently schedules another full
catalog refresh primarily to rebuild mounted-file prefetch tokens; this must be
split from true catalog rebuilding.

The firmware currently has no global allocator. Do not introduce one. Retain
the 2,304-byte viewport while reading so Back can return immediately to the
same menu state. Reclaiming that small viewport is outside this plan because it
would require a shared type-erased arena or a forced SD reload at every
book-to-menu transition. The large 30,416-byte catalog is the target.

## Fixed design decisions

### Version-2 index format

Use two files in the existing `BINBOOK` metadata directory:

- `BINBOOK.A.INDEX`
- `BINBOOK.B.INDEX`

Keep the version-1 `BINBOOK.LIBRARY.INDEX` as a read-only migration fallback.
Do not overwrite or delete it until a version-2 slot has been written, read
back, and strictly validated. Once at least one valid version-2 slot exists,
ignore the legacy file for selection. Deleting the legacy file is optional and
must never be needed for correctness.

Version-2 bytes are fixed as follows:

| Offset | Size | Field |
|---:|---:|---|
| 0 | 4 | ASCII `BBLI` |
| 4 | 2 | little-endian version `2` |
| 6 | 2 | little-endian header size `20` |
| 8 | 2 | little-endian record count, `0..=200` |
| 10 | 2 | reserved, all zero |
| 12 | 4 | nonzero little-endian generation |
| 16 | 4 | little-endian CRC32 |
| 20 | N | unchanged 139-byte index records |

The CRC is computed over the 20-byte header with bytes 16 through 19 treated
as zero, followed by every record in stored order. Generation zero is invalid.
Increment with `wrapping_add(1).max(1)`.

Select a slot only after validating its header, exact file length, every
record, ordering, duplicate-name rule, and final CRC. Compare generations with
wrapping arithmetic: candidate `b` is newer than `a` only when
`delta = b.wrapping_sub(a)` is nonzero and less than `0x8000_0000`. Equal or
exactly antipodal generations choose slot A deterministically. If only one
slot validates, select it. If neither validates, validate the version-1 file.

Keep the existing record encoding and comparator:

1. compare ASCII-casefolded filename bytes;
2. use raw filename bytes as the tie-breaker;
3. reject exact duplicate filenames;
4. retain the existing maximum UTF-8 filename length and `.binbook` suffix
   validation.

### Bounded external sort

Use `LIBRARY_VIEWPORT_CAPACITY`, currently 15, as the in-RAM sort capacity.
The maximum catalog therefore produces 14 sorted runs.

Use one reserved scratch file, `BBLIBRUN.TMP`, in the existing `TMP` directory.
The spool is never authoritative and must not appear in library scans or
diagnostic book listings. Truncate it at refresh start and delete it after
success or failure.

Each spool record is:

```text
u16 mount token, little endian; zero means no token
139-byte version-independent library index record
```

The 141-byte spool record preserves the transient mounted-file token through
sorting without adding it to the persistent index format.

Refresh algorithm:

1. Start the existing bounded mounted-directory scan and configure at most 15
   exact prefetch names.
2. Decode scan records one at a time from the existing scan buffer. Do not
   materialize a second eight-entry decoded vector across an `await`.
3. Accumulate at most 15 `LibraryEntry` values. When full, sort them with the
   canonical comparator and append one run to the spool. Record only
   `{start_record: u16, len: u8}` for that run.
4. Reject entry 201 as `TooManyBooks`, finish or cancel the owner scan cleanly,
   delete the spool, and do not touch either committed slot.
5. After the final run is written and the owner scan is finished, clear and
   reuse the same 15-entry vector for run heads. Do not allocate separate
   batch and merge-head arrays.
6. Choose the inactive A/B slot relative to the newest valid committed slot.
   If neither slot is valid, target A with generation 1.
7. Begin an upload of exactly `20 + count * 139` bytes. Write a provisional
   version-2 header with CRC zero.
8. Perform a one-pass k-way merge of at most 14 run heads. Use linear minimum
   selection; 14 comparisons per output record are acceptable and avoid a
   second heap structure. Detect out-of-order and duplicate records during
   merge.
9. Stream persistent records in bounded chunks through
   `BufferSlot::LibraryIndex`, update CRC as records are emitted, then
   backpatch the final header at offset zero.
10. Flush and commit the candidate, read the complete candidate back through
    bounded reads, and run the same strict validator used at boot.
11. Only after readback validation succeeds may the completion identify the
    new slot as authoritative.

The production refresh workspace must be no larger than 3 KiB on the firmware
target. Inspect the generated async task size and linked symbols so the
15-entry vector is not copied into multiple future states.

Do not silently skip an overlength or unrepresentable `.binbook` filename.
Extend `MountedScanBatch`, `MountedFileScanBatch`, `ScanProgress`, and their
encoders/state machines with a sticky `unsupported_name` outcome. A refresh
that observes it must finish/cancel the directory owner cleanly, return
`LibraryStatus::UnsupportedName`, and retain the old committed index. Files
that do not end in `.binbook` remain intentionally ignored.

### Internal scratch requests

Do not expose arbitrary temporary-file operations. Extend the internal
`storage_owner::Request` state machine with fixed-purpose requests usable only
by `StorageClient::Library`:

- `LibraryScratchCreate`
- `LibraryScratchWrite { offset, buffer_offset, len }`
- `LibraryScratchRead { offset, len, buffer }`
- `LibraryScratchFlush`
- `LibraryScratchDelete`

Use existing `Ack`, `Written`, and `Read` responses and existing correlation
rules. Reject the request if the client, phase, buffer slot, offset, or length
is wrong. The mounted backend must hold at most one scratch write or read
session and must drop it before deleting the file.

Do not enlarge the persistent metadata-read cache merely to hold both index
slots, both reader-state slots, and the legacy index. Keep it bounded and add
deterministic eviction so exact requested metadata files can be opened one at
a time. A/B validation is sequential; all five files need not be open
simultaneously.

### Cursor and publication types

Add these concepts; exact Rust field visibility may remain crate-private:

```rust
enum LibraryIndexSlot { A, B, Legacy }

struct LibraryIndexCursor {
    slot: LibraryIndexSlot,
    generation: u32, // zero only for the validated legacy cursor
    entry_count: u16,
    header_size: u16,
}

struct LibraryRefreshWorkspace {
    entries_or_heads: heapless::Vec<LibraryEntry, 15>,
    runs: heapless::Vec<RunDescriptor, 14>,
    // scalar scan, merge, count, CRC, and generation state only
}

struct PreparedMount {
    absolute_index: u16,
    token: MountFileToken,
}

struct LibraryPublication {
    status: LibraryStatus,
    cursor: Option<LibraryIndexCursor>,
    book_count: u16,
    prepared: heapless::Vec<PreparedMount, 15>,
}
```

`LibraryIndexCursor::record_range` must use its own header size and the
unchanged 139-byte record size. Window loads must always read the exact slot
named by the cursor; they must not re-resolve "latest" midway through a menu
session.

During merge, convert any nonzero spool mount token into a compact
`PreparedMount` using the emitted absolute index. This is collision-safe and
avoids retaining a second vector of full filenames after publication.

### Refresh versus prefetch

Introduce distinct runtime work kinds:

- `CatalogRefresh`: boot reconciliation, successful upload, successful delete,
  media change, and explicit diagnostic refresh. It scans, sorts, and publishes
  a new A/B index.
- `ViewportPrefetch`: viewport boundary movement only. It may perform a bounded
  owner scan to prepare exact current-window file sessions, but it discards
  catalog records and never writes the spool or an index slot.

A prefetch request contains `(absolute_index, exact StorageName)` pairs for the
current viewport. Its completion contains compact `PreparedMount` values.
Opening a book must always fall back to an exact name-based open when no valid
mount token is ready, so rapid Select after a viewport move remains correct.

The refresh task must yield between scan and write batches. Reader input,
display work, state durability barriers, and foreground book reads keep their
existing storage-owner priority. A new interaction may postpone prefetch, but
must not corrupt or silently cancel a catalog publication already past its
durability boundary.

## Implementation tasks

### Task 0: Protect the worktree and capture a real baseline

- [x] Run `git status --short`, record the current commit, and identify all
  pre-existing modified and untracked files. Do not reset, clean, switch
  branches, or overwrite unrelated changes.
- [x] Build the current target with the exact final comparison features:

  ```bash
  cd firmware
  RUSTC="$(rustup which --toolchain nightly rustc)" \
    rustup run nightly cargo build -p binbook-fw \
    --features firmware-bin,diagnostic-console,sd-storage,usb-rx-isr-buffer \
    --target riscv32imc-unknown-none-elf --release
  ```

- [x] Record `readelf -SW` `.data` and `.bss` values, application size printed
  by the flash/build tooling, and the exact feature set in `HANDOFF.md`.
- [x] Re-run the target type-size measurement and record `Library`,
  `LibraryViewport`, `LibraryEntry`, and relevant task future sizes. Use
  `CARGO_PROFILE_RELEASE_STRIP=false` or `-Zprint-type-sizes` without editing a
  tracked Cargo profile.
- [x] Run the existing focused library, refresh, load, save, storage-owner, and
  embedded-storage tests before changing behavior. Record any pre-existing
  failures separately.

**Gate:** baseline source state, feature set, target RAM sections, type sizes,
and focused test results are recorded before implementation.

### Task 1: Add version-2 codec and slot selection test-first

- [x] Add failing exact-byte tests for the 20-byte header, generation, CRC,
  record offsets, maximum length, and zero-record index.
- [x] Add failing tests for A only, B only, both valid, equal generation,
  wrapping generation, antipodal generation, corrupt newer slot, corrupt older
  slot, and both corrupt.
- [x] Add a failing test proving a valid version-1 index is selected only when
  neither version-2 slot validates.
- [x] Implement version-aware headers, `LibraryIndexSlot`, expanded cursor,
  strict streamed validation, and deterministic slot selection.
- [x] Keep record encoding byte-identical. Update cursor, viewport, prune, and
  diagnostic consumers to accept a version-aware header size.

Run:

```bash
cargo test -p binbook-fw --features sd-storage --test library
cargo test -p binbook-fw --features sd-storage --test library_index_load
```

**Gate:** every slot-selection and migration test passes, including a corrupt
higher-generation candidate falling back to the older valid slot.

### Task 2: Add bounded scratch storage operations test-first

- [x] Add fake-owner tests proving the five fixed scratch requests enforce
  client, sequence, buffer, bounds, and lifecycle rules.
- [x] Add mounted FAT-image tests with a spool larger than the scan buffer;
  write it in nontrivial chunks, flush, read it at multiple offsets, and delete
  it.
- [x] Add injected failures at create, write, flush, read, and delete. A failed
  operation must return its real category and leave the storage owner usable.
- [x] Implement the fixed `BBLIBRUN.TMP` session in the mounted backend without
  adding generic path access or a second large buffer.
- [x] Refactor metadata session caching so A, B, legacy, state A, and state B
  work through the existing bounded cache with deterministic eviction.

Run:

```bash
cargo test -p embedded-sd-storage
cargo test -p binbook-fw --features sd-storage --test storage_owner
```

**Gate:** the scratch path streams data larger than every involved buffer,
deletes cleanly, and does not increase the metadata cache capacity.

### Task 3: Implement bounded run generation and merge test-first

- [x] Replace tests that merely prove a 200-entry `Library` can be built with
  tests that drive 200 correlated scan records through the bounded refresh
  state machine.
- [x] Add unsorted fixtures for counts `0`, `1`, `15`, `16`, `20`, `21`, and
  `200`. Force names that sort across scan-batch and 15-entry run boundaries.
- [x] Add a 201-entry fixture that proves `TooManyBooks`, scratch cleanup, and
  no candidate-slot write.
- [x] Add maximum-length UTF-8 names, unsupported names, exact duplicates,
  casefold ties, and fully reversed input.
- [x] Add a reference-model differential test: sort in an ordinary host vector,
  run the streaming refresh, and compare every final persistent byte.
- [x] Implement one-record-at-a-time scan decoding, 15-entry run generation,
  141-byte spool records, run descriptors, vector reuse, and the one-pass
  k-way merge.
- [x] Propagate unsupported `.binbook` names explicitly from the mounted FAT
  iterator through `ScanProgress`; do not convert them into a successful scan
  with fewer entries.
- [x] Compute the persistent CRC during merge, backpatch the header, and reject
  any output that is not strictly increasing under the canonical comparator.
- [x] Collect prepared mount tokens by final absolute record index.

Run:

```bash
cargo test -p binbook-fw --features sd-storage --test library_refresh
cargo test -p binbook-fw --features sd-storage --test library_index_save
```

**Gate:** the 200-entry result is byte-identical to the reference model, the
workspace never exceeds 15 entries, and entry 201 cannot modify a committed
slot.

### Task 4: Make A/B publication transactional under every failure

- [x] Add fault injection at scan start, each run write, scratch flush, merge
  read, inactive-slot begin, provisional header, first/middle/final record,
  header backpatch, commit, readback, validation, and cleanup.
- [x] Seed a nonempty valid old slot before every fault. After the failure,
  independently run slot selection and assert the old slot, generation, count,
  and records are unchanged and authoritative.
- [x] Add media-removal tests during scan, scratch write, candidate write, and
  readback.
- [x] Add reboot tests with a truncated inactive slot, a CRC-valid stale slot,
  a CRC-invalid newer slot, and a complete newer slot.
- [x] Implement candidate abort and cleanup without clearing the active
  viewport or falsely returning `Ready`.
- [x] Read back and strictly validate the complete candidate before emitting
  `LibraryPublication`.

Run the focused refresh, load, storage-owner, and embedded FAT-image tests.

**Gate:** every injected failure retains the old catalog; only a fully written,
flushed, read-back, and validated slot becomes authoritative.

### Task 5: Integrate cursor navigation and selection preservation

- [x] Add failing menu/runtime tests for forward and backward movement across
  viewport boundaries with totals `20`, `21`, and `200`.
- [x] Prove a window read uses the cursor's exact slot even when the other slot
  gains a newer generation afterward.
- [x] Add selection tests where entries are inserted before and after the
  selected book, the selected book moves to another viewport, and the selected
  book is removed. Preserve by exact filename hash when present; otherwise
  clamp the old ordinal to `min(old_index, new_total - 1)`.
- [x] Replace the mounted runtime's `StaticCell<Library>` refresh argument with
  one bounded `LibraryRefreshWorkspace`.
- [x] Load both slots at boot, retain only the selected 15-record viewport, and
  pass the selected cursor through window loads and state pruning.
- [x] Split viewport prefetch from catalog refresh. Remove the full refresh
  call from ordinary viewport crossing.
- [x] Preserve name-based book open as the correctness fallback when no token
  is prepared.
- [x] Ensure failed refresh and media removal leave the current in-RAM viewport
  usable until a durability or media boundary makes that impossible.

Run all `binbook-fw` tests with `sd-storage` after integration.

**Gate:** navigation reaches every record in both directions, selection is
stable across publication, and ordinary viewport movement writes no index
file.

### Task 6: Update diagnostics and add discriminating evidence

- [x] Update mounted diagnostic `storage list` to select A/B with the shared
  validator, paginate records through bounded reads, and fall back to version
  1 only under the migration rule. It must never instantiate `Library`.
- [x] Add feature-gated runtime events for:
  - selected index slot, generation, and total count;
  - loaded viewport start, count, and total;
  - selected menu ordinal and filename hash;
  - refresh published versus old-index-retained, with the exact failure phase.
- [x] Map those events into existing diagnostic logs with tests for exact
  fields. Do not add unconditional debug printing.
- [x] Add Cargo feature `library-index-fault-after-first-record`. Require
  `diagnostic-console` and `sd-storage` with `compile_error!` otherwise. In that
  build, every catalog refresh must fail immediately after the first persistent
  record write to the inactive slot. Normal builds must compile out the hook.
- [x] Add tests proving the diagnostic fault damages only the inactive slot and
  that list, viewport load, and reboot continue selecting the old slot.

Run:

```bash
cargo test -p binbook-fw --features diagnostic-console,sd-storage
cargo test -p binbook-fw \
  --features diagnostic-console,sd-storage,library-index-fault-after-first-record
cargo test -p binbook-diagnostic-protocol
cargo test -p binbook --features serial-device
```

**Gate:** diagnostics distinguish a published refresh from a retained old
index, list all 200 entries with bounded RAM, and the fault build cannot be
mistaken for a successful refresh.

### Task 7: Remove full catalogs and prove the RAM result

- [x] Grep every production and feature-gated path for `Library`,
  `Library::empty`, `LibraryIndexDecoder`, `LibraryIndexStore`,
  `MAX_LIBRARY_ENTRIES`, and `StaticCell`. Classify every remaining use.
- [x] Remove the active refresh allocation and the unreachable legacy
  full-catalog runtime. A full reference model may remain under tests only.
- [x] Remove obsolete load/save/refresh APIs only after all menu, reader,
  diagnostic, state-prune, boot, upload, delete, and refresh callers use the
  cursor path.
- [x] Add a host size assertion for `LibraryRefreshWorkspace` and rerun target
  `-Zprint-type-sizes`. Inspect async future sizes for hidden by-value copies.
- [x] Build the exact baseline feature set and record final `.data`, `.bss`,
  application size, workspace size, and delta.
- [x] Require a reduction of at least 24 KiB in `.data + .bss`. If the target is
  missed, inspect symbols and future layouts; do not lower the target or claim
  success without identifying the retained allocation.

**Gate:** source inspection, type-size evidence, and linked-image evidence all
show that no production full catalog remains and the RAM target is met.

### Task 8: Run final host and target gates

Run from the repository root unless the command explicitly changes directory:

```bash
cargo clean
cargo fmt --all -- --check
cargo test -p embedded-sd-storage
cargo test -p binbook-fw --features sd-storage
cargo test -p binbook-fw --features diagnostic-console,sd-storage
cargo test -p binbook-fw \
  --features diagnostic-console,sd-storage,library-index-fault-after-first-record
cargo clippy -p binbook-fw --all-targets \
  --features diagnostic-console,sd-storage -- -D warnings
cargo test --workspace
uv run pytest -q
git diff --check

cd firmware
RUSTC="$(rustup which --toolchain nightly rustc)" \
  rustup run nightly cargo build -p binbook-fw \
  --features firmware-bin,sd-storage \
  --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" \
  rustup run nightly cargo build -p binbook-fw \
  --features firmware-bin,diagnostic-console,sd-storage,usb-rx-isr-buffer \
  --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" \
  rustup run nightly cargo build -p binbook-fw \
  --features firmware-bin,diagnostic-console,sd-storage,usb-rx-isr-buffer,library-index-fault-after-first-record \
  --target riscv32imc-unknown-none-elf --release
```

After the first `cargo clean`, do not accept a result produced only by an older
artifact. Record pre-existing failures separately and fix every regression
caused by this work before continuing.

**Gate:** all required host suites, feature-gated suites, Clippy, formatting,
Python compatibility tests, and three firmware target builds pass.

### Task 9: Verify on a live Xteink X4

Hardware verification is mandatory. Follow
`docs/reference/xteink-x4-agent-device-verification.md` sequentially. Use one
owner of `$PORT` at a time. Record exact commands and complete relevant output
in `HANDOFF.md` as soon as it is captured.

Set portable variables:

```bash
PORT="${PORT:?set the live X4 serial port}"
OUT="${OUT:-target/diag-evidence/library-catalog-ram}"
BIN="${BIN:-target/release/binbook}"
FIXTURE="${FIXTURE:-crates/binbook-storage/tests/fixtures/book_a.binbook}"
mkdir -p "$OUT"
```

#### 9.1 Establish the starting state

- [x] Build the release CLI with `serial-device` and flash the normal image:

  ```bash
  cargo build --release -p binbook --features serial-device
  ESPFLASH_PORT="$PORT" \
    FW_FEATURES="firmware-bin,diagnostic-console,sd-storage,usb-rx-isr-buffer" \
    firmware/scripts/flash-xteink-x4-nav-probe.sh
  ```

- [x] Capture serial output for at least 15 seconds using the command in
  `AGENTS.md`, saving it under `$OUT`. Wait for capture to exit. Use this exact
  portable form for every required 15-second capture:

  ```bash
  SERIAL_LOG="${SERIAL_LOG:-$OUT/normal-boot.log}"
  PORT="$PORT" uv run --with pyserial --no-project python3 -c '
  import os, serial, sys, time
  ser = serial.Serial(os.environ["PORT"], 115200, timeout=1)
  ser.dtr = False; ser.rts = False; time.sleep(0.05)
  ser.rts = True; time.sleep(0.05); ser.rts = False; time.sleep(0.1)
  start = time.time()
  while time.time() - start < 15:
      data = ser.read(ser.in_waiting or 1)
      if data:
          sys.stdout.buffer.write(data); sys.stdout.flush()
  ser.close()
  ' | tee "$SERIAL_LOG"
  ```

  Set `$SERIAL_LOG` to a new path under `$OUT` before each later capture; never
  overwrite an earlier boot or fault record.

- [x] Run independent HELLO, STATUS, LOG, and storage-list queries. Record the
  complete initial library inventory, slot, generation, entry count, selected
  ordinal/hash, protocol-error count, and last error.

  ```bash
  "$BIN" diag hello --port "$PORT"
  "$BIN" diag status --port "$PORT"
  "$BIN" diag logs --port "$PORT" --since 0
  "$BIN" diag storage list --port "$PORT"
  ```

- [x] Send Back if necessary and use logs plus a fresh webcam capture to prove
  the library, rather than an open book, is visible.

  ```bash
  "$BIN" diag key --port "$PORT" BACK
  VIDEO_DEVICE="${VIDEO_DEVICE:?set the live webcam device}" \
    ffmpeg -hide_banner -loglevel error -f video4linux2 \
    -i "$VIDEO_DEVICE" -frames:v 1 "$OUT/library-start.jpg"
  ```

#### 9.2 Create and navigate a multi-viewport catalog

- [x] Upload uniquely named copies of `$FIXTURE` until the total catalog has at
  least 21 entries. Use a recorded disposable prefix and never overwrite an
  initial filename:

  ```bash
  "$BIN" diag storage upload \
    --port "$PORT" \
    --file "$FIXTURE" \
    --name "$DISPOSABLE_NAME"
  ```

- [x] After the final upload, independently list until the refresh settles.
  Require the exact expected name/size set, at least 21 entries, a newer valid
  generation, zero protocol errors, and no unexplained last error.
- [x] Drive Up until logs independently report selected ordinal 0. Then drive
  Down one event at a time past ordinal 15 and at least one further boundary.
  Drive back across both boundaries. Require `LibraryViewportLoaded` events
  with nonzero starts and exact windows; queued key acknowledgements alone do
  not count.

  ```bash
  "$BIN" diag key --port "$PORT" UP
  "$BIN" diag key --port "$PORT" DOWN
  "$BIN" diag logs --port "$PORT" --since "$LAST_VERIFIED_CURSOR"
  ```

  Repeat one key command at a time, advancing `$LAST_VERIFIED_CURSOR` from each
  decoded log response. Do not batch keys when establishing viewport evidence.
- [x] Capture the panel after crossing a boundary and show the user the fresh
  capture path. Inspect that exact webcam image for correct names, selection,
  orientation, clipping, stale rows, and unexpected full refreshes.

  ```bash
  VIDEO_DEVICE="${VIDEO_DEVICE:?set the live webcam device}" \
    ffmpeg -hide_banner -loglevel error -f video4linux2 \
    -i "$VIDEO_DEVICE" -frames:v 1 "$OUT/library-second-viewport.jpg"
  ```
- [x] Select a disposable book whose sorted ordinal is at least 15. Verify the
  filename hash in logs, page count in STATUS, and visible page. Return to the
  library and prove the same selection is restored.

#### 9.3 Verify publication and reboot recovery

- [x] While a nonzero selected ordinal is active, upload one name that sorts
  before it and one that sorts after it. After each refresh, independently
  prove the selected filename hash is unchanged even though its ordinal may
  move.
- [x] Reset/reboot the device, capture at least 15 seconds of serial output,
  then query HELLO, STATUS, LOG, and storage list. Require the same newest
  generation, exact catalog, valid viewport, and preserved selection.

#### 9.4 Verify an interrupted inactive slot

- [x] Record the authoritative slot, generation, count, and complete list.
- [x] Flash the fault image:

  ```bash
  ESPFLASH_PORT="$PORT" \
    FW_FEATURES="firmware-bin,diagnostic-console,sd-storage,usb-rx-isr-buffer,library-index-fault-after-first-record" \
    firmware/scripts/flash-xteink-x4-nav-probe.sh
  ```

- [x] Capture boot serial for at least 15 seconds. The boot refresh must report
  the injected post-first-record failure, and independent listing must still
  return the previously authoritative slot, generation, count, and exact
  records.
- [x] Upload one additional disposable fixture with `--no-wait-library`. The
  book upload must commit, the subsequent catalog refresh must hit the same
  injected failure, and an independent list must not expose the new file.
- [x] Reboot while still running the fault image. Re-query and prove the old
  catalog remains authoritative after reboot; a transport acknowledgement or
  cached host list is insufficient.
- [x] Restore the normal image, capture boot serial, and wait for normal boot
  reconciliation. Independently prove a newer valid generation now includes
  the uploaded file.

#### 9.5 Restore the device

- [x] Delete only names bearing the recorded disposable prefix. Quote every
  exact filename; do not use whitespace-splitting `xargs`.
- [x] Wait for the resulting refresh, then require the complete final list to
  equal the captured initial list exactly.
- [x] Leave the normal diagnostic image flashed. Reconfirm HELLO, STATUS, zero
  protocol errors, acceptable last error, the final slot/generation/count, and
  the restored visible library with a fresh webcam capture.

Record activation latency and whether any unexpected full refresh, clipping,
stale menu row, ghosting, or delayed input occurred. This change should not
alter display refresh policy, but live observation is required because menu
navigation is part of the acceptance surface.

**Gate:** the live device navigates more than one viewport, preserves selection,
recovers after reboot, retains the old A/B catalog under an injected interrupted
publication, adopts the new catalog only after normal verified publication,
and is restored to its exact starting library.

### Task 10: Documentation, adversarial review, and completion matrix

- [x] Add a current storage-format specification under `docs/specs/` describing
  version 2, A/B selection, migration, CRC, generation ordering, and the
  non-authoritative spool. Do not put this internal index in
  `BINBOOK_FORMAT_SPEC.md`; it is not part of `.binbook`.
- [x] Update applicable reference documentation to describe cursor-backed
  navigation, bounded refresh, and the absence of a production full catalog.
- [x] Grep all docs and code for `BINBOOK.LIBRARY.INDEX`, `Library<200>`,
  `100 KiB`, `full-capacity catalog`, and related stale claims. Correct them in
  the same change.
- [x] Remove the completed catalog-RAM section from `docs/ROADMAP.md`; do not
  mark it done in the roadmap.
- [x] Rewrite the relevant `HANDOFF.md` sections as a current-state snapshot.
  Distinguish host verification, linked-RAM evidence, successful live behavior,
  injected-failure evidence, visual evidence, and any unresolved issue.
- [x] Perform an adversarial review after documentation is written:
  - attempt to load the partially written newer slot;
  - attempt to make viewport reads switch slots mid-session;
  - attempt to trigger a full index rewrite through ordinary navigation;
  - attempt to hide a 200-entry array in an async future;
  - attempt to expose a committed upload after refresh failure;
  - attempt to disprove the RAM delta with linked symbols and section sizes.
- [x] Populate the acceptance matrix below with exact test names, commands,
  outputs, artifact references, and observations. A blank cell means the work
  is incomplete.

## Required acceptance matrix

| Requirement | Implementation path | Automated evidence | Live/observed evidence |
|---|---|---|---|
| No production `Library<200>` | mounted runtime uses `LibraryRefreshWorkspace` and `LibraryViewport` | source/static-symbol audit; target links | `.data + .bss` is 240,460 bytes; live boot succeeds |
| At least 24 KiB RAM reduction | full refresh catalog static removed | identical features: baseline 275,732, candidate 240,460 bytes | 35,272-byte reduction; 292,192-byte normal application flashed |
| Supports 0 through 200 books | v2 codec, spool, merge | boundary tests for 0/1/15/16/20/21/200 | live SD catalog grew from 10 to 25 entries and returned exactly to 10 |
| Navigation across viewports | exact-slot cursor window reads | forward/backward tests for 20/21/200 | typed events traversed ordinals 0 through 20 and cache windows in both directions |
| Stable selection | filename-hash restoration | insert-before/after and removal/clamp tests | selected hash survived insert-before, insert-after, book open/close, and reboot |
| Transactional refresh | A/B slots and full readback validation | publication boundary-failure sweep and fault-feature tests | phase-5 fault retained slot 2 generation 22 before and after reboot; normal firmware later adopted the committed book |
| Legacy compatibility | version-1 fallback | A/B selector migration tests | normal live A/B selection and publication verified; migration remains host-tested |
| Diagnostic listing is bounded | sequential slot validator and cursor paging | 200-entry cursor/list component coverage | complete 25-entry live list returned without reconstructing a full catalog |
| Viewport movement does not rebuild | exact cursor window load | navigation tests prove exact windows | generation stayed 17 during ordinary traversal; only publications advanced it |
| No allocator introduced | heapless/caller-owned storage | dependency/source audit and target builds | normal no-heap image boots |
| Device restored | deletion is restricted to the recorded disposable prefix | exact final-list comparison | final list exactly matched the initial ten names and sizes; normal image and zero-error STATUS restored |
| Visual menu correctness | unchanged bounded partial-refresh menu path | framebuffer/menu rendering tests | user directly verified readable upright text, one correct highlight, no missing/duplicate/stale rows, and responsive partial navigation; the long title is width-truncated |
| Rapid publication synchronization | refresh barrier remains pending through display cursor adoption; host retries transient list status | `catalog_stays_pending_until_display_adoption_settles_it`; `library_wait_retries_transient_error_responses_until_catalog_settles` | original immediate second-upload timeout reproduced; five consecutive immediate uploads then completed with `library_visible=true`, exact list, and zero-error STATUS |

Every acceptance row has applicable automated and live evidence. The final
adversarial review did not disprove the completion claims, and the completed
catalog qualification item has been removed from `docs/ROADMAP.md`.
