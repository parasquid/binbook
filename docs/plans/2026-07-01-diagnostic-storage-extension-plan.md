# Diagnostic Storage Extension (Sub-project B) Implementation Plan

> **Location note:** Authored under `.omo/plans/` (planning agent sandbox). Belongs
> at `docs/plans/2026-07-01-diagnostic-storage-extension-plan.md`. Move before
> execution.

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the X4 serial diagnostic protocol with `storage upload / list /
delete / read` commands over the SD storage from Sub-project A, plus a host CLI,
so BinBook files can be pushed to and verified on the card — producing the first
real-hardware evidence for the whole storage stack.

**Architecture:** Protocol v2 in `binbook-diagnostic-protocol` (4096-byte frames,
`CAP_STORAGE`, new storage opcodes carrying a reserved `backend` enum, new
`Unsupported` status). `binbook-fw` dispatch routes the new opcodes to
`binbook-storage` / `embedded-sd-storage` over A's shared bus. A host CLI
(`binbook diag storage --device sd …`) drives CRUD with pipelined uploads. The
hardware gate is an end-to-end `nav_probe.binbook` round-trip.

**Tech Stack:** Rust (`no_std` firmware + `std` host CLI), `binbook-diagnostic-protocol`,
`embedded-sdmmc` 0.9.0, `binbook-storage` + `embedded-sd-storage` (from A),
`clap` (host CLI), a `no_std` CRC32.

**Authoritative refs:** spec `docs/specs/2026-07-01-diagnostic-storage-extension-design.md`;
`firmware/crates/binbook-diagnostic-protocol/src/lib.rs` (existing frame/opcode
layout); A's crates and `binbook-fw` shared bus.

**Depends on:** Sub-project **A** delivered (`embedded-sd-storage`, `binbook-storage`,
the `binbook-fw` SD mount + shared SPI2 bus, and the `Filesystem` adapter).

---

## Key corrections vs the spec (READ FIRST)

