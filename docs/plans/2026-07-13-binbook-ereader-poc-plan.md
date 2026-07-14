# BinBook Xteink X4 E-Reader POC Implementation Plan

> **For the implementing agent:** Execute this plan sequentially in the current
> branch/worktree. Do not delegate to subagents. Use a todo tracker and keep it
> current. Every task begins with a failing test, ends with its focused tests
> passing, and must leave no known regression behind. Do not stage, commit, or
> change branches unless the user explicitly asks.

**Goal:** Deliver a dependable proof-of-concept e-reader firmware on the Xteink
X4: list BinBooks on the SD card, select and open the actual selected file, turn
pages, return to the library, resume every book at its saved page, enter deep
sleep with a two-second POWER hold, and wake to the last visible screen.

**Why this plan replaces the older plan:**
[`2026-07-01-library-menu-reading-flow-plan.md`](2026-07-01-library-menu-reading-flow-plan.md)
describes the initial menu implementation. Parts of it now exist, but its
remaining assumptions are wrong for this POC: normal firmware must not expose
the embedded probe as a fake library book, progress is per-book rather than
last-book-only, state belongs on the SD card rather than internal flash, and the
current menu still does not open the selected SD file. Treat that plan as
historical context after this replacement is adopted.

**Primary authoritative references:**

- `AGENTS.md` and optional `AGENTS.local.md`
- `BINBOOK_FORMAT_SPEC.md`
- `docs/reference/rust-development-standards.md`
- `docs/reference/squidscript-and-xteink-reference.md`
- `docs/reference/xteink-x4-agent-device-verification.md`
- `docs/specs/2026-07-01-library-menu-reading-flow-design.md`, updated by the
  final documentation task to reflect the decisions in this plan

**Tech stack:** Rust `no_std`, `binbook-core`, `binbook-storage`,
`embedded-sd-storage`, `xteink-x4-display`, `ssd1677-driver`, Embassy,
`esp-hal` 1.1.x, `embedded-hal` 1.0, `heapless`, FAT via `lamfat`, host Cargo
tests, pinned nightly RISC-V firmware builds, diagnostic USB, and live X4
webcam/serial verification.

## Execution Status (2026-07-14)

Tasks 0 through 8A are implemented. Task 9's physical button/power matrix and
adversarial hardware checks are complete. The original task recipes remain
below so a cold agent can audit the intended TDD sequence; they are not work to
repeat. Use the current-state checkpoint in section 2 and the host-local
`HANDOFF.md` acceptance matrix as the implementation/evidence authority.

The exact staged-persistence image physically verified all six ADC buttons,
short POWER, one explicit double-tap cleanup, hold/release gating, real
ESP32-C3 deep sleep, GPIO3 wake, nonzero reading-page restoration, and
non-default menu-selection restoration. A subsequent adversarial audit found
that the mounted runtime falsely rejected routed refresh-mode/probe actions.
The corrected 253,168-byte image executes Balanced mode changes and all six
advertised probes; standard-build Fast remains intentionally feature-gated.
Live request/response/log/state/webcam evidence covers the corrected branches.

During that session, physical Left/Right exposed and then verified a repaired
state-publication bug: the mounted path now emits `PageDisplayed` for successful
unsequenced turns, so the visible page, committed reader state, and diagnostic
STATUS remain synchronized. The regression test is
`successful_physical_turn_publishes_the_committed_page_without_a_protocol_sequence`.

Live evidence also covers the production diagnostic storage owner,
background indexing, exact selected-book opening, page boundaries, Back,
independent resume, same-name replacement, deletion pruning, corrupt-book
recovery, and recovery from a deliberately torn newest reader-state slot.

Physical SD removal is not a completion gate. The X4 exposes SPI SD pins but no
card-detect input, so removal does not generate a distinct hardware signal; it
only causes some storage transaction to fail. The valid contract is tested at
the error boundary: `CardNotFound` maps to `NoCard`, other device/FAT failures
map to `CardError`, failed scans never prune progress or replace the last valid
index, and Select retries initialization. Live inserted-card operation is
covered separately. Hot removal while reading is out of scope; a removal run is
optional board qualification and cannot by itself prove all storage failures.

---

## 1. Locked Product Behavior

Do not reopen these choices during implementation unless repository evidence
proves them infeasible and the user approves a change.

### Library

- Normal firmware lists `.binbook` files in `/BOOKS` from the SD card.
- Sort filenames with ASCII case folding (`A`-`Z` compare as `a`-`z`); leave
  every non-ASCII UTF-8 byte unchanged. Break equal-folded names by their
  original UTF-8 bytes so ordering is deterministic without adding Unicode
  case-folding tables to firmware.
- Rows show filenames only. Metadata titles/authors and covers are deferred UX
  work.
- Support at most 200 entries. If entry 201 is observed, do not show a partial
  library as though it were complete; render `Too many books (max 200)`.
- Store names up to `binbook_storage::MAX_BINBOOK_NAME_BYTES` (currently 121
  UTF-8 bytes). Visually truncate with an ellipsis, but retain the complete name
  for open/read operations. If a longer `.binbook` name is observed, render an
  explicit unsupported-name error rather than silently omitting it.
- Only enumerate by extension during the library scan. Opening every large book
  at boot is already known to be too slow on the current SD path. Validate the
  selected file with `binbook_core::Book::open` when the user opens it.
- Normal firmware has explicit `No SD card`, `Card error`, and `No books`
  screens. The embedded `nav_probe.binbook` remains available only to
  diagnostic/test builds and is never inserted into the normal library.
- The menu remains a five-row viewport with wrap-around navigation. Returning
  from reading restores the same selected book and viewport.

### Reading

- Select opens the exact selected SD filename and renders that book's actual
  page 0 or saved page.
- Left/Up go to the previous page; Right/Down go to the next page. First and
  last page boundaries clamp; reading does not wrap.
- Back or Select closes the book, updates progress in RAM, and returns to the
  library without an SD write in the transition path.
- A page becomes current only after its render succeeds. A failed render must
  not advance saved progress.
- On open failure, show `Cannot open book` and the filename. Back returns to the
  library; Select retries after a rescan.
- On a read failure after a book is open, preserve the last successfully
  committed page. Back returns to a rescanned library; Select retries that page.

### Progress and state

- Remember an independent page for every book, up to the same 200-book limit.
- Mark state dirty after successful selection, open, page, and close changes.
  Save after two seconds without interaction; new input postpones the save.
  Retry a failed idle save after five seconds without discarding dirty RAM.
  Save and verify synchronously immediately before sleep.
- State is card-local in two binary files inside `/BOOKS`:
  `BINBOOK.A.STATE` and `BINBOOK.B.STATE`.
- A successful scan removes progress records for filenames no longer present.
  A missing/unreadable card or failed enumeration must never garbage-collect
  state.
- Match progress by both filename hash and BinBook rendition hash. Replacing a
  file under the same filename with different content starts at page 0.
  Renaming is intentionally treated as delete plus add for this POC.
- Sleep/wake restores the last screen. Sleeping while reading wakes directly to
  the saved book/page. Sleeping in the menu wakes to the menu with the same
  selection. If the saved book is missing or replaced, wake to the library.
- If an idle save fails, retain the new state in RAM and retry after five
  seconds. Unexpected power loss can lose only changes newer than the last
  committed A/B generation.
- If saving before sleep fails, cancel sleep, keep the device awake, and show
  `Cannot save; sleep cancelled`.

### Power

- A continuous two-second active-low hold on POWER/GPIO3 requests sleep.
- A single short POWER press does nothing. Two short POWER taps whose releases
  are at most 500 ms apart request an explicit user full refresh. Holding the
  second press continuously for two seconds requests sleep rather than refresh.
  No automatic inactivity sleep is included.
