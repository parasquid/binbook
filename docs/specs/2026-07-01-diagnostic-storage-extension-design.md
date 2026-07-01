# Diagnostic Storage Extension Design — CORRECTED COPY

> **Corrected copy.** This replaces the committed
> `docs/specs/2026-07-01-diagnostic-storage-extension-design.md`. Move agent:
> overwrite the `docs/specs/` file with this one.
>
> **Correction applied:** the original upload design assumed `embedded-sdmmc`
> supported pre-allocation and an atomic rename (`.partial` → final). It supports
> **neither**. The upload flow is amended to write directly to the final filename
> and guarantee integrity via (a) CRC32 verification at commit and (b)
> `binbook-storage`'s `Book::open` validation (incomplete/corrupt files are
> invisible to `StoreList`). The user-facing contract is unchanged.

## Status and authority

This is an approved design for Sub-project **B** of the SD-card library feature
(**A** SD storage foundation → **B** diagnostic storage extension → **C** library
menu + reading flow). B extends the existing X4 serial diagnostic protocol with
commands to **upload, list, delete, and read back** BinBook files on the SD
storage delivered by A. B is what makes A verifiable on real hardware and gives a
mechanism to populate the card for testing C.

`BINBOOK_FORMAT_SPEC.md` and the existing `binbook-diagnostic-protocol` crate are
authoritative for the wire format. B extends the protocol (new opcodes, larger
frame, new capability, version bump) but does not change BinBook file bytes.

Build order is **A → B → C**. B depends on A's crates (`embedded-sd-storage`,
`binbook-storage`) and the `binbook-fw` shared-bus arbiter.

## Existing protocol facts (from `binbook-diagnostic-protocol`)

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

## Non-goals (deferred)

- The internal-flash (LittleFS) backend itself — **roadmap item** (see end of
  doc). B reserves the `backend` selector and implements `sd` only.
- The library menu and reading flow — Sub-project **C**.
- "Book is open in the reader" vs delete — there is no reader yet in B; the
  reader (C) will close-before-delete. B defines no open-book concept.

## Protocol changes (`binbook-diagnostic-protocol`)

- **Frame size bump:** `MAX_PAYLOAD_BYTES` → **4096** (`MAX_FRAME_BYTES` ~4120);
  `PROTOCOL_VERSION` → 2. `Hello` already advertises `max_frame_bytes`, so the
  host CLI adapts automatically. This benefits `LogGet` too. Static firmware frame
  buffer grows to ~4 KB (trivial on ESP32-C3).
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

Chosen over session-streaming for resumability and robustness. Parallel chunk
uploads do **not** improve throughput here (single serial pipe + serial SD over
shared SPI + FAT/embedded-sdmmc is not concurrency-safe for writes); the real
throughput lever is **host-side pipelining** (send *N* frames before waiting for
acks), which composes cleanly with the stateless model.

1. `StoreUploadBegin { backend, name, total_size, expected_crc32 }` → firmware
   **creates the file at its final name** (`Mode::ReadWriteCreate`) and caches one
   write-handle keyed by `name` to avoid per-chunk open/close. No pre-allocation
   (not supported). Returns `Ok` / `NotFound`(no card) / `BadRequest`(bad name /
   too large / already exists / another upload in flight).
2. `StoreUploadWrite { backend, name, offset, data[] }` → idempotent; seeks to
   `offset` and writes. The host sends chunks in **ascending offset order**
   (sequential append semantics); arbitrary out-of-order writes are not supported
   (FAT would zero-fill gaps). Re-sending an already-written offset overwrites
   identically. Host pipelines multiple `StoreUploadWrite` frames.
3. `StoreUploadCommit { backend, name, expected_crc32 }` → flush, read back all
   written bytes, compute CRC32, and compare to `expected_crc32`. On match the
   file is complete and valid; on mismatch the response carries
   `result=CrcMismatch` + `computed_crc32` and the (incomplete) file is left in
   place — but it is **invisible to `StoreList`** because `binbook-storage`'s
   enumeration validates every file via `Book::open` (a truncated/corrupt file
   fails to open and is skipped). The host may then `StoreDelete` it or re-upload.
4. `StoreAbort { backend, name }` (optional) → delete the file and drop the cached
   handle.

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
  ≤ 64 characters; reject path separators (flat `/books/` only); UTF-8.

## `StoreDelete`

`StoreDelete { backend, name }` → FAT remove. Reject if an upload of the same
`name` is in flight. This is also how a host cleans up a file left by a failed
upload (it won't appear in `StoreList`, but `StoreDelete` removes it by name).

## `StoreRead` (verification backbone)

`StoreRead { backend, name, offset, max_bytes }` → `{ data[], eof }`, paginated
like `LogGet`. Lets the host independently fetch a stored file's bytes and
confirm it is byte-identical to the source — the completion-evidence backbone.

## Host CLI

New noun-based subcommand under the existing `diag` command:

```
binbook diag storage --device {sd|flash} {upload,list,delete,read} ...
```

`--device` **defaults to `sd`**. `upload <path>` reads a local `.binbook` and
pushes it (pipelined, ascending offsets); `list` prints entries; `delete <name>`
removes; `read <name>` fetches bytes (used by the verification harness). The CLI
reads `max_frame_bytes` from `Hello` and adapts chunk sizing.

## Crate wiring (`binbook-fw`)

`diag::dispatch_command` gains new `DispatchResult` variants that route storage
opcodes to `binbook-storage` / `embedded-sd-storage` over the `binbook-fw` shared
bus (gated). The diagnostic task is the sole SD accessor in B (C's reader will
share the same arbiter). One cached write-handle per active upload name; released
on commit/abort.

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

## Open questions for the implementation plan

- Final opcode byte values and exact payload layouts (LE field order, name
  encoding/length prefix, offset width).
- Pipelining window size and ack model on the host (how many in-flight frames,
  how loss/retransmit is detected given per-frame CRC + idempotent writes).
- Whether `Hello` should also advertise which backends are implemented (a
  capabilities refinement) in addition to the reserved `backend` enum.
- Exact `Unsupported` status byte value.

---

## Roadmap entry to add to `docs/ROADMAP.md`

> **Location note:** This entry belongs in `docs/ROADMAP.md` (the planning agent
> cannot write there). Move it during implementation.

```markdown
## LittleFS internal-flash BinBook storage backend

Status: roadmap (not started).

Add LittleFS on the ESP32-C3 internal flash as a second `binbook-storage`
backend (a full writable peer to SD: upload/list/delete/read). The diagnostic
protocol already reserves the `backend` enum (`sd` | `flash`) and the
`--device flash` CLI flag from the 2026-07-01 diagnostic-storage-extension
design; implementing this backend lights up the `flash` path without a protocol
change. Respect the existing crash sector and the read-only `FlashStorage` table;
size for a small number of small books given ~192 KB total. Address
wear-leveling/fragmentation and the careful sector-aligned erase/program path
before implementation.
```