The `embedded-sdmmc` API (confirmed during A's research) does **not** expose
`rename` or `truncate`-to-size / pre-allocation. Spec B assumed both ("pre-
allocate `total_size`", "rename `.partial` → final"). This plan adjusts, without
changing the user-facing contract:

- **No rename** → upload writes **directly to the final filename** (no `.partial`).
  Integrity is guaranteed two ways: (1) CRC32 verify at `StoreUploadCommit`, and
  (2) A's `enumerate_binbooks` validates each file via `Book::open`, so an
  incomplete/corrupt upload is **invisible to `StoreList`** (and to the menu in C).
  A leftover corrupt file can be removed with `StoreDelete`. This is documented as
  a deliberate deviation; "atomic-ish" is achieved via validation, not rename.
- **No pre-allocate** → `StoreUploadBegin` creates the file (`Mode::ReadWriteCreate`);
  `StoreUploadWrite` does seek-to-offset + write. The host sends chunks in
  **ascending offset order** (sequential append semantics). The stateless model
  keeps its virtues (idempotent re-send, resumable from offset 0 or last-known),
  but arbitrary out-of-order writes are not supported (FAT/embedded-sdmmc would
  zero-fill gaps). Pipelining (N frames in flight) still applies.
- **CRC32 source:** add a `no_std` CRC32 (the `crc` crate with `crc32` + ISO-HDLC,
  or a 40-byte inline impl). This is a transfer-integrity check, not a BinBook
  format field. Byte-identity is proven independently by `StoreRead` + host diff.

If these corrections are unacceptable, stop and revise the spec before executing.

---

## File structure

**Modify:**
- `firmware/crates/binbook-diagnostic-protocol/src/lib.rs` (frame size, version,
  `CAP_STORAGE`, `Status::Unsupported`, `StorageBackend`, new `Opcode`s + payload
  codecs) + its `tests/codec.rs`
- `firmware/crates/binbook-fw/Cargo.toml` (deps: crc; feature `sd-storage` from A)
- `firmware/crates/binbook-fw/src/diag.rs` (`DispatchResult` storage variants)
- `firmware/crates/binbook-fw/src/runtime/diagnostic_console.rs`
  (`RuntimeCommand` storage variants + handlers)
- `firmware/crates/binbook-fw/src/storage.rs` (A's adapter; extend with
  write/delete + a cached upload handle)
- `crates/binbook/src/` (host CLI: new `storage` subcommand) + tests

**Responsibilities:** protocol codec is entirely in `binbook-diagnostic-protocol`
(host-testable). Dispatch + handlers in `binbook-fw` (host-testable against a mock
`Filesystem` that supports write/delete). Host CLI in the `binbook` crate.

---

## Task 1: Bump frame size + protocol version

**Files:** `firmware/crates/binbook-diagnostic-protocol/src/lib.rs`, its `tests/codec.rs`

- [ ] **Step 1: Write the failing test**

In `tests/codec.rs` add:

```rust
#[test]
fn round_trips_max_sized_payload() {
    use binbook_diagnostic_protocol::*;
    let payload = vec![0xA5u8; MAX_PAYLOAD_BYTES]; // 4096 after the bump
    let mut frame = vec![0u8; MAX_FRAME_BYTES];
    let n = encode_frame(FrameKind::Response, Opcode::Status, Status::Ok, 7, &payload, &mut frame).unwrap();
    let (header, decoded_payload) = decode_frame(&frame[..n]).unwrap();
    assert_eq!(header.kind, FrameKind::Response);
    assert_eq!(header.payload_len as usize, MAX_PAYLOAD_BYTES);
    assert_eq!(decoded_payload, &payload[..]);
}

#[test]
fn advertises_protocol_version_2() {
    assert_eq!(binbook_diagnostic_protocol::PROTOCOL_VERSION, 2);
}
```

> Use the real top-level `encode_frame`/`decode_frame` names from `lib.rs`
> (confirm exact names; the crate already packs COBS+CRC+header somewhere — use
> that path, not a new one).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p binbook-diagnostic-protocol --test codec`
Expected: FAIL (MAX_PAYLOAD_BYTES is 496; version is 1).

- [ ] **Step 3: Make the change**

In `src/lib.rs`:

```rust
pub const PROTOCOL_VERSION: u8 = 2;
pub const MAX_FRAME_BYTES: usize = 4112;   // 4096 payload + ~16 header/COBS/CRC slack
pub const MAX_PAYLOAD_BYTES: usize = 4096;
```

(Compute the exact `MAX_FRAME_BYTES` from the existing header + COBS worst-case +
CRC. If any static `[u8; MAX_FRAME_BYTES]` buffers exist in `binbook-fw`, they
grow automatically; audit for any smaller hardcoded frame buffers and bump them.)

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p binbook-diagnostic-protocol`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add firmware/crates/binbook-diagnostic-protocol
git commit -m "feat(diag-protocol): bump to v2 with 4096-byte payload frames"
```

---

## Task 2: Add `StorageBackend`, `Unsupported` status, `CAP_STORAGE`, new opcodes

**Files:** `firmware/crates/binbook-diagnostic-protocol/src/lib.rs`, `tests/codec.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn storage_backend_round_trips() {
    use binbook_diagnostic_protocol::{StorageBackend, Status};
    assert_eq!(StorageBackend::from_u8(0), Some(StorageBackend::Sd));
    assert_eq!(StorageBackend::from_u8(1), Some(StorageBackend::Flash));
    assert_eq!(Status::Unsupported, Status::from_u8(5).unwrap());
    assert!(binbook_diagnostic_protocol::CAP_STORAGE != 0);
}
```

- [ ] **Step 2: Run to verify it fails** → `cargo test -p binbook-diagnostic-protocol --test codec` (FAIL: unresolved).

- [ ] **Step 3: Implement**

In `src/lib.rs`:

```rust
pub const CAP_STORAGE: u32 = 1 << 6; // next free bit after CAP_DISPLAY_PROBE (1<<5)
// add CAP_STORAGE to ALL_CAPABILITIES

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    Ok = 0,
    Error = 1,
    BadRequest = 2,
    NotFound = 3,
    InternalError = 4,
    Unsupported = 5,
}
// extend from_u8 with 5 => Unsupported

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StorageBackend {
    Sd = 0,
    Flash = 1,
}
impl StorageBackend {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v { 0 => Some(Self::Sd), 1 => Some(Self::Flash), _ => None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    Hello = 0x01, Key = 0x02, Page = 0x03, Status = 0x04,
    LogGet = 0x05, LogClear = 0x06, CrashGet = 0x07, CrashClear = 0x08,
    DisplayProbe = 0x09,
    StoreList = 0x0A,
    StoreUploadBegin = 0x0B,
    StoreUploadWrite = 0x0C,
    StoreUploadCommit = 0x0D,
    StoreAbort = 0x0E,
    StoreDelete = 0x0F,
    StoreRead = 0x10,
}
// extend Opcode::from_u8 with the new arms
```

- [ ] **Step 4: Run to verify it passes** → `cargo test -p binbook-diagnostic-protocol` (PASS).

- [ ] **Step 5: Commit**

```bash
git add firmware/crates/binbook-diagnostic-protocol
git commit -m "feat(diag-protocol): add storage opcodes, backend enum, Unsupported status, CAP_STORAGE"
```

---

## Task 3: Payload codecs — `StoreList` and `StoreRead`

**Files:** `firmware/crates/binbook-diagnostic-protocol/src/lib.rs`, `tests/codec.rs`

Layouts (all little-endian; every storage payload starts with a `backend: u8`):

- `StoreList` request: `[backend: u8]`. Response: `[entry_count: u16 LE]` then
  `entry_count` × `{ name_len: u8, name[name_len], file_size: u64 LE, page_count: u32 LE }`.
  If the response would exceed `MAX_PAYLOAD_BYTES`, truncate entries and append a
  final `has_more: u8` flag byte (host paginates with an offset cursor `u32 LE`
  prepended to the request when needed — add `cursor: u32` to the request).
- `StoreRead` request: `[backend: u8, name_len: u8, name[..], offset: u64 LE, max_bytes: u32 LE]`.
  Response: `[bytes_read: u32 LE, eof: u8, data[..]]`.

- [ ] **Step 1: Write failing round-trip tests** for `encode_store_list_request`,
  `decode_store_list_request`, `encode_store_list_response` (with truncation +
  `has_more`), `decode_store_list_response`, and the `StoreRead` pair. Assert a
  2-entry list round-trips and that an oversized list sets `has_more`.

- [ ] **Step 2: Run to verify it fails.**

- [ ] **Step 3: Implement** the eight functions following the existing
  `encode_*`/`decode_*` style (return `Result<usize, ProtocolError>` /
  `Result<T, ProtocolError>`, bounds-check every field, reject names > 64 or with
  path separators via `ProtocolError::InvalidValue`).

- [ ] **Step 4: Run to verify it passes** → `cargo test -p binbook-diagnostic-protocol`.

- [ ] **Step 5: Commit**

```bash
git add firmware/crates/binbook-diagnostic-protocol
git commit -m "feat(diag-protocol): StoreList/StoreRead payload codecs"
```

---

## Task 4: Payload codecs — upload (`Begin`/`Write`/`Commit`/`Abort`) + `Delete`

**Files:** `firmware/crates/binbook-diagnostic-protocol/src/lib.rs`, `tests/codec.rs`

Layouts:

- `StoreUploadBegin`: `[backend: u8, name_len: u8, name[..], total_size: u64 LE, expected_crc32: u32 LE]`.
- `StoreUploadWrite`: `[backend: u8, name_len: u8, name[..], offset: u64 LE, data[..]]`
  (`data` fills the rest of the payload; the frame's `payload_len` fixes its length).
- `StoreUploadCommit`: `[backend: u8, name_len: u8, name[..], expected_crc32: u32 LE]`.
- `StoreAbort`: `[backend: u8, name_len: u8, name[..]]`.
- `StoreDelete`: `[backend: u8, name_len: u8, name[..]]`.

Commit response carries a `result: u8` (`0 = Ok`, `1 = CrcMismatch`, `2 = TooLarge`,
`3 = NotFound`) plus `computed_crc32: u32 LE` so the host can log the mismatch.

- [ ] **Step 1–4:** failing round-trip tests → implement → pass. Include a test
  that `StoreUploadWrite` with `MAX_PAYLOAD_BYTES`-sized `data` round-trips and
  that oversize names are rejected.

- [ ] **Step 5: Commit**

```bash
git add firmware/crates/binbook-diagnostic-protocol
git commit -m "feat(diag-protocol): storage upload/delete payload codecs"
```

---

## Task 5: `binbook-fw` — dispatch plumbing + `StorageHandle`

**Files:** `firmware/crates/binbook-fw/src/diag.rs`, `src/runtime/diagnostic_console.rs`, `src/storage.rs`, `Cargo.toml`

- [ ] **Step 1: Extend `DispatchResult`** (`src/diag.rs`) with storage variants:

```rust
pub enum DispatchResult {
    Response { status: Status, payload_len: usize },
    RenderTurn { turn: crate::input::PageTurn },
    RenderPage { target_page: u32 },
    NoAction,
    DisplayProbe(crate::async_refresh::DisplayProbeKind),
    LogGet { cursor: u32, max_bytes: u16 },
    LogClear,
    CrashGet,
    CrashClear,
    StorageList { backend: StorageBackend, cursor: u32 },
    StorageRead { backend: StorageBackend, name: heapless::String<64>, offset: u64, max_bytes: u32 },
    StorageUploadBegin { backend: StorageBackend, name: heapless::String<64>, total_size: u64, expected_crc32: u32 },
    StorageUploadWrite { backend: StorageBackend, name: heapless::String<64>, offset: u64 },
    StorageUploadCommit { backend: StorageBackend, name: heapless::String<64>, expected_crc32: u32 },
    StorageAbort { backend: StorageBackend, name: heapless::String<64> },
    StorageDelete { backend: StorageBackend, name: heapless::String<64> },
}
```

Add `StorageBackend` re-export + `heapless` dep to `binbook-fw`.

- [ ] **Step 2: Add opcode arms in `dispatch_command`** that decode each storage
  payload and return the matching `DispatchResult`. `backend == Flash` →
  `Response { status: Status::Unsupported, payload_len: 0 }` immediately. Decode
  failures → `Status::BadRequest`.

- [ ] **Step 3: Plumb `StorageHandle`** — a handle to A's mounted SD `Filesystem`
  (the `embedded-sd-storage`/`binbook-storage` mount from A's Task 8), shared
  between the diagnostic console task and (later) the reader. Put it behind a
  `static` `Channel`/`Mutex` analogous to the existing `AGGREGATOR_*` channels,
  gated by `#[cfg(feature = "sd-storage")]`. If A's mount failed at boot (no
  card), storage dispatch returns `Status::NotFound`/`InternalError`.

- [ ] **Step 4: Host test — dispatch decoding** (`tests/diagnostic_storage.rs`):
  feed frames for each storage opcode (good + bad: unknown backend, oversize
  name, corrupt length) and assert the right `DispatchResult` / `BadRequest` /
  `Unsupported`. Use a stub `CommandContext`.

- [ ] **Step 5: Run** → `cargo test -p binbook-fw --features diagnostic-console,sd-storage` (PASS).

- [ ] **Step 6: Commit**

```bash
git add firmware/crates/binbook-fw
git commit -m "feat(binbook-fw): dispatch plumbing for storage opcodes"
```

---

## Task 6: `binbook-fw` — `StoreList`, `StoreRead`, `StoreDelete` handlers

**Files:** `src/runtime/diagnostic_console.rs`, `src/storage.rs`, tests

- [ ] **Step 1: Extend A's `Filesystem` trait with write/delete** (or add a
  separate `WritableFilesystem` trait in `binbook-storage` that `SdStorage`
  implements): `create_file(name)`, `write_at(name, offset, &[u8])`,
  `delete_file(name)`, `file_exists(name)`. Keep `binbook-storage` `no_std`.

