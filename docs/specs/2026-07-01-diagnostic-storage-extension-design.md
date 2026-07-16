# Diagnostic Storage Extension Design

Status: Implemented for the SD backend when firmware is built with
`diagnostic-console,sd-storage`. The LittleFS internal-flash backend remains
out of scope.

Correction note: the upload design writes directly to the final filename and
guarantees integrity via CRC32 verification at commit plus `binbook-storage`
validation. It does not rely on `embedded-sdmmc` pre-allocation or atomic rename.

## Status and authority

This is an approved design for Sub-project **B** of the SD-card library feature
(**A** SD storage foundation → **B** diagnostic storage extension → **C** library
menu + reading flow). B extends the existing X4 serial diagnostic protocol with
commands to **upload, list, delete, and read back** BinBook files on the SD
storage delivered by A. B is what makes A verifiable on real hardware and gives a
mechanism to populate the card for testing C.

`BINBOOK_FORMAT_SPEC.md` is authoritative for BinBook file bytes. The
`binbook-diagnostic-protocol` crate is authoritative for the current diagnostic
wire format.

Build order is **A → B → C**. B depends on A's crates (`embedded-sd-storage`,
`binbook-storage`) and the `binbook-fw` shared-bus arbiter.

## Pre-change protocol facts

- COBS-framed binary, `MAGIC = "BB"`, per-frame CRC, frame delimiter `0x00`.
- `RawFrameHeader { kind: u8, opcode: u8, status: u8, sequence: u16, payload_len: u16 }`.
- `FrameKind`: Request(1), Response(2), Event(3).
- `Status`: Ok, Error, BadRequest, NotFound, InternalError.
- `Opcode` today: Hello(0x01), Key(0x02), Page(0x03), Status(0x04), LogGet(0x05),
  LogClear(0x06), CrashGet(0x07), CrashClear(0x08), DisplayProbe(0x09).
- `MAX_FRAME_BYTES = 512`, `MAX_PAYLOAD_BYTES = 496`.
- Capabilities are a `u32` bitmask advertised in the `Hello` response, which also
  returns `max_frame_bytes`.
- `binbook-fw::diag::dispatch_command` matches opcode → decodes payload → returns
  a `DispatchResult`.

Current code has protocol version 2, larger frames, storage opcodes,
`CAP_STORAGE`, and `Status::Unsupported`; use
`firmware/crates/binbook-diagnostic-protocol` for exact values.

## Goals

- Upload a `.binbook` from host to the SD card over serial, with integrity
  verification and corruption-safe semantics (incomplete uploads never appear as
  valid books).
- List, delete, and read back BinBook files on the card over serial.
- Make the storage backend explicitly addressable so a future internal-flash
  backend (LittleFS) can be added without churning the protocol again.
- Keep B's scope to **SD only**; reserve the backend selector for forward
  compatibility.
- Provide a host CLI (`binbook diag storage ...`) that drives the whole CRUD path
  and produces independent verification evidence.

## Diagnostic-storage scope boundary

- The internal-flash (LittleFS) backend itself — **roadmap item** (see end of
  doc). B reserves the `backend` selector and implements `sd` only.
- The library menu and reading flow are implemented by the reader runtime, not
  by diagnostic storage.
- Diagnostic storage defines no open-book ownership concept. The reader drops
  its active `Book` before rescanning the library on close.

## Current owner-backed implementation

The reader, state store, library refresh, and diagnostic console all submit
typed requests to the single mounted FAT owner. Diagnostic commands must not
mount, enumerate, or borrow the filesystem independently.

- `StoreList` pages through the selected, validated A/B library index (or the
  version-1 migration fallback); it does not start a competing directory scan.
  The CLI advances a `start_index` cursor until the device returns an empty
  page.
- List records preserve portable names up to 121 bytes. `page_count` is zero
  because the persistent index does not store BinBook page metadata.
- `StoreRead` uses bounded owner reads and the CLI streams directly to its
  output. Consecutive reads reuse the owner's cached file size. A host may keep
  exactly two ascending read requests in flight only when HELLO advertises
  `CAP_STORAGE_READ_WINDOW_2`; capability absence requires stop-and-wait.
- Upload begin/write/commit uses exact ascending offsets, size checking, and
  whole-file CRC32. `storage begin` and `storage abort` expose cleanup of an
  incomplete upload for diagnostics.
- Successful commit, delete, or re-add schedules a cooperative background
  library refresh. The last valid index remains visible until replacement scan
  and save finish.