- Finish the current non-cancellable display activation before shutdown. Stop
  accepting new navigation, persist state, flush storage, put the SSD1677 into
  deep sleep, then wait for POWER to be released before entering ESP32-C3 deep
  sleep. Release gating prevents the initiating hold from causing an immediate
  wake.
- Wake source is the next active-low POWER press on GPIO3. Wake is reset-like:
  remount, rescan, reinitialize the panel, and restore the saved screen.

### Refresh policy

- Routine menu navigation, menu-to-book, book-to-menu, and page turns must not
  use a full waveform when the prior visible state is known. Use bounded
  partial or differential refresh and preserve enough state to supply correct
  previous/target planes.
- Do not run automatic periodic full-refresh cleanup. The user requests cleanup
  with a POWER double-tap.
- Fault recovery must use a verified non-full-refresh path or retain the
  persisted image until the user explicitly requests cleanup. Unknown panel
  state does not authorize an automatic full waveform.

---

## 2. Current Repository Truth and Known Traps

Read these files before editing:

- `firmware/crates/binbook-fw/src/runtime.rs`
- `firmware/crates/binbook-fw/src/runtime/mounted_display_task.rs`
- `firmware/crates/binbook-fw/src/runtime/mounted_reader_io.rs`
- `firmware/crates/binbook-fw/src/runtime/storage_owner_task.rs`
- `firmware/crates/binbook-fw/src/runtime/display_backend.rs`
- `firmware/crates/binbook-fw/src/runtime/input_task.rs`
- `firmware/crates/binbook-fw/src/menu.rs`
- `firmware/crates/binbook-fw/src/reader_state.rs`
- `firmware/crates/binbook-fw/src/state_owner.rs`
- `firmware/crates/binbook-fw/src/storage_owner.rs`
- `firmware/crates/binbook-fw/src/async_refresh.rs`
- `firmware/crates/binbook-fw/src/input.rs`
- `firmware/crates/binbook-fw/src/storage.rs`
- `crates/binbook-storage/src/{filesystem.rs,read_at.rs}`
- `crates/embedded-sd-storage/src/{sd_filesystem.rs,fat_directory.rs}`
- `crates/xteink-x4-display/src/{engine.rs,ui_render.rs,panel.rs}`
- `crates/ssd1677-driver/src/{commands.rs,driver.rs,refresh.rs}`

The implementing agent must account for all of these facts:

1. The menu and its basic button intents already exist and have live display
   evidence. Do not rewrite the framebuffer/font stack.
2. `display_task` opens the selected SD BinBook, installs its real page count,
   renders its saved page, and reports identity-based lifecycle diagnostics.
   The embedded `PROBE_BOOK` remains diagnostic-only when SD storage is absent.
3. Per-book, rendition-aware progress is stored in CRC-protected A/B state files
   on SD and is read back after every committed slot write. The obsolete
   `resume.rs` mechanism has been removed.
4. GPIO3 hold/release gating, SSD1677 deep sleep, ESP32-C3 deep sleep, and GPIO3
   wake are implemented and physically verified from both a nonzero reading
   page and a non-default menu selection.
5. The verified X4 ADC ranges are: channel 1 Right `<=750`, Left `<=1750`,
   Select `<=3000`, Back `<=3800`; channel 2 Down `<=750`, Up `<=2400`; idle
   values such as 4095 decode to no button. A physical Select value of 2748
   exposed and verified the correction from the stale narrower ranges.
8. SD and display share SPI through `FreqManagedSpiDevice`. Preserve that design
   and the existing 4 MHz SD setting unless separate live evidence approves a
   change. Do not bundle SD or display-clock optimization into this work.
9. `sd-storage` now selects the mounted storage-owner runtime. Reader, state,
   index, and diagnostic operations use one FAT/block-device owner with typed,
   bounded requests. Do not restore the old synchronous shared-handle runtime
   or create a second FAT owner.
10. The legacy `storage.rs` adapter is not the mounted reader's status
    authority. Preserve `CardNotFound` as a distinct no-card result throughout
    owner/backend mappings; map FAT/partition/corruption failures to card error
    and exact-file lookup failure to not-found. Do not guess `No SD card` from
    every I/O error.
11. `docs/ROADMAP.md` may have unrelated working-tree changes. Preserve them
   and never overwrite or revert user work.

### Current cache-first checkpoint

Diagnostic storage is now on the same mounted owner as the reader. The SD build
supports paginated index-backed list, bounded read-to-EOF, CRC-checked upload,
delete, and explicit begin/abort. Commit and delete schedule cooperative index
refresh. Live evidence has demonstrated a 29→28 delete publication and 28→29
exact re-add; an in-scan failure is cancelled so it cannot leave a stale owner
scan token. Do not reintroduce a separately mounted diagnostic filesystem.

Owner upload scheduling is client-affine from begin through commit/abort. This
prevents a state-save begin from being interleaved with an active library-index
upload (or vice versa) and receiving `Busy`. The discriminating live test ran a
delete-triggered refresh concurrently with Back: Back completed successfully
before the refresh/index publication finished.

Independent live resume evidence now covers two real SD books: `a.binbook`
restored page 6, `reader-a.binbook` restored page 3, and reopening `a.binbook`
restored page 6 again. Each checkpoint was queried independently through
diagnostic STATUS; packet acknowledgements alone were not used as evidence.

Same-filename replacement is also live-verified with a disposable SD entry. A
16-page rendition saved page 4; overwriting the same name with a distinct
one-page rendition reopened at page 0 with `page_count=1`. Deleting the fixture
published index 30→29 and selection recovered to `a.binbook` page 6.

Reader-state readback verification must compare canonical record content, not
heapless-vector insertion order. The encoder sorts records on wire; after a
successful semantic verification the decoded canonical state replaces RAM.

The dual-slot fallback has also been exercised through the mounted diagnostic
storage owner on live hardware. The newest 292-byte generation-58 slot was
replaced with a deliberately truncated 100-byte file and independently read
back. After reset, structured boot diagnostics reported generation 57 from the
other slot; selecting the preserved book then restored page 6 of its 16 pages.
The subsequent successful background state update republished a valid
generation-58 slot. A second reset selected generation 58 and again restored
page 6 after Select, proving the republished slot was valid rather than merely
the expected size. This is state/outcome evidence, not merely an upload
acknowledgement.

Sleep preparation uses the host-tested `SleepPreparationOutcome` coordinator
in both board runtimes. A state-save failure selects the visible cancellation
notice, emits status 1, returns `PowerAck::Failed`, and forbids both SSD1677 and
CPU deep sleep. A panel-sleep failure emits status 2 and likewise forbids CPU
deep sleep. Only the fully successful outcome can acknowledge readiness to the
physical power task.

Power requests and acknowledgements are strictly one-for-one. The physical
input task waits for and consumes the acknowledgement for both `FullRefresh`
and `PrepareSleep`; `PowerRequestCompletion` authorizes CPU deep sleep only for
the matching `PrepareSleep + Ready` pair. This prevents an earlier full-refresh
acknowledgement from being mistaken for completion of a later sleep request.

- The former duplicate `MenuNames` runtime allocation has been removed. The
  normal SD reader uses the validated cursor and bounded viewport; it does not
  allocate the legacy full `Library` on the menu path.
- `BINBOOK.LIBRARY.INDEX` provides fixed 139-byte persistent records behind a
  validated file-backed `LibraryIndexCursor`. `LibraryViewport` retains 15
  entries (five visible plus prefetch) and measures 2,432 bytes regardless of
  whether the index has 15 or 200 books. Cursor validation and viewport-owner
  loading are wired into the normal mounted reader and live-tested.