- [ ] **Step 2: Host test with a writable `MemoryFs`** — `tests/storage_handlers.rs`:
  seed `MemoryFs` with one book; assert `StoreList` returns it; `StoreRead`
  returns exact bytes at an offset; `StoreDelete` removes it and a subsequent
  `StoreList` is empty.

- [ ] **Step 3: Implement handlers** delegating to the `StorageHandle`'s
  `Filesystem`. `StoreList` uses `binbook_storage::enumerate_binbooks` (validated)
  and encodes via Task 3's codec. `StoreRead` reads `max_bytes` at `offset`
  (paginated by host until `eof`). `StoreDelete` calls `delete_file`.

- [ ] **Step 4: Run** → `cargo test -p binbook-fw --features diagnostic-console,sd-storage` (PASS).

- [ ] **Step 5: Commit**

```bash
git add firmware/crates/binbook-fw crates/binbook-storage
git commit -m "feat(binbook-fw): StoreList/StoreRead/StoreDelete handlers"
```

---

## Task 7: `binbook-fw` — upload handler (no rename, no pre-allocate)

**Files:** `src/runtime/diagnostic_console.rs`, `src/storage.rs`, tests

- [ ] **Step 1: Host test for the upload state machine** — `tests/storage_upload.rs`
  against `MemoryFs`: `Begin{name,total,crc}` creates the file; a sequence of
  `Write{offset,data}` (ascending) writes bytes; `Commit{expected_crc}` computes
  CRC32 of written bytes and returns `Ok` when they match the source, `CrcMismatch`
  otherwise; `Abort` deletes. Assert an interrupted upload (no commit) leaves a
  file that `enumerate_binbooks` rejects (fails `Book::open` → invisible to list).