- An in-scan owner failure is cancelled before another refresh begins so a
  stale scan token cannot make every later `ScanStart` return busy.

## Protocol changes (`binbook-diagnostic-protocol`)

- **Frame size bump:** `MAX_PAYLOAD_BYTES` → **4096** (`MAX_FRAME_BYTES` = 4126);
  `PROTOCOL_VERSION` → 2. `Hello` already advertises `max_frame_bytes`, so the
  host CLI adapts automatically. This benefits `LogGet` too. Every frame-sized
  firmware buffer remains explicit and is included in linked-RAM inspection.
- **New capability bit:** `CAP_STORAGE` (next free bit, advertised in `Hello`).
- **New status:** `Unsupported` (used when a `backend` value is valid but not
  implemented, e.g. `flash` today).
- **New opcodes** (next free values after `DisplayProbe = 0x09`): `StoreList`,
  `StoreUploadBegin`, `StoreUploadWrite`, `StoreUploadCommit`, `StoreDelete`,
  `StoreRead`, and optional `StoreAbort`.

Every storage opcode carries a **`backend` enum** (`sd` | `flash`) as the first
payload byte, reserved for forward compatibility. Only `sd` is implemented; a
`flash` request returns `Status::Unsupported`.

## Upload flow (stateless offset, write-to-final-name, CRC-verify — no rename)

> **Amended.** `embedded-sdmmc` exposes no `rename` and no `truncate`/pre-
> allocation. The original "pre-allocate + rename `.partial`→final" is therefore
> replaced. Integrity is guaranteed without rename.

Chosen over session-streaming for resumability and robustness. Parallel SD
writes are not permitted (the serial SD/FAT owner remains sequential), but the
host may overlap transport latency by sending two complete write frames before
waiting for their acknowledgements. Firmware advertises this exact bound with
`CAP_STORAGE_UPLOAD_WINDOW_2`; peers without that capability remain strictly
request/response. The negotiated window does not change protocol version or
payload layouts.

1. `StoreUploadBegin { backend, name, total_size, expected_crc32 }` → firmware
   **creates the file at its final name** (`Mode::ReadWriteCreate`) and caches one
   write-handle keyed by `name` to avoid per-chunk open/close. No pre-allocation
   (not supported). Returns `Ok` / `NotFound`(no card) / `BadRequest`(bad name /
   too large / already exists / another upload in flight).
2. `StoreUploadWrite { backend, name, offset, data[] }` → idempotent; seeks to
   `offset` and writes. The host sends chunks in **ascending offset order**
   (sequential append semantics); arbitrary out-of-order writes are not supported
   (FAT would zero-fill gaps). Re-sending an already-written offset overwrites
   identically. The host uses 3,584-byte aligned data chunks and, only when the
   capability is advertised, sends exactly two ascending frames at a time.
   Both response sequences, statuses, and accepted lengths are correlated
   before progress advances. If either response fails, the host collects the
   outstanding response, issues no new write, and attempts `StoreAbort`.
3. `StoreUploadCommit { backend, name, expected_crc32 }` → flush, read back all
   written bytes, compute CRC32, and compare to `expected_crc32`. On match the
   file is complete and valid. On mismatch the response carries
   `result=CrcMismatch` + `computed_crc32`; firmware does not execute the owner
   commit or publish the file. The upload session remains available for the
   host's required `StoreAbort` cleanup.
4. `StoreAbort { upload_id }` → delete the incomplete file and drop the owner
   upload session.

A per-frame CRC already protects transport; the final whole-file CRC32 is the
application-level integrity check, and `StoreRead` back to the host is the
independent byte-identity proof (see Verification).

## `StoreList`

Returns entries from `/books/`. Each entry is **header-only and cheap**:
`{ name, file_size, page_count }`. Because enumeration validates via `Book::open`,
only complete, valid BinBooks appear — any half-uploaded or corrupt file is
omitted automatically. `page_count` comes from the BinBook section table read
during validation. Full `BookMetadata` (title/author) is a deeper read and is
**not** included; fetch on demand if C's menu later wants titles.

## Storage layout and filename rules

- **Layout:** BinBook files live in a **`/books/`** subdirectory on the FAT
  volume — keeps the card clean when mounted on a PC. (Root is not used.)
- **Filename rules:** enforce `.binbook` extension; long filenames (LFN) enabled;
  at most 121 UTF-8 bytes; reject path separators (flat `/books/` only).

## `StoreDelete`