- The mounted owner prototype can retain exact FAT sessions and issue
  reusable generation-scoped tokens. The former 200-session cache measured
  17,312 bytes; an active-plus-pending 33,312-byte design was rejected. The
  backend now accepts an explicit set of at most 15 viewport names, retains only
  their sessions, and measures 4,560 bytes. Mounted refresh submits the sorted
  viewport set before scanning. Live
  evidence measured selected-book descriptor access at 205 ms through a token
  versus 4,541 ms through an uncached lookup. The shipped backend is bounded to
  the 15 selected/nearby viewport names and never retains 200 sessions. Active
  indexed sessions return to their viewport slot on a book switch; physical
  A -> B -> A verification measured 206/205/206 ms metadata phases and removed
  the former 4,252 ms final-A fallback.
- The mounted runtime is the `sd-storage` runtime. It validates one
  15-entry viewport, reloads later windows with one bounded record read,
  prepares exact viewport sessions in the background, persists owner-backed
  A/B reader state, restores independent per-rendition progress, renders a
  cancellable delayed grayscale overlay, and shows a recoverable open-error
  screen. Live evidence covers those paths. Physical POWER hold, deep sleep,
  and wake restoration remain mandatory completion gates.
- Reader return performs the cached-menu Clean transition without a directory
  rescan or durable state save in the interaction path. State is dirty in RAM
  and is saved after two seconds of idle; cooperative refresh continues
  afterward. Phase-level live instrumentation
  corrected an earlier attribution error: `DisplayRequestEnd` measures the
  entire request, not the e-ink waveform or the time until the new screen first
  becomes visible. With the old standard-partial activation, open measured
  1,458 ms total (205 ms session metadata, 5 ms page metadata, 194 ms previous
  plane, 549 ms target plane, and 503 ms activation). Back made the menu visible
  after approximately 1,172 ms (475 ms previous plane, 194 ms target plane, and
  503 ms activation), then spent approximately 2,098 ms durably writing and
  reading back state. Do not describe the latter save time as panel latency.
- Menu-to-book and book-to-menu now use the accepted `PreviewFastV1` followed
  immediately by `CleanupStrongV1`; routine page turns retain their existing
  balanced policy. Live instrumentation measured the combined fast activation
  at 184 ms, open at 1,139 ms total, and Back's visible transition at
  approximately 853 ms. The remaining open cost is dominated by SD
  decode/streaming and panel RAM writes, not BUSY waveform time. Any future
  latency claim must report the named phases and visible-transition time
  separately from durable-save and command-completion time.
- Menu highlight navigation now uses the same accepted preview/cleanup recipe
  and reports direct phases. A live injected Down measured 189 ms previous UI
  plane write, 90 ms full menu composition, 189 ms target UI plane write, and
  184 ms activation: 652 ms end to end. A physical press adds the measured
  40 ms debounce and up to one 20 ms polling interval, so activation begins
  approximately 508–528 ms after the press. This proves the remaining delay is
  primarily pre-activation work, not the e-ink waveform.
- Plane-stage probes split a real book open's 553 ms target stage into 360 ms
  SD reads, 27 ms decode, and 165 ms panel writes. Optimize storage access
  before decompression; do not justify a 48,000-byte decoded-plane cache from
  this evidence.
- State-save probes isolated the former approximately two-second close tail:
  reopening the just-committed metadata file for verified readback cost
  1,521 ms. The mounted owner now converts the flushed write session into its
  read session. Live readback fell to 5 ms and the complete verified save to
  475 ms while preserving A/B commit, flush, decode, semantic verification,
  and canonical state replacement.
- RAM-staged persistence removes that verified write from Back. On the current
  246,336-byte image, Back completed in 2,169 ms with phases 191 ms previous
  plane, 190 ms target plane, and 1,710 ms Clean activation. The immediately
  following injected Down completed in 724 ms. Only after two seconds idle did
  the verified state save run; it completed in 581 ms and emitted
  `READER_STATE_SAVED`. A request arriving after synchronous FAT work has begun
  can still queue for that bounded save duration.
- Physical Select, Back, then Down at the first recognizable library screen
  moved the dithered highlight correctly and felt responsive on the same image.
- Diagnostic PAGE first/last/goto now use the mounted reader's exact-page
  lifecycle and the same commit-after-visible-render invariant as page turns.
  The live corrected image reached pages 0, 15, and 6 with matching STATUS.
- Stage diagnostics use `READER_RETURN_STAGE`: stage 1 save, 2 scan, 3 compose,
  4 panel; phase 0 start, 1 end; `arg2` is elapsed milliseconds on end.
- The one-shot diagnostic serial transport discards stale same-sequence frames
  with the wrong opcode and continues waiting for the exact sequence/opcode pair.

---

## 3. New Interfaces and Binary State Format

### `binbook-core` rendition identity

Add a public minimal type and reader:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenditionIdentity {
    pub rendition_hash: [u8; 32],
}

impl<R: ReadAt> Book<R> {
    pub fn rendition_identity(
        &mut self,
        record: &mut [u8],
    ) -> Result<RenditionIdentity, Error<R::Error>>;
}
```

Read the required `RENDITION_IDENTITY` section through the existing section
directory and `read_prefix` machinery. Require the current 312-byte section
shape, parse its first 32 bytes as the canonical rendition hash, and reject a
short section/buffer or all-zero hash. Do not expose section offsets and do not
change `BINBOOK_FORMAT_SPEC.md`.

### Shared SD access

Create a firmware-local shared wrapper; do not put Embassy or board ownership
inside reusable storage crates:

```rust
pub struct SharedSdFilesystem<F> {
    inner: embassy_sync::blocking_mutex::Mutex<
        CriticalSectionRawMutex,
        core::cell::RefCell<F>,
    >,
}