- [ ] **Step 2: Implement** against the `WritableFilesystem`:
  - `Begin`: cache one open upload context `{ name, written: u64, expected_crc32 }`
    keyed by name (single active upload; reject a second `Begin` while one is open
    → `Status::BadRequest`). `create_file(name)` (ReadWriteCreate). No
    pre-allocation (not supported).
  - `Write`: `write_at(name, offset, data)`; reject if `offset` is beyond
    `written` (out-of-order not supported — host sends ascending). Idempotent: a
    re-send of an already-written offset overwrites identically.
  - `Commit`: read back all bytes, compute CRC32, compare; on match flush +
    `Response{result: Ok}`; on mismatch `Response{result: CrcMismatch,
    computed_crc32}`.
  - `Abort`: `delete_file(name)` + drop the cached context.

- [ ] **Step 3: Add CRC32** (`crc` crate, `no_std`, ISO-HDLC polynomial — same as
  the Python `crc32` used by `binbook inspect`-adjacent tooling, so host and
  firmware agree). Pin in `Cargo.toml`.

- [ ] **Step 4: Run** → `cargo test -p binbook-fw --features diagnostic-console,sd-storage` (PASS).

- [ ] **Step 5: Commit**

```bash
git add firmware/crates/binbook-fw Cargo.toml
git commit -m "feat(binbook-fw): storage upload (stateless in-order, crc-verify, no rename)"
```