`StoreDelete { backend, name }` → FAT remove. Reject if an upload of the same
`name` is in flight. This is also how a host cleans up a file left by a failed
upload (it won't appear in `StoreList`, but `StoreDelete` removes it by name).

## `StoreRead` (verification backbone)

`StoreRead { backend, name, offset, max_bytes }` → `{ data[], eof }`, paginated
like `LogGet`. Lets the host independently fetch a stored file's bytes and
confirm it is byte-identical to the source — the completion-evidence backbone.
The host never buffers the complete file. With `CAP_STORAGE_READ_WINDOW_2`, it
sends two consecutive ranges before collecting their correlated responses. It
validates opcode, sequence, status, returned length, and EOF before writing the
bytes. A speculative second response after the first range reports EOF is
discarded. Partial output is removed on failure. Peers without the capability
remain strictly request/response compatible.

## Host CLI

Storage inspection and cleanup remain under the existing `diag` command:

```
binbook diag storage {list,delete,read,begin,abort} ...
```

The current CLI targets SD. Top-level `binbook upload --file <path>` reads a
local `.binbook` and pushes it as a bounded ascending stream. `list` prints
entries, `delete` removes one, and `read` fetches bytes for independent
verification. Full upload and full readback accept
`--transfer-mode auto|legacy|bulk`, `--window auto|1|2|4|8`, and
`--transfer-report <path>`. A range read selected by `--offset` or
`--max-bytes` rejects those transfer-only flags. The CLI reads negotiated limits
and never selects a mode or window solely from protocol version.

## Crate wiring (`binbook-fw`)

The async diagnostic runtime routes decoded storage frames through the mounted
storage-owner request channel. The owner is the sole FAT and block-device
accessor. One write session may be active; commit or abort releases it.

## Concurrency / bus

All SD access goes through A's shared-bus mutex with display-gating. Upload
writes are serialized through the arbiter (SD access granted only when the
display is not mid-refresh). The cached write-handle is owned by the diagnostic
task; no cross-task sharing in B.

## Testing and verification

- **Host unit tests (no hardware):**
  - `binbook-diagnostic-protocol`: codec round-trips for every new opcode at the
    4096-byte frame size; `backend` enum encoding; `Unsupported` status; COBS at
    the larger frame size.
  - `binbook-fw` dispatch: new opcodes against a **mock `Filesystem`** (writable),
    asserting correct `DispatchResult` variants and error statuses (bad name,
    too large, crc mismatch, delete-in-flight, no card, upload-in-flight).
- **Hardware evidence gate — the whole point of B** — uses the real
  `nav_probe.binbook` fixture end to end, from a verified-empty state:
  1. Format card FAT, mount → ensure **`/books/` is empty**.
  2. `storage list --device sd` → assert **empty list** (proves `StoreList` works
     on an empty directory, not a no-op).
  3. `storage upload nav_probe.binbook` → `StoreUploadCommit` returns CRC-Ok.
  4. `storage list` → assert **exactly one entry: `nav_probe.binbook`** with
     matching `file_size` and `page_count`.
  5. `storage read nav_probe.binbook` (paginated to EOF) → host confirms
     **byte-identical** to the source (sha256 match), and CRC32 matches.
  6. `storage delete nav_probe.binbook` → `storage list` → assert **empty again**
     (proves delete and that the round-trip was real, not a leftover).
  7. Incomplete-upload invisibility: interrupt an upload mid-stream (power-cycle
     or `StoreAbort`), then `storage list` → assert the partial file does **not**
     appear (validation makes it invisible).
  8. Bridge into C's visual gate: re-upload, then decode page 0 through the
     display path and **webcam**-inspect it.

Capture the full serial transcript as evidence. Every CRUD path must be exercised
with discriminating preconditions, per the completion-evidence discipline.

## Negotiated transport behavior

Protocol v2 retains the existing payload layouts. `HELLO` advertises
`CAP_STORAGE` for storage commands, `CAP_STORAGE_UPLOAD_WINDOW_2` for the
bounded upload window, and `CAP_STORAGE_READ_WINDOW_2` for bounded readback.
Capability absence is authoritative: a host sends only one request before
waiting even when the peer reports protocol v2. Per-frame CRC guards transport,
exact accepted lengths advance acknowledged progress, and the commit CRC plus
independent complete readback remain the whole-file integrity decision.

### Phase 2 transfer negotiation and observability

The protocol crate reserves and implements these capability bits:

- bit 10, `CAP_STORAGE_TRANSFER_LIMITS`;
- bit 11, `CAP_STORAGE_TRANSFER_METRICS`;
- bit 12, `CAP_STORAGE_BULK_V1`.

The current production capability set advertises transfer limits and metrics.
It deliberately withholds `CAP_STORAGE_BULK_V1` until the four-slot runtime has
passed its live RAM, cleanup, integrity, and zero-error checkpoint. New hosts
therefore continue selecting the legacy path automatically against the current
firmware even though bulk codecs and runtime state machines are compiled in.
Forced `--transfer-mode bulk` fails clearly when the capability is absent.

`StoreTransferLimits` (`0x12`) returns schema 1 with maximum data bytes, legacy
upload/read windows, bulk slot count, measured RX capacity, maximum frame size,
and idle timeout. Reserved bytes and unknown flags are rejected. A host clamps
its requested window to these limits and reports the actual selected value.

`StoreTransferTrace` (`0x13`) starts, retrieves, and clears one bounded metrics
aggregate. It separates USB RX/TX bytes, framing, frame CRC, owner queue and
execution, physical SD reads/writes, payload CRC, commit, and library refresh.
It also records overlap, RX/slot high-water marks, queue stalls, and
instrumentation anomalies. Legacy success, failure, and abort paths terminalize
the trace; a rejected legacy read cannot leave the trace slot active. Metrics
are diagnostic aggregates and do not create per-frame log growth.

### Bulk streaming protocol

Bulk v1 uses opcodes `StoreBulkOpen` (`0x14`), `StoreBulkData` (`0x15`),
`StoreBulkCredit` (`0x16`), `StoreBulkStatus` (`0x17`), `StoreBulkFinish`
(`0x18`), and `StoreBulkAbort` (`0x19`). Every new opcode is sent only when
`CAP_STORAGE_BULK_V1` is advertised; otherwise the exact legacy protocol is the
fallback.

`StoreBulkOpen` negotiates direction, chunk size, and window and returns a
nonzero stream ID, selected chunk, initial credits, durable offset, and total
size. Upload data events carry exact ascending offsets. Firmware accepts them
only into one shared pool of four caller-owned 3,584-byte slots and queues slot
descriptors—not payload copies—to the sole storage owner. Cumulative upload
credits are emitted only after correlated SD-write completion, so host progress
never advances beyond device-confirmed durable bytes.

For readback, firmware retains a slot until its data event has been sent and the
host returns a credit for bytes successfully written to the output. The host
flushes and truncates to the last acknowledged offset at a status barrier before
granting further credits. Partial output is removed on unrecoverable failure.

Status is the recovery boundary after a lost event or timeout. It reports the
exact stream, direction, state, available credits, durable offset, total size,
and running CRC. The host discards uncertain upload packets or rolls back its
readback output to that offset; it never blindly retries a mutating frame.
Finish validates exact size and CRC. Abort invalidates the generation, uses the
priority owner-control lane for upload cleanup, reclaims stale leases by owner
sequence, and does not release the shared pool until the abort acknowledgement
and every outstanding lease return. A named 60-second idle timeout invokes the
same cleanup. Control/status/abort handling remains serviceable when all data
slots are occupied.

The runtime has one physical four-slot pool shared sequentially between upload
and readback, an eight-entry owner-operation bound, and fixed-capacity reply
metadata. Reply saturation produces an explicit error while any operation
already admitted by the coordinator still drains, preventing stranded storage
state. The linked diagnostic image currently retains 72,704 bytes of DRAM
headroom. This is build evidence, not authorization to advertise bulk or a live
throughput claim.

### Transfer progress and reports

TTY progress names preparation, upload/readback, CRC verification, publication,
and library-update phases. It shows durable bytes, percentage, rolling rate,
elapsed time, and ETA. `--progress auto|always|never` preserves non-TTY behavior.

`--transfer-report <path>` atomically writes schema-version 1 JSON after a full
upload or readback. It records requested/selected mode, exact limits, host phase
durations, rate, recovery counters, CRC/result fields, cleanup state, protocol
error delta, and the firmware trace when supported. Post-negotiation failures
also write a report. Preflight failures do not fabricate a selected mode or
limits; stderr is authoritative before negotiation succeeds.

---

## Future internal-flash backend

The backend enum reserves `flash`, but the CLI and firmware currently implement
SD only. A future LittleFS implementation would need explicit flash-layout,
wear-leveling, capacity, erase/program, and crash-sector analysis before this
protocol path could be advertised. It is not part of the serial-throughput
phase.