#[derive(Clone, Copy)]
pub struct SdHandle<'a, F> {
    shared: &'a SharedSdFilesystem<F>,
}
```

`SdHandle` implements `Filesystem` and `WritableFilesystem` by locking for one
synchronous filesystem operation. Never await while the lock or `RefMut` is
held. The display task and diagnostic task receive handles to the same static
filesystem instance.

Add an owned named source used by `binbook-core`:

```rust
pub struct SdBookSource<'a, F> {
    fs: SdHandle<'a, F>,
    name: heapless::String<{ binbook_storage::MAX_BINBOOK_NAME_BYTES }>,
}
```

It implements `binbook_core::ReadAt` using `file_size` and `read_at`, so the
`Book` can own its source without retaining `&mut F` across the reading session.

### Reader-state wire format

Implement this in a new `firmware/crates/binbook-fw/src/reader_state.rs`. The
format is firmware state, not a BinBook section.

Header, exactly 72 bytes:

```text
0..4    magic = "BBST"
4..6    version:u16 = 1
6..8    header_size:u16 = 72
8..12   generation:u32
12      saved_mode:u8 (0=menu, 1=reading)
13..16  reserved = 0
16..24  selected_filename_hash:u64
24..32  active_filename_hash:u64
32..64  active_rendition_hash:[u8;32]
64..66  record_count:u16 (0..=200)
66..68  reserved = 0
68..72  crc32:u32
```

Each progress record, exactly 44 bytes:

```text
0..8    filename_hash:u64
8..40   rendition_hash:[u8;32]
40..44  page:u32
```

Rules:

- Total encoded size is `72 + record_count * 44`; maximum is 8,872 bytes.
- `filename_hash` is FNV-1a 64 over the exact UTF-8 filename bytes. Add known
  vector tests so the algorithm cannot drift.
- Sort records by `(filename_hash, rendition_hash)` before encoding.
- CRC32 is CRC-32/ISO-HDLC over the complete encoded file with header bytes
  `68..72` temporarily zero. Create
  `firmware/crates/binbook-fw/src/crc32.rs` as the single firmware-local
  implementation and migrate the equivalent private helpers in `diag_log.rs`
  and `diag_storage.rs` to it, preserving their existing known-vector tests. Do
  not add a hashing dependency.
- Decode rejects wrong magic/version/header size, nonzero reserved bytes,
  invalid mode, count over 200, length mismatch, duplicate keys, all-zero
  progress-record rendition hashes, and CRC mismatch. In Menu mode the active
  filename/rendition fields must be zero; in Reading mode they must be nonzero
  and match one progress record.
- Compare slot generations with wrapping arithmetic. With no valid slot, start
  from empty menu state and expose a nonfatal state warning. The first save
  writes slot A at generation 1; later saves overwrite the invalid/older slot
  at `loaded_generation.wrapping_add(1)`.
- To save, write the invalid/older slot with `begin_upload`, sequential
  `write_upload_chunk` calls, and `commit_upload`; then read it back completely
  and decode/CRC-verify it. Only after verification is the generation committed
  in RAM. A failed/torn write must leave the previous slot readable.

### Dynamic display backend

Use one concrete source enum so menu, SD reading, and diagnostic probes do not
require incompatible backend types:

```rust
pub enum RuntimeBookSource<'a, F> {
    Sd(SdBookSource<'a, F>),
    #[cfg(feature = "diagnostic-console")]
    Embedded(binbook_core::SliceSource<'static>),
}
```

Implement `ReadAt` for the enum with a firmware-local source error. Generalize
`HardwareDisplayBackend` so `book` is
`Option<Book<RuntimeBookSource<'a, F>>>`. Every `DisplayBackend` render method
returns `DisplayError::InvalidState` when there is no active book. Menu/status
rendering continues to access the panel directly.

Add:

```rust
pub async fn initialize_at<B: DisplayBackend, S: EventSink>(
    &mut self,
    page: u32,
    backend: &mut B,
    events: &mut S,
    now: u64,
) -> Option<DisplayCompletion>;
```

It validates `page < page_count`, initializes the panel, recovery-seeds that
page, and schedules the normal gray settle. Existing `initialize()` delegates
to `initialize_at(0, ...)` so all existing callers remain compatible.

### Power interfaces

Add `ControllerState::DeepSleep`, `Error::InvalidState`,
`Command::DEEP_SLEEP = 0x10`, and:

```rust
pub fn deep_sleep(&mut self) -> Result<(), Error>;
```

The method sends command byte `0x10` with no data, then records
`ControllerState::DeepSleep`. Writes, refresh activation, and waveform loading
while in that state return `InvalidState` until a full init/reset succeeds. Add
an `X4Panel::deep_sleep()` wrapper.

Use a firmware-local `PowerRequest::PrepareSleep` / `PowerAck` channel and an
atomic lifecycle state (`Running`, `SleepPending`). Diagnostic KEY injection of
logical POWER remains a no-op because it has no trustworthy hold duration; only
physical GPIO3 can initiate sleep.

---

## 4. File Map

### Create

- `firmware/crates/binbook-fw/src/reader_state.rs`: state model, codec, record
  lookup/update/pruning, slot selection, and fixed-capacity buffers.
- `firmware/crates/binbook-fw/src/crc32.rs`: the shared firmware-local
  CRC-32/ISO-HDLC implementation used by reader state and diagnostics.
- `firmware/crates/binbook-fw/src/runtime/storage_access.rs`: shared filesystem
  handle, named SD `ReadAt`, slot persistence, and library scan adapter.
- `firmware/crates/binbook-fw/src/power.rs`: pure power-hold state machine and
  shutdown lifecycle types.
- `firmware/crates/binbook-fw/src/runtime/power_task.rs`: GPIO3 release-gated
  RTC deep-sleep owner.
- Focused integration tests as named in the tasks below.

### Modify

- `crates/binbook-core/src/{lib.rs,book_records.rs}` and tests: rendition hash.
- `crates/xteink-x4-display/src/engine.rs` and engine tests:
  `initialize_at`.
- `crates/ssd1677-driver/src/{commands.rs,driver.rs,error.rs,refresh.rs}` and
  driver tests: controller deep sleep.
- `firmware/crates/binbook-fw/src/{lib.rs,input.rs,menu.rs,async_refresh.rs}`:
  corrected input thresholds, full-length names, statuses, and reader/power
  types.
- `firmware/crates/binbook-fw/src/runtime/{display_backend.rs,display_task.rs,input_task.rs}`:
  actual selected-book lifecycle, progress, error flow, and sleep coordination.
- `firmware/crates/binbook-fw/src/{main.rs,runtime.rs}`: GPIO3/LPWR ownership,
  shared SD setup, boot scan, state restore, and task wiring.
- `firmware/crates/binbook-fw/src/runtime/diagnostic_console.rs`: consume an
  `SdHandle` rather than exclusive `&mut SdFilesystem` ownership.
- Replace/remove `firmware/crates/binbook-fw/src/resume.rs` after all callers
  use `reader_state`; do not leave two authorities.
- Update tests and docs named in Task 9.

---

## 5. Sequential Implementation Tasks

## Task 0: Establish Baseline and Correct Physical Button Decoding

**Purpose:** Begin from known repository health and fix a current physical-input
prerequisite before judging menu behavior.

- [ ] Record `git status --short`; preserve the existing `docs/ROADMAP.md`
  modification and any later unrelated user changes.
- [ ] Run the current focused baseline:

  ```bash
  RUSTUP_TOOLCHAIN=stable cargo test -p binbook-fw
  RUSTUP_TOOLCHAIN=stable cargo test -p binbook-fw --features diagnostic-console,sd-storage
  RUSTUP_TOOLCHAIN=stable cargo test -p xteink-x4-display
  RUSTUP_TOOLCHAIN=stable cargo test -p embedded-sd-storage --test fat_image
  ```

- [ ] Add failing boundary tests in `input.rs` for every verified ADC threshold,
  exact threshold values, one value above each threshold, and idle `4095/4095`.
  Include `decode_buttons(4095, 4095) == None` as the discriminating idle case.
- [ ] Replace the incorrect thresholds with the locked X4 values from Section 2.
- [ ] Run `cargo test -p binbook-fw` and the feature-gated firmware tests.
- [ ] Do not proceed if baseline failures are unexplained. Record pre-existing
  failures separately from regressions in `HANDOFF.md`.

## Task 1: Expose the BinBook Rendition Identity

- [ ] Add failing `binbook-core` tests using a real fixture to assert the exact
  32-byte rendition hash; mutate the section length/hash in a copied fixture to
  prove malformed and all-zero identities are rejected.
- [ ] Add `RenditionIdentity`, its parser, public export, and
  `Book::rendition_identity` using the existing section read path.
- [ ] Run:

  ```bash
  RUSTUP_TOOLCHAIN=stable cargo test -p binbook-core
  ```

- [ ] Confirm no encoder bytes or `BINBOOK_FORMAT_SPEC.md` layouts changed.

## Task 2: Implement the Pure Reader-State Model and Codec

- [ ] Create failing tests for:
  - empty state encoding;
  - exact 72-byte header and 44-byte record layouts;
  - FNV-1a known vectors;
  - maximum 200 records and rejection of 201;
  - deterministic sorting;
  - CRC corruption;
  - bad version/mode/reserved bytes/length;
  - duplicate records;
  - generation wrap selection;
  - lookup requiring both filename and rendition hashes;
  - replacement under the same name returning no saved page;
  - successful prune of a deleted filename;
  - page update only after a simulated successful commit;
  - saved Menu and Reading modes.
- [ ] Implement `ReaderState` with `heapless::Vec<ProgressRecord, 200>` and one
  caller-owned `[u8; 8_872]` encoding buffer. No heap allocation and no fixed
  temporary larger than the intentional persistent state buffer.
- [ ] Replace the obsolete one-book `ResumeRecord` exports only after tests for
  the new model pass.
- [ ] Run:

  ```bash
  RUSTUP_TOOLCHAIN=stable cargo test -p binbook-fw reader_state
  RUSTUP_TOOLCHAIN=stable cargo test -p binbook-fw
  ```

## Task 3: Share SD Access and Persist Dual State Slots

- [ ] Add a failing host test with an in-memory `Filesystem` proving two handles
  reach the same underlying files and that `SdBookSource` reads a payload larger
  than the obvious 1 KiB/4 KiB scratch sizes without truncation.
- [ ] Add fault-injection tests for every slot-save boundary: create/truncate,
  first chunk, later chunk, commit/flush, readback, and CRC verification. Each
  failure must leave the previous valid generation loadable.
- [ ] Add FAT-image tests that create/read/alternate `BINBOOK.A.STATE` and
  `BINBOOK.B.STATE`, verify exact bytes, and confirm neither appears in
  `.binbook` enumeration.
- [ ] Implement `SharedSdFilesystem`, `SdHandle`, and `SdBookSource` in the
  firmware integration layer. Do not add Embassy dependencies to
  `binbook-storage` or `embedded-sd-storage`.
- [ ] Implement `ReaderStateStore::load()` and `save_verified()` using the
  existing streaming upload hooks. Perform all slot operations synchronously
  under one filesystem handle; never yield while an upload session is active.
- [ ] Change diagnostic storage construction to use a shared handle. Re-run the
  existing diagnostic upload/list/read/delete tests to prove this refactor did
  not weaken storage behavior.
- [ ] Run:

  ```bash
  RUSTUP_TOOLCHAIN=stable cargo test -p binbook-storage
  RUSTUP_TOOLCHAIN=stable cargo test -p embedded-sd-storage --test fat_image
  RUSTUP_TOOLCHAIN=stable cargo test -p binbook-fw --features diagnostic-console,sd-storage --test diagnostic_storage
  ```

## Task 4: Build the Bounded Library Model and Status Screens

- [ ] Add failing pure tests for deterministic sorting, exact preservation of
  121-byte names, visual truncation only, five-row scrolling, selection restore
  by filename hash, and 201-entry overflow.
- [ ] Add scan-result tests that distinguish `NoCard`, `CardError`, `NoBooks`,
  `Ready`, `TooManyBooks`, and `UnsupportedName`. Do not collapse them into one
  empty list.
- [ ] Refine the firmware SD error adapter so `SdCardError::CardNotFound` maps to
  `NoCard`, filesystem/partition errors map to `CardError`, and file lookup maps
  to `NotFound`. Cover every mapping with a host test.
- [ ] Add state-GC tests proving records are pruned only after `Ready` or a
  successful empty scan, never after mount/enumeration error.
- [ ] Change the menu cache to
  `heapless::String<{ binbook_storage::MAX_BINBOOK_NAME_BYTES }>` and define one
  `LibraryEntry` authority containing full name, size, and filename hash.
- [ ] Add `LibraryStatus` and render status/error frames using the existing
  `Gray2Framebuffer` and `render_ui_bw`. Do not create another graphics stack.
- [ ] Remove normal-runtime insertion of `nav_probe.binbook`. Under
  `diagnostic-console`, retain the embedded fixture only for explicit display
  probes, not as an SD menu entry.
- [ ] Implement Select-to-rescan for library status screens. Back is a no-op and
  POWER remains owned by the physical power task.
- [ ] Run:

  ```bash
  RUSTUP_TOOLCHAIN=stable cargo test -p binbook-fw --features sd-storage
  RUSTUP_TOOLCHAIN=stable cargo test -p xteink-x4-display
  ```

## Task 5: Open and Render the Actual Selected SD Book

- [ ] Create two small, host-generated BinBook fixtures with different page
  counts and unmistakably different content. Every page must follow the display
  test-image discipline: orientation border/corner labels, edge labels,
  crosshair/rulers, grayscale swatches, faint grid, page number, and an
  asymmetric top-left marker. Book A uses dominant horizontal bands; Book B
  uses a checkerboard or vertical bands.
- [ ] Add a failing integration test that selects Book B after Book A and proves
  the opened source name, page count, rendition hash, and rendered page bytes
  are Book B's. A mode-change event or `Ok` acknowledgement alone is not proof.
- [ ] Add failing `xteink-x4-display` engine tests for `initialize_at`: saved
  nonzero page renders directly, invalid pages fail, and `initialize()` remains
  equivalent to page 0.
- [ ] Implement `RuntimeBookSource`, make backend book storage optional, and
  guard all book-dependent renderer methods against no active book.
- [ ] On `MenuSelect`, open the selected `SdBookSource`, validate the BinBook,
  read its rendition hash/page count, install it in the backend, construct a new
  engine, and call `initialize_at(saved_or_zero)`.
- [ ] Only publish `Mode::Reading`, `BookOpen`, current page, and page count after
  the initial page render succeeds. On failure, discard the book/engine and
  render `Cannot open book`.
- [ ] On Back/Select in reading, cancel pending overlay/base-sync safely, retain
  the last committed page, drop the active book, render the cached library with
  its restored selection, then request the deferred refresh from Task 5A.
- [ ] Add tests for corrupt files, unsupported X4 profiles, zero/invalid page
  count, first/last boundaries, read failure, retry, and Back recovery.
- [ ] Run:

  ```bash
  RUSTUP_TOOLCHAIN=stable cargo test -p xteink-x4-display
  RUSTUP_TOOLCHAIN=stable cargo test -p binbook-fw --features diagnostic-console,sd-storage
  ```

## Task 5A: Cooperative Storage Owner and Background Library Refresh

This task is required for genuine background refresh. Merely spawning the
current synchronous FAT enumeration in another Embassy task is incorrect on the
single-core ESP32-C3 because it still blocks the executor. Likewise, a mounted
FAT iterator cannot be held across `await` through the current synchronous
critical-section/`RefCell` handle.

- [ ] Add host tests for a single storage-owner request protocol before changing
  runtime wiring. Cover bounded reads, file size, state upload begin/write/
  commit/abort, diagnostic storage operations, scan start/step/finish, busy
  rejection, cancellation, and error propagation.
- [ ] Define fixed-capacity request/response types in `binbook-fw`; do not put
  filenames or payloads on unbounded allocation paths. Preserve the 121-byte
  filename and existing diagnostic frame limits. Large page/state payloads must
  remain caller-owned or streamed; do not transfer the 28 KiB `Library` through
  a channel by value.
- [ ] Move `RuntimeSdFilesystem` into one Embassy storage-owner task. Display,
  state persistence, and diagnostic storage must stop borrowing the underlying
  filesystem directly and instead use typed clients. There must remain exactly
  one FAT/block-device owner.
- [ ] Add a mounted directory-scan session inside the owner. Process at most
  eight raw directory entries per step into a file-backed replacement index,
  then yield to the executor. Keep the FAT session mounted across steps; do not
  remount and rescan from index zero. Produce deterministic case-folded order
  with bounded sort metadata/record offsets rather than a full resident name
  collection.
- [ ] Validate the completed replacement index structurally and by CRC before
  atomic publication. Failed or cancelled scans retain the prior valid index.
  FAT has no add/remove notification, so schedule cooperative scans at boot and
  opportunistically after return-to-menu or idle time.
- [ ] Make the menu read through `LibraryIndexCursor` and retain only a
  15-entry `LibraryViewport` (five visible plus bounded prefetch). Do not load
  all 200 records at menu entry and do not add a second full `Library` buffer.
- [ ] Replace the proven 200-session prototype cache with a small bounded
  selected/nearby session prefetch/LRU. Tokens are mount- and scan-generation
  scoped, publish only after a successful scan, are never persisted, and are
  consumed when promoted to the active book.
- [ ] Make Select use the prefetched exact session without scanning `/BOOKS`.
  On a prefetch miss, prioritize a cooperative exact-file lookup while leaving
  the menu visible and responsive; do not put a multi-second scan in the
  book-selection UX path.
- [ ] Prioritize bounded page/state reads and power-save persistence over
  background scan steps. Define cancellation when a selected book opens or
  sleep begins. A cancelled or failed scan must not mutate the active cache or
  prune progress.
- [ ] On Back/Select, render the validated viewport first and enter Menu mode.
  Start or continue refresh only after the cached panel operation completes.
  When a complete replacement is `Ready` or `NoBooks`, atomically publish it,
  restore selection by filename hash, prune deleted progress, persist pruning,
  and redraw only when visible content/status changed.
- [ ] For `NoCard`, `CardError`, `TooManyBooks`, or `UnsupportedName`, retain the
  active cache. Emit the scan status and expose a recoverable non-destructive
  warning; never replace a valid cache with a partial listing.
- [ ] Add a responsiveness test with a 200-entry fake scan proving input/status
  requests are serviced between scan batches. A test that only proves the final
  listing is insufficient.
- [ ] Add RAM assertions/evidence for the 2,432-byte viewport and the chosen
  bounded session LRU. Prove menu/session RAM is independent of a 200-book
  library. Keep the measured 17,312-byte all-session cache only as comparative
  prototype evidence; do not ship it as the final menu cache.
- [ ] Run feature-gated host tests after each migration slice. Do not delete the
  old synchronous handle until every reader, state, and diagnostic operation has
  an implemented and tested owner request.
- [ ] Flash and verify through diagnostic Select/Back before physical input.
  Required evidence: cached panel stage completes before scan finish; Select
  reaches book metadata without a directory rescan; STATUS and input queries
  succeed during a 200-entry scan; only the bounded viewport/prefetch is
  resident; unchanged scan causes no second panel operation; changed scan
  publishes and redraws; failed scan retains the prior index.

Completion evidence for Task 5A must include the request implementation path,
host tests for every request kind, target build, exact stage logs, and live
visual/query results. Compilation or a completed scan alone is not sufficient.

## Task 6: Integrate Per-Book Progress and Last-Screen Restore

- [ ] Add an end-to-end host test with this discriminating sequence:
  1. Open A at page 0 and successfully render page 2.
  2. Close A and verify a slot write/readback succeeds.
  3. Open B, render page 1, and close it.
  4. Reopen A and verify direct initialization at page 2.
  5. Reopen B and verify direct initialization at page 1.
- [ ] Add tests that a failed page render leaves the prior page saved, a failed
  close save retains progress in RAM and sets the warning, and a later save
  clears the warning.
- [ ] Add boot-restore tests for Menu, Reading, missing active filename,
  same-name/different-rendition replacement, reduced page count (clamp to
  `page_count - 1`), corrupt A with valid B, corrupt both slots, and absent state
  files.
- [ ] Load state only after a successful SD mount. Scan, select the newest valid
  state slot, prune missing filenames, persist pruning if necessary, then choose
  the initial screen.
- [ ] Update the in-RAM record only on successful page commit. Flush the state on
  reading close and immediately before sleep.
- [ ] Ensure diagnostic `STATUS` reports the active SD book's page/page count,
  not the embedded fixture's values.
- [ ] Run the full feature-gated firmware host tests before continuing.

## Task 7: Add SSD1677 Deep Sleep and Physical POWER Lifecycle

- [ ] Add failing fake-SPI driver tests proving `deep_sleep()` sends exactly
  command `0x10`, sends no data, sets `ControllerState::DeepSleep`, and rejects
  write/refresh/waveform operations until full initialization succeeds.
- [ ] Implement the driver and X4 panel wrappers without board-specific GPIO or
  RTC dependencies in reusable crates.
- [ ] Add failing pure `PowerHoldState` tests for: short press, 1,999 ms hold,
  2,000 ms hold, one-shot request while still held, bounce/release reset, failed
  preparation requiring a new press, and release gating.
- [ ] Add GPIO3 and LPWR to the `main.rs` peripheral destructure and
  `RuntimePeripherals`. Create GPIO3 as pull-up input.
- [ ] Implement `power_task` at a 20 ms poll cadence. On a two-second hold, set
  lifecycle `SleepPending`, request display/storage preparation, and await the
  acknowledgement. Navigation input must be ignored while pending.
- [ ] In `display_task`, serialize preparation after any active render: cancel
  only cancellable pre-activation work, wait for active BUSY completion, encode
  and verify state, call panel deep sleep, then return success. On any failure,
  return an error and restore `Running` without entering CPU sleep.
- [ ] After a successful acknowledgement, wait until GPIO3 reads high, construct
  an active-low `RtcioWakeupSource`, and call `Rtc::sleep_deep`. Do not add timer
  wake in this POC.
- [ ] On boot, record `wakeup_cause()` in structured diagnostics and use the same
  scan/state restoration path for cold boot and POWER wake.
- [ ] Add host tests for shutdown ordering and all failure boundaries. A success
  acknowledgement is permitted only after storage verification and controller
  sleep both succeeded.
- [ ] Build the RISC-V target immediately after this task; host tests cannot
  prove the ESP-HAL RTC API wiring.

## Task 8: Diagnostic Acceptance Surfaces and Adversarial Fixtures

- [ ] Keep diagnostic `KEY` for Left/Right/Up/Down/Select/Back routed through the
  same mode-aware path as physical input. Logical diagnostic POWER remains a
  no-op and must not claim a physical hold occurred.
- [ ] Do not add a new diagnostic opcode or change the existing `STATUS` payload
  for this POC. Continue using STATUS for active page/page count and STORAGE for
  the independent directory listing.
- [ ] Add shared diagnostic event codes and concrete runtime events for:
  `LIBRARY_SCAN(status, book_count)`,
  `BOOK_OPENED(filename_hash_low32, rendition_hash_prefix32, page_count)`,
  `READER_STATE_SAVED(generation, record_count)`,
  `READER_STATE_PRUNED(removed_count, remaining_count)`,
  `SLEEP_PREPARED(status)`, and `WAKE_CAUSE(cause)`. Use the existing three
  signed event arguments; do not emit zero-filled success data before the
  operation completes.
- [ ] Use stable argument codes: library status `0=Ready`, `1=NoCard`,
  `2=CardError`, `3=NoBooks`, `4=TooManyBooks`, `5=UnsupportedName`,
  `6=OpenError`, `7=ReadError`; sleep status `0=Ok`, `1=StateSaveError`,
  `2=PanelSleepError`; wake cause `0=ColdOrOther`, `1=PowerGpio`. Store hash
  words bit-for-bit in `i32` arguments rather than narrowing their numeric
  value.
- [ ] Define those numeric codes in `binbook-diagnostic-protocol`, map them in
  the firmware aggregator, and teach the existing CLI log formatter their
  names. Do not hand-assemble a second payload format.
- [ ] Add host diagnostic tests that verify response opcode, sequence, status,
  payload, resulting state, and a follow-up query for every new action.
- [ ] Prepare an SD acceptance set containing Book A, Book B, one corrupt
  `.binbook`, and both state slots. Do not rely on whatever books happen to be
  on the user's card.
- [ ] Add an overflow host fixture with 201 names; hardware need only cover the
  normal two-book library unless the live card can be prepared safely.

## Task 8A: Pre-Physical Menu/Reader UX Latency Pass

Complete this task before asking the user to perform physical verification.
Diagnostic key injection and webcam evidence exercise the software flow; the
later physical session reconfirms ADC input and POWER behavior and must not be
used as the first test of these latency changes.

### Measurement contract

- [x] Add diagnostic-only named phase probes around selected-book/session open,
  page metadata, previous-plane staging, target-plane staging, activation, and
  close-state save. Compile them out when `diagnostic-console` is disabled.
- [x] Treat `DisplayRequestEnd` as end-to-end command completion only. Report
  separately: input-to-first-visible-new-screen, panel RAM staging, BUSY
  activation, and work intentionally continuing after the new screen is
  visible. Never infer panel latency from a multi-stage request total.
- [x] Establish discriminating live baselines from a nonzero real SD page. The
  baseline must include phase events, STATUS before and after, and a fresh
  webcam image; an acknowledgement alone is insufficient.

### Fast transition policy

- [x] Begin with a failing `xteink-x4-display` fake-SPI test that requires the
  accepted `PreviewFastV1` then `CleanupStrongV1` LUT tails and exactly two
  activations for an already-staged menu/book transition.
- [x] Add a reusable staged-plane fast differential activation and use it only
  for menu-to-book and book-to-menu. Keep routine page turns on their existing
  balanced policy and full refresh exclusively on the explicit POWER chord.
- [x] Live-verify the change. Current evidence is 184 ms for preview plus
  cleanup versus 503 ms for standard partial; total open is 1,139 ms, and Back
  first shows the menu at approximately 853 ms. Preserve the exact phase logs
  in `HANDOFF.md`.
- [x] Use the accepted fast transition for menu highlight changes and add
  direct previous-plane, compose, target-plane, and activation probes. Live
  diagnostic Down is 652 ms total with phases 189/90/189/184 ms.
- [x] Enforce regression budgets through named phase events and live hardware
  measurements:
  menu/book activation must remain below 250 ms on the reference device,
  menu-to-book first-visible latency below 1,250 ms, and book-to-menu
  first-visible latency below 1,000 ms. Hardware measurements remain the
  authority; host tests cover phase/event mapping but must not fake elapsed
  hardware time. Current evidence is 184 ms, 1,139 ms, and approximately
  926 ms respectively.

### Remaining justified work

- [x] Stage bounded reader state in RAM and schedule verified persistence after
  two seconds idle. Foreground interaction postpones a not-yet-started save;
  failed saves remain dirty and retry after five seconds. The synchronous FAT
  operation is not preemptible once started, so this removes storage from the
  normal transition path but does not claim concurrent FAT I/O.
- [x] Remove redundant state-slot directory reopen from verified save. Begin
  with a failing mounted-owner block-read test, retain the flushed metadata
  write session for immediate readback, and keep other metadata files
  independently reopenable. Live readback improved from 1,521 ms to 5 ms and
  complete verified save from 2,105 ms to 475 ms.
- [x] Preserve the durability barrier: `PrepareSleep` verifies the newest RAM
  state before panel/CPU sleep and cancels sleep on failure. Ordinary close no
  longer waits for durable persistence.
- [x] Keep the bounded 15-entry session cache and do not retain a just-closed
  `OwnerBook` as an unchecked metadata cache. Diagnostic overwrite/delete can
  invalidate the active FAT file while leaving that in-RAM index structurally
  plausible; reuse would require a replacement-generation/identity validation
  that costs the metadata read it attempts to avoid. The measured 205–207 ms
  exact cached open is within the accepted 1,250 ms visible-open budget.
- [x] Investigate the 549 ms target-plane stage using phase probes around SD
  reads, decompression, and panel writes. Compare it with the 194 ms UI-plane
  write to isolate storage/decode cost. Before adding a decoded-plane cache,
  record compressed fixture sizes and a release map-size delta; a 48,000-byte
  full-plane cache is not accepted without evidence that its latency benefit is
  worth the constrained-device RAM cost. Prefer bounded streaming and reuse of
  already-resident panel RAM.
- [x] Evaluate dirty-window and incremental-menu candidates empirically before
  adopting them. A 200×480 rotated window reduced each plane stage only from
  about 189 ms to 143–144 ms, produced ambiguous retained-band webcam evidence,
  and initially stretched activation while the partial window remained active.
  A full-window activation corrected the waveform to 186 ms, but incremental
  row composition took 116 ms versus 90 ms for byte-clear plus full compose.
  Both runtime experiments were removed; the verified 652 ms full-plane fast
  transition remains the POC policy. Do not revive either optimization without
  a different conversion/render strategy and discriminating visual evidence.
- [x] Do not add a Select “feedback” e-ink refresh. The existing phase evidence
  shows that an extra preview/cleanup activation
  would add about 184 ms before the book, so it is not part of this POC.

### Pre-physical exit gate

- [x] Run the focused display and firmware feature-gated tests, formatting,
  diff check, and the pinned-nightly combined target build.
- [x] Flash the combined diagnostic/SD image, then inject Select and Back from
  discriminating states. Capture named phase logs, independent STATUS results,
  and fresh webcam evidence. Leave the device on a known nonzero page for the
  later physical session.
- [x] Update the refresh-policy reference, reader-flow design, roadmap, and
  `HANDOFF.md` with current behavior and RAM/timing evidence. Do not put local
  capture paths in tracked documents.

## Task 9: Full Verification, Documentation, and Completion Audit

- [x] Run formatting and all host gates from the repository root:

  ```bash
  RUSTUP_TOOLCHAIN=stable cargo clean
  RUSTUP_TOOLCHAIN=stable cargo test --workspace
  RUSTUP_TOOLCHAIN=stable cargo test -p binbook-fw --features diagnostic-console
  RUSTUP_TOOLCHAIN=stable cargo test -p binbook-fw --features diagnostic-console,sd-storage
  RUSTUP_TOOLCHAIN=stable cargo test -p binbook --features serial-device
  RUSTUP_TOOLCHAIN=stable uv run pytest -q
  RUSTUP_TOOLCHAIN=stable cargo fmt --check
  git diff --check
  ```

- [x] Build the target with the pinned nightly tools, not arbitrary PATH tools.
  Use a subshell so later commands still run from the repository root:

  ```bash
  (cd firmware && \
    RUSTC="$(rustup which --toolchain nightly rustc)" \
    rustup run nightly cargo build -p binbook-fw \
      --features firmware-bin,diagnostic-console,sd-storage \
      --target riscv32imc-unknown-none-elf --release)
  ```

- [x] Follow `docs/reference/xteink-x4-agent-device-verification.md`
  sequentially. Only one process may own `$PORT`. The older runbook's fixed
  `page_count=16`/page-15 assertions apply only when its embedded diagnostic
  fixture is explicitly active; for the SD reader, substitute the known Book A
  or Book B page count and record that precondition. Do not report a mismatch
  merely because the POC intentionally boots to the library. Flash the actual
  combined feature image:

  ```bash
  export PORT="${PORT:-/dev/ttyACM0}"
  ESPFLASH_PORT="$PORT" \
  FW_FEATURES="firmware-bin,diagnostic-console,sd-storage" \
    firmware/scripts/flash-xteink-x4-nav-probe.sh
  ```

- [x] Capture at least 15 seconds of serial output using the exact pyserial
  command in `AGENTS.md`. Record bootloader, partition, segment/app load, and
  wake-cause output.
- [x] Establish independent HELLO, STATUS, storage-list, and log baselines.
- [x] Use diagnostic keys and fresh webcam captures to verify menu navigation,
  opening A, opening B, page turns, boundaries, Back, corrupt-book recovery,
  and per-book reopen. Every displayed test page must retain the required
  orientation frame.
- [x] Physically verify the ADC buttons, not only injected keys: Up, Down,
  Select, Back, Left, and Right must each produce the expected visible/state
  transition from a discriminating starting state.
- [x] Physically test POWER:
  - short press does not sleep;
  - hold for two seconds, release, and observe USB disappearance/deep sleep;
  - press POWER again and observe USB re-enumeration;
  - repeat once from a nonzero reading page and once from a non-default menu
    selection;
  - confirm the restored screen visually and through independent status/state
    queries.
- [x] Delete Book A, trigger a successful rescan/reboot, and prove its progress
  record is absent and it does not reappear at the old page if later re-added.
- [x] At the storage error-classification boundary, inject `CardNotFound` and a
  distinct device/FAT failure. Prove the former renders `No SD card`, the latter
  renders `Card error`, both retain the last valid index and reader-state
  records, and Select schedules a retry. The X4 has no card-detect input, so
  physical removal is optional board qualification rather than completion
  evidence for this contract. Hot removal while reading is out of scope.
- [x] Update `HANDOFF.md` immediately with exact commands, relevant full output,
  starting/ending states, webcam paths, verified behavior, transport-only
  acknowledgements, failures, and remaining gaps. Keep it a current snapshot,
  not a diary, and do not stage it.
- [x] Update `docs/specs/2026-07-01-library-menu-reading-flow-design.md` to the
  final current design: real SD opening, explicit empty/error states, 200-book
  bound, dual SD state slots, per-book progress, last-screen restore, and
  physical sleep/wake. Remove the old internal-flash and embedded-fallback
  claims.
- [x] Update `docs/reference/xteink-x4-firmware-flashing.md` with current button,
  library, and POWER behavior plus verified commands. Use portable placeholders
  such as `$PORT` and `$OUT`; do not put host-local artifact paths in tracked
  docs.
- [x] Grep `docs/ROADMAP.md` and all tracked docs for `library`, `menu`,
  `resume`, `reader`, `sleep`, `wake`, `nav_probe`, and the old plan filename.
  Remove/rewrite stale future-work claims while preserving unrelated existing
  edits. Completed work must be removed from the roadmap, not marked done.
- [x] Perform the adversarial completion review: inspect every request branch,
  deliberately start from non-default pages/selections, corrupt one state slot,
  inject both no-card and generic storage failures, replace a same-name book,
  and attempt to disprove each claim in the acceptance matrix. Do not call the
  POC complete with any blank evidence cell. For every gate, identify the
  hardware/runtime signal it observes and reject tests that cannot discriminate
  the claimed behavior.

---

## 6. Required Acceptance Matrix

Copy this table into `HANDOFF.md` and replace every `pending` cell with exact
test names and live evidence before completion.

| Requirement | Implementation path | Automated test | Observed device evidence |
| --- | --- | --- | --- |
| Correct physical ADC buttons | `input.rs`, `input_task.rs` | `adc_ladder_boundaries_match_x4_reference`; debounce/raw-poll tests | Exact-image Left/Right/Up/Down logged discriminating ADC values and committed 1→0→1→0→1; Select/Back visibly entered/exited the book and restored the library |
| Deterministic SD library | library scan/model/cursor | sorting and 200-book cursor tests | Live 35-record sorted scan and bounded 15-record viewport |
| Explicit no-card/card-error/no-books states | status renderer/classifier | `card_not_found_is_distinct_from_other_sd_device_failures`; `status_distinguishes_all_scan_outcomes`; `status_frames_render_visible_explanations` | Error boundary distinguishes all states; inserted-card production path verified live; no card-detect pin exists |
| Capacity/name errors are not silent | bounded scan | 201-book and 122-byte-name cases in `status_distinguishes_all_scan_outcomes` | Exact status contract verified at the scan boundary; hardware fixture is not required |
| Selected filename opens the actual SD bytes | exact owner session/book | `cached_book_open_activates_the_exact_mount_token_before_reading_metadata`; selected-owner-read test | Valid, one-page, two-page, and corrupt selected entries produced distinct live outcomes |
| Actual selected page count is used | owner book/display snapshot | `opening_a_book_replaces_probe_status_with_the_real_book_state` | STATUS reported counts 16, 2, and 1 for distinct selected SD books |
| Page turns and boundaries commit only successful pages | owner lifecycle/display | boundary, failed-render, and exact-goto tests | Injected boundaries and exact-image physical 1→0→1→0→1 transitions matched STATUS/logs |
| Back/Select returns to preserved menu selection | mounted reader/menu | menu restore and filename-hash selection tests | Physical Back and Select returned to the preserved row; same-row reopen worked without a navigation workaround |
| Book A and B retain independent pages | reader state/A-B slots | `visible_page_commit_and_close_preserve_independent_book_progress` | Live A page 6, B page 3, A page 6 restoration |
| Same-name replacement starts fresh | rendition-aware commit | replacement rendition tests | Live 16-page/page-4 file replaced by distinct one-page rendition reopened at page 0 |
| Deleted book state is pruned | successful-scan GC | 200-record prune and close-rescan tests | Live delete published 29→28 and re-add returned 28→29 without stale progress |
| Failed/no-card scan does not prune | scan-result gating | invalid-index and scan-status prune tests | Injected failure boundary retained last valid index/state; physical removal cannot distinguish failure classes |
| One torn state slot recovers from the other | dual-slot store | every save-boundary fault test; corrupt-newest fallback test | Live newest slot truncation loaded generation 57, then republished valid generation 58 |
| Sleep-time save failure cancels sleep | shutdown coordinator | `state_save_failure_cancels_sleep_before_panel_shutdown_and_selects_notice` | Injected outcome proves failed ack, cancellation notice, and no panel/CPU sleep |
| Short POWER press does not sleep | `PowerHoldState`/GPIO3 | `separated_short_taps_remain_noops` | Physical short press retained USB/screen and emitted no sleep request |
| Two-second hold enters real deep sleep | power/display/RTC | hold timing and exact SSD1677 deep-sleep tests | Exact 246,336-byte image: physical three-second reading-page hold plus release made USB consistently absent |
| Initiating press is release-gated | power task | `deep_sleep_entry_is_blocked_until_the_initiating_hold_is_released` | USB disappearance checked after release of the initiating hold |
| Wake restores nonzero reading page | boot restore | reader-state/state-owner restore tests | GPIO wake logged cause 1, STATUS restored page 1/count 2, webcam showed matching `PAGE01/02` fixture |
| Wake restores menu selection | boot restore | filename-hash selection restore/prune tests | Exact staged-persistence image: one physical Down saved generation 101; deep sleep removed USB; GPIO wake cause 1 restored library mode and the same lower-row dither highlight in matched pre/post webcam frames |
| Corrupt/open/read errors remain recoverable | open-error/storage owner | failed-open/turn and routing tests | Live corrupt selection showed `CANNOT OPEN BOOK`; retry errored and Back recovered library |
| Diagnostic STATUS/logs reflect real state | runtime aggregator/protocol | snapshot, lifecycle, and unsequenced physical-commit tests | Live page counts/pages, ADC transitions, storage lifecycle, wake cause, pruning, and zero-error states independently queried |

## Completion Definition

This POC is complete only when:

1. all focused, workspace, feature-gated, Python, formatting, and target-build
   gates pass;
2. actual selected SD books render distinguishably on the X4;
3. per-book progress, deletion pruning, menu restore, reading restore, physical
   deep sleep, and POWER wake all have live evidence;
4. `HANDOFF.md` contains a fully populated acceptance matrix and distinguishes
   visual evidence from acknowledgements;
5. the final adversarial review cannot reproduce a no-op, placeholder response,
   stale embedded-book path, state-loss bug, or unverified completion claim.