---

## Task 8: Host CLI — `binbook diag storage`

**Files:** `crates/binbook/src/diag_storage.rs`, `crates/binbook/src/diag_protocol.rs`, `Cargo.toml`, tests

- [ ] **Step 1: Add the subcommand** under the existing `diag` clap parser:
  `storage --device <sd|flash> <upload|list|delete|read> …` with `--device`
  defaulting to `sd`. `upload <path>`; `list [--cursor N]`; `delete <name>`;
  `read <name> [--offset N --max-bytes N]`.

- [ ] **Step 2: Host test against a loopback/fake serial** (or a pure
  request-builder unit test): assert `upload` frames a file as ascending
  `StoreUploadWrite` chunks of `MAX_PAYLOAD_BYTES` read from `Hello`, with
  **pipelining** (N frames in flight before draining acks), and verifies the
  commit `Ok`. Assert `list` decodes the response and `read` reassembles to EOF.

- [ ] **Step 3: Implement** the serial transport reuse (existing
  `serial_transport.rs`) + pipelining window (start with N=4, tune later).

- [ ] **Step 4: Run** → `cargo test -p binbook` (PASS).

- [ ] **Step 5: Commit**

```bash
git add crates/binbook
git commit -m "feat(binbook): diag storage upload/list/delete/read CLI with pipelining"
```

---

## Task 9: Hardware gate — `nav_probe.binbook` end-to-end

This is the **completion-evidence gate** for the whole A+B stack. Run on live
hardware with serial + (for the bridge to C) webcam. Capture the full transcript.

- [ ] **Step 1: Prepare a clean card** — format FAT32, mount, confirm `/books/`
  is empty.

- [ ] **Step 2: Empty list** — `binbook diag storage list --device sd` → assert
  an empty entry list (proves `StoreList` works on an empty dir, not a no-op).

- [ ] **Step 3: Upload** — `binbook diag storage upload target/debug/.../nav_probe.binbook`
  (or the committed fixture path) → `StoreUploadCommit` returns `result=Ok`.

- [ ] **Step 4: List shows it** — `storage list` → exactly one entry
  `nav_probe.binbook` with matching `file_size` and `page_count`.

- [ ] **Step 5: Read back is byte-identical** — `storage read nav_probe.binbook`
  (paginated to EOF) → host concatenates and asserts `sha256 == sha256(source)`
  (and CRC32 matches). This is the independent byte-identity proof.

- [ ] **Step 6: Delete + empty** — `storage delete nav_probe.binbook` → `storage
  list` → assert empty again.

- [ ] **Step 7: Bridge to C (webcam)** — re-upload, then trigger a page-0 decode
  through the display path and **webcam**-inspect that the rendered page matches
  `binbook decode nav_probe.binbook --page 0` (this is the visual gate C will build on).

- [ ] **Step 8: Record evidence** in `HANDOFF.md` — exact commands, serial
  transcript excerpts, the sha256 match, and the webcam frame.

---

## Task 10: Full workspace gate + HANDOFF + roadmap entry

- [ ] **Step 1: Host gates**

```bash
cargo test --workspace
cargo test -p binbook-fw --features diagnostic-console,sd-storage
cargo test -p binbook --features serial-device
```
Expected: all PASS.

- [ ] **Step 2: Firmware build**

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly \
  cargo build -p binbook-fw --features firmware-bin,diagnostic-console,sd-storage \
  --target riscv32imc-unknown-none-elf --release
```
Expected: builds.

- [ ] **Step 3: Write `HANDOFF.md`** — verified (codec tests; dispatch handler
  tests with mock `Filesystem`; CLI tests; the Task 9 hardware round-trip with
  transcript + sha256 + webcam); transport-only/unverified (none); known
  limitations (no rename/pre-allocate — incomplete uploads invisible via
  validation; single active upload; in-order chunks only). Mark storage
  **byte-level verified** only after Task 9 Step 5 passes on hardware.

- [ ] **Step 4: Move the LittleFS roadmap entry** from spec B into
  `docs/ROADMAP.md` (it is appended to the spec under "Roadmap entry to add to
  `docs/ROADMAP.md`").

- [ ] **Step 5: Commit**

```bash
git add HANDOFF.md docs/ROADMAP.md
git commit -m "docs: handoff + roadmap for diagnostic storage extension (sub-project B)"
```

---

## Self-review (spec coverage)

- **new opcodes / capability / version / Unsupported / backend enum** → Tasks 1–2. ✓
- **frame size 4096** → Task 1. ✓
- **upload: stateless offset, pre-allocate, crc, rename, pipelining** → Task 7
  (with documented no-rename/no-pre-allocate corrections) + Task 8 pipelining. ✓
  (deviation flagged at top)
- **StoreList cheap (name+size+page_count), `/books/`, filename rules** → Task 6
  + codecs Task 3 (name ≤64, no separators). ✓
- **StoreDelete / StoreRead paginated** → Tasks 3, 6. ✓
- **host CLI noun `storage`, `--device` defaults sd** → Task 8. ✓
- **backend selector reserved for LittleFS** → Tasks 2, 5 (`Flash` → Unsupported). ✓
- **nav_probe end-to-end HW gate (empty→upload→list→read→delete→empty)** → Task 9. ✓
- **crate wiring in binbook-fw dispatch** → Task 5. ✓

**Type consistency:** `StorageBackend`, `Opcode` new arms, `DispatchResult`
storage variants, and the codec function names match across Tasks 2–7. The
`WritableFilesystem` trait (Task 6 Step 1) is consumed by Task 7. CRC32 source is
fixed (Task 7 Step 3) and shared by host (Task 8) and firmware.

**No placeholders:** all code steps show code or follow an existing, named
pattern; the two `embedded-sdmmc` capability gaps (rename/pre-allocate) are
resolved with a concrete design, not deferred.

## Hardware gate (honesty)

B is the first **byte-level hardware-verified** milestone of the whole feature
(Task 9 Step 5: `StoreRead` sha256 == source). Do not mark B complete without that
sha256 match and the serial transcript on a live card. The webcam bridge (Step 7)
is the hand-off into C's visual gate.
