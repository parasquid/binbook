# Serial Transfer Throughput Phase 2 Plan

**Status:** Transactional publication, physical SD-write tracing, metadata
write-back caching, table-driven CRC, and SPI-frequency caching are live.
The live tracer now learns FAT geometry from the mounted card's BPB and emits
fixed-size purpose labels for payload, each FAT copy, directory, journal, and
other writes. Successful physical writes expose their caller-owned sector data
to the diagnostic observer; CRC-valid `BBXJ` records register journal sectors
in a bounded semantic map before the physical completion is finalized. A fresh
1,022,165-byte upload captured all 352 writes with no overflow or anomalies:
340 payload calls, one FAT0 call, one FAT1 call, seven directory calls, and
three journal calls. Journal and directory physical time totaled only 29,040
us, while payload writes took 4.201 s and inter-payload gaps took 4.160 s.
The selected transport now uses a feature-gated interrupt-drained USB ingress
ring while shrinking the task-owned RX buffer from two frames to one. Combined
capacity remains 8,252 bytes, so this adds no net frame-buffer RAM. Three warm
1,022,165-byte legacy uploads measured 132,559--133,094 B/s (median 132,904
B/s); exact legacy readback measured 171,389 B/s. This is about 2.69x the
original 49 KB/s upload and 1.86x the original 92 KB/s readback baselines.
The selected shape remains 3,584-byte chunks and window two. A bounded
four-slot 1,536-byte bulk probe regressed to 101,004 B/s because extra SD setup
outweighed deeper ingress buffering, so bulk remains unadvertised.
The rejected conditional chunk/ring changes have been removed. The exact
cleaned selected source was rebuilt and flashed as a 278,768-byte application;
a fresh cold-name upload reached 108,914 B/s including library refresh and its
byte-exact readback reached 170,105 B/s, with matching SHA-256 and zero protocol
errors. The earlier three-run warm median remains the sustained-upload result.
Final current-image acceptance uploaded the 24,671,670-byte book at 136,353
B/s and read it back at 170,650 B/s. CRC32, external `cmp`, independent SHA-256,
library visibility, and final zero-error STATUS all passed. This is 2.76x the
original final-book upload rate and about 1.85x the original readback rate.
Protocol codecs, host negotiation and bulk serial algorithms, durable-progress
CLI UX, aggregate metrics state,
caller-buffered multi-block mounted I/O, pure four-slot upload/readback state
machines, bounded upload/readback storage-owner coordinators, protocol dispatch,
the mounted diagnostic-task channel driver, idle-timeout cleanup, metric phase
hooks, legacy trace terminalization, and atomic transfer-report output are
implemented and covered by host tests plus a target build. The upload
coordinator moves four static buffer leases
without payload duplication, validates owner correlation, batches credits only
after durable completions, reclaims stale leases after abort, and gates publish
on exact CRC. The readback coordinator retains slots through exact owner reads
and host-durable credits and rolls back safely at status barriers. One static
four-buffer pool is instantiated by the mounted diagnostic runtime and shared
between directions. In-transfer failures after successful negotiation also
write reports; preflight failures cannot yet populate the locked selected-mode
and limits fields honestly. Live X4 trials selected the eight-block legacy path:
it improved controlled upload end-to-end time from a 31.602 s single-block
median to 19.117 s (39.5%, about 1.65x). Four-slot bulk was byte-correct but did
not meet the 20% gain gate, so it remains diagnostic-only and unadvertised.

Live pseudo-TTY evidence verifies exact durable bytes, percentage, rate,
elapsed time, ETA, CRC verification, publication, and library-update phases.
The terminal completion percentage was corrected from 99.9% to 100.0% with a
regression test and a second live capture. Adversarial CLI review also replaced
the top-level `list` and `delete` placeholder prints with real protocol routing;
live delete plus independent `NotFound` and real list output verify execution.

**Cold-start resume point:** Do not rebuild the protocol, bulk pipeline,
transactional publication, or SD transaction tracer from scratch. A new
executor should first read `HANDOFF.md`, then:

1. Keep the selected production image bulk-disabled and enable
   `usb-rx-isr-buffer`. Do not advertise bulk
   without a new implementation that passes the locked material-gain gate.
2. Preserve the hidden staged upload, CRC-gated A/B journal, boot recovery, and
   two-sector metadata write-back cache. Together with table CRC, cached SPI
   frequency transitions, and interrupt-drained USB ingress, the current warm
   1,022,165-byte median is 132,904 B/s upload and the verified readback is
   171,389 B/s, with exact SHA and zero protocol errors.
3. Do not add reserve-only allocation now. The latest complete trace has 37
   CMD24 and 321 CMD25 calls and only about 3.22 s of physical writes in an
   8.35 s upload; the earlier 591-write/21.306-second result is superseded.
4. Preserve the live BPB-derived semantic trace. Its latest complete
   1,022,165-byte run measured 4.201 s in payload writes and 4.160 s in gaps
   between payload calls. CRC-validated journal-sector discovery split three
   journal calls from seven directory calls; together they consumed only 29
   ms. Do not optimize journal or directory I/O as a throughput lever without
   new contrary measurements.
5. Bulk, window eight, and larger frames are rejected at the current evidence gate:
   window four showed no gain or USB/SD overlap, and inbound RX safely holds
   only two maximum frames. Direct persistent-TX encoding fixed the live bulk
   readback framing failure, but warm bulk measured only 145,877--147,754 B/s
   readback and 103,931 B/s upload, both below selected legacy. Reconsider only
   after a phase-changing design can produce real USB/SD overlap. Full-window
   cumulative bulk upload credits reduced control TX, but the live 1,022,165
   byte upload still reached only 107,517 B/s with zero overlap. It remained
   exact and zero-error; bulk readback reached 160,667 B/s with exact SHA.
6. Finish Tasks 9-10, including a fresh final-book readback if completion of
   this entire plan is required. Do not claim every acceptance row complete
   while the live fault matrix remains partial.

**Goal:** Improve Xteink X4 serial upload and full-file readback beyond the
verified approximately 49 KB/s upload and 92 KB/s readback rates, while keeping
protocol v2 compatibility, exact response validation, bounded firmware RAM,
CRC protection, interruption cleanup, byte-identical readback, and zero
protocol errors.

**Expected result:** A measured improvement, plausibly another 2–4 times where
the phase timings show that much removable latency. The acceptance rate is not
an arbitrary headline number: derive it from the instrumented USB, protocol,
storage, and commit phases as specified below.

**Hard device constraint:** At plan creation time another agent was using the
Xteink X4. Do not flash, open a serial port, probe hardware ports, capture the
webcam, or otherwise access the device until the user or current device owner
explicitly releases it. Host
tests, source work, target builds, ELF inspection, and fake-device benchmarks
may proceed without the board. Once released, every hardware command in this
plan is sequential and has one serial owner.

## 1. Cold-start context

Read these before editing:

- `AGENTS.md` and `AGENTS.local.md`
- `HANDOFF.md` (host-local current state; never stage or commit it)
- `docs/reference/rust-development-standards.md`
- `docs/reference/xteink-x4-agent-device-verification.md`
- `docs/reference/squidscript-and-xteink-reference.md`
- `docs/specs/2026-07-01-diagnostic-storage-extension-design.md`
- `docs/plans/2026-07-15-serial-upload-throughput-and-progress-plan.md`
  for the completed Phase 1 behavior and evidence
- `../SquidScript/platforms/esp/src/main.rs`, specifically the bounded BLE
  storage command queue, cancellation lane, session checks, and upload worker
- `../SquidScript/platforms/esp/src/ota.rs`, specifically exact-offset chunk
  classification and durable retry handling
- `../SquidScript/platforms/esp/src/lib.rs`, specifically its use of this
  repository's reusable `SdStorage` upload APIs

SquidScript is reference-only for this work; do not edit the sibling checkout.
Reuse its bounded queue, explicit session cancellation, strict offset, and
storage-worker ideas. Do not copy its by-value BLE chunk command, because that
would duplicate a full payload in every queued item. Treat its current
zero-fill extent preparation as a measurable hypothesis, not proof that it is
faster for USB transfers.

The verified Phase 1 implementation is represented by commits `ca6c493`
through `a9855b4`. Do not assume those are still `HEAD`; record the actual
commit and worktree status when starting.

Current behavior to preserve:

- Protocol version is `2`, `MAX_PAYLOAD_BYTES` is `4096`, and encoded
  `MAX_FRAME_BYTES` is `4126`.
- Upload data chunks are `3584` bytes, aligned to seven 512-byte SD sectors.
- New peers advertise two-frame upload and readback windows; old peers fall
  back to stop-and-wait.
- Host upload is a bounded two-pass stream: pass one computes size and CRC32;
  pass two sends data. It does not hold the complete book in RAM.
- Progress advances from validated device acknowledgements, not host writes.
- Upload commit verifies the device-computed CRC and then separately waits for
  library visibility.
- Readback streams to a buffered host file and removes partial output on
  failure.
- Firmware USB RX is event-driven, but `diagnostic_task` still awaits
  `handle_owner_storage_command`, which awaits the storage-owner response.
  While that await chain is active, the USB task does not continue decoding
  frames.
- The mounted production path keeps a persistent FAT mount and file session,
  but `BlockDeviceStream` still submits one block per SD call.
- The reusable `SdStorage::begin_upload` path used by SquidScript preallocates
  and caches FAT extents by zero-filling the entire expected file first. That
  avoids later FAT walking but doubles payload writes unless a reserve-only
  strategy replaces the zero-fill. Treat it as a benchmark candidate, not an
  automatic optimization.
- SD SPI stays at the verified 4 MHz in this plan. Its raw line-rate ceiling is
  about 500,000 bytes/s before commands, tokens, FAT work, and contention.

Measured live baselines from `HANDOFF.md`:

| Artifact | Operation | Result |
| --- | --- | --- |
| 1,022,165-byte navigation fixture | legacy upload before Phase 1 | median 215.50 s / 4.63 KiB/s |
| same fixture | Phase 1 upload | median 30.91 s / 6.97× legacy |
| 24,671,670-byte final book | Phase 1 upload | 500.13 s / 49,334 B/s |
| same final book | Phase 1 readback | 268.24 s / about 92 KB/s |

The final book upload and readback matched CRC32, `cmp`, and SHA-256 and left
`protocol_error_count=0`.

## 2. Root-cause hypotheses and rules

Test these hypotheses in order; do not optimize all of them in one unmeasured
batch.

1. **USB and storage are serialized.** The current USB task waits for each
   owner-backed command. A larger host window alone cannot create sustained
   overlap.
2. **Blocking SD limits true concurrency.** The mounted FAT stack and
   `embedded-sdmmc` block-device calls are synchronous on a single-core
   cooperative executor. RAM slots permit useful USB and SD bursts, but they do
   not prove simultaneous work. Only subtract overlap when timestamped metrics
   observe it.
3. **Single-block SD calls waste command overhead.** Aligned 3584-byte chunks
   can become one seven-block write instead of seven single-block writes. The
   underlying `embedded-sdmmc` driver already supports multi-block slices.
4. **FAT allocation may be material.** The mounted writer allocates as the file
   grows. SquidScript's extent-cached path avoids that during data writes but
   performs a whole-file zero-fill first. Compare total physical I/O and total
   wall time, not only the fast-looking second pass.
5. **Per-frame responses still cost host/device turns.** A negotiated bulk
   stream with cumulative credits should reduce acknowledgements while keeping
   exact durable progress.
6. **Larger frames may not be worth their RAM.** Existing frame decode and
   response paths already materialize frame-sized arrays. Do not increase the
   frame budget until buffer-lifetime and linked-RAM evidence prove it safe.

Implementation rules:

- Use TDD for every slice: add the smallest discriminating failing test, run it
  and record the expected failure, implement the minimum change, then rerun all
  relevant tests.
- Measure on hardware after each independent performance slice. Do not batch
  multi-block I/O, extent changes, bulk streaming, and larger frames into one
  flash.
- Keep reusable storage behavior in `embedded-sd-storage`; keep board timing
  and Embassy task wiring in `binbook-fw`; keep serial/file/CLI behavior in the
  `binbook` host crate.
- Use caller-owned fixed buffers. Queues contain slot indices and descriptors,
  never embedded 3584-byte arrays.
- Keep control/status/abort traffic independent from the full data queue.
- Never silently retry a mutating chunk. Query durable status, then resume only
  from the exact returned offset or abort and restart.
- Do not raise the SD clock or add buffers merely because a build links.

## 3. Locked protocol additions

Keep protocol v2. Old hosts ignore unknown capability bits; new hosts must not
send a new opcode unless the corresponding capability is advertised.

Add these capability bits in
`firmware/crates/binbook-diagnostic-protocol/src/types.rs`:

```text
bit 10  CAP_STORAGE_TRANSFER_LIMITS
bit 11  CAP_STORAGE_TRANSFER_METRICS
bit 12  CAP_STORAGE_BULK_V1
```

Retain bits 8 and 9 for the existing two-frame legacy behavior. Add these
opcodes without renumbering existing values:

```text
0x12 StoreTransferLimits
0x13 StoreTransferTrace
0x14 StoreBulkOpen
0x15 StoreBulkData
0x16 StoreBulkCredit
0x17 StoreBulkStatus
0x18 StoreBulkFinish
0x19 StoreBulkAbort
```

All integers below are little-endian. Encoders and decoders must write fields
individually; do not use `repr(C)` or transmute as a wire format.

### 3.1 Transfer limits

`StoreTransferLimits` request payload is empty. Its response payload is:

| Offset | Field | Type | Meaning |
| ---: | --- | --- | --- |
| 0 | schema_version | u8 | `1` |
| 1 | flags | u8 | bit 0 upload bulk, bit 1 read bulk |
| 2 | max_data_bytes | u16 | initially `3584` |
| 4 | legacy_upload_window | u8 | negotiated maximum, at least 1 |
| 5 | legacy_read_window | u8 | negotiated maximum, at least 1 |
| 6 | bulk_slots | u8 | physically backed data credits |
| 7 | reserved | u8 | must encode zero |
| 8 | rx_capacity_bytes | u16 | measured/guaranteed admitted encoded bytes |
| 10 | max_frame_bytes | u16 | initially `4126` |
| 12 | idle_timeout_ms | u32 | initially `60000` |

New host fallback order is:

1. If `CAP_STORAGE_TRANSFER_LIMITS` is present, query and validate this payload.
2. Otherwise map existing window-2 capability bits to window 2.
3. Otherwise use window 1.

Reject zero windows, zero chunk size, a chunk larger than the frame budget, or
limits inconsistent with advertised capabilities. Reject unknown flag bits and
nonzero reserved fields.

### 3.2 Transfer metrics

`StoreTransferTrace` request begins with `action:u8`, `direction:u8`, two zero
reserved bytes, and `trace_id:u32`:

```text
action:     0=start, 1=get, 2=clear
direction:  0=upload, 1=readback, 255=unspecified/control
trace_id:   zero for start; exact assigned ID for get/clear
```

`start` and `get` return this exact 132-byte schema-version-1 snapshot. `clear`
returns an empty success payload:

| Offset | Field | Type |
| ---: | --- | --- |
| 0 | schema_version | u8 |
| 1 | direction | u8 |
| 2 | trace_state | u8 |
| 3 | selected_window | u8 |
| 4 | trace_id | u32 |
| 8 | chunk_size | u16 |
| 10 | peak_rx_bytes | u16 |
| 12 | logical_bytes | u32 |
| 16 | frames | u32 |
| 20 | usb_rx_bytes | u32 |
| 24 | usb_tx_bytes | u32 |
| 28 | usb_rx_us | u32 |
| 32 | usb_tx_us | u32 |
| 36 | framing_us | u32 |
| 40 | frame_crc_us | u32 |
| 44 | owner_queue_us | u32 |
| 48 | owner_execute_us | u32 |
| 52 | sd_read_calls | u32 |
| 56 | sd_write_calls | u32 |
| 60 | sd_read_blocks | u32 |
| 64 | sd_write_blocks | u32 |
| 68 | sd_read_logical_bytes | u32 |
| 72 | sd_write_logical_bytes | u32 |
| 76 | sd_read_us | u32 |
| 80 | sd_write_us | u32 |
| 84 | fat_allocation_us | u32 |
| 88 | upload_prepare_us | u32 |
| 92 | data_io_us | u32 |
| 96 | crc_us | u32 |
| 100 | commit_us | u32 |
| 104 | library_refresh_us | u32 |
| 108 | data_phase_us | u32 |
| 112 | end_to_end_us | u32 |
| 116 | usb_sd_overlap_us | u32 |
| 120 | peak_slots | u8 |
| 121 | flags | u8 |
| 122 | queue_stalls | u16 |
| 124 | protocol_errors_start | u32 |
| 128 | protocol_errors_end | u32 |

`trace_state` is `0=active`, `1=succeeded`, `2=failed`, or `3=aborted`. Trace
flag bit 0 means library refresh is still pending; bit 1 means at least one
counter saturated; all other bits are rejected. Starting while a trace is
active returns `Status::BadRequest`; starting after a terminal trace replaces
it. Getting or clearing the wrong ID returns `Status::NotFound`. Clearing is
allowed only after the trace is terminal. An upload becomes terminal at bulk
finish, with the pending flag set until its matching library refresh completes.
Refresh completion updates the record only when its trace ID still matches; a
newer trace is never contaminated by a late refresh. `--no-wait-library`
reports the pending flag instead of waiting.

Use saturating `u32` microsecond counters. The current approximately 500-second
book upload fits. Counts and bytes are also saturating `u32`. Add checked codec
tests for the exact encoded length, every field, short payloads, invalid
actions, and overflow saturation.

Do not emit one log record per frame. Metrics are an aggregate session object,
so a 24 MB book cannot overflow the 256-record runtime log.

### 3.3 Bulk stream wire contract

Data-plane frames are deliberately one-way events: `StoreBulkData` and
`StoreBulkCredit` always use `FrameKind::Event`, frame `Status::Ok`, and frame
sequence zero in either direction. Reject another kind, status, or sequence.
Every other opcode in this section uses the existing request/response model;
the response must exactly match request opcode and sequence. This distinction
removes per-chunk response turns without weakening validation.

`StoreBulkOpen` request:

```text
direction:u8          0=upload, 1=readback
backend:u8            existing StorageBackend value
requested_window:u8   0=automatic, otherwise 1/2/4/8
reserved:u8           zero
requested_chunk:u16   0=automatic, otherwise requested bytes
name_len:u8
reserved:u8           zero
total_size:u32        required for upload; zero for readback
expected_crc32:u32    required for upload; zero for readback
name:[u8; name_len]
```

Upload `total_size` must be `1..=u32::MAX`; readback requires both size and CRC
request fields to be zero. For an automatic chunk, select 3584. For an explicit
chunk, require at least 512 and select the largest 512-byte multiple not greater
than the request or 3584. For an automatic window select the firmware maximum;
otherwise clamp the requested 1/2/4/8 value to the firmware maximum. Reject all
other window values, unsafe names, unknown directions/backends, unknown flags,
and nonzero reserved fields.

Successful response:

```text
stream_id:u32
selected_chunk:u16
initial_credits:u8
direction:u8
durable_offset:u32
total_size:u32
```

`StoreBulkData` common payload:

```text
stream_id:u32
offset:u32
data_len:u16
flags:u8              bit 0=end-of-data; all other bits zero
reserved:u8           zero
data:[u8; data_len]
```

`data_len` is nonzero and no larger than the negotiated chunk. The end flag is
required exactly when `offset + data_len == total_size` and forbidden earlier.
The final chunk may be shorter and need not be sector aligned.

- Upload data travels host-to-device in event frames. It does not receive a
  one-response-per-frame acknowledgement.
- Readback data travels device-to-host in event frames.
- Offsets must be exactly contiguous. Duplicate, stale, skipped, wrapped, or
  oversized data is a terminal protocol error for that stream.

`StoreBulkCredit` payload is:

```text
stream_id:u32
durable_offset:u32
credits:u8
status:u8             existing Status value
reserved:u16          zero
```

For upload it is a device event. For readback it is a host event. Credits are
additive but the resulting outstanding credit count may never exceed the
negotiated slot count. Send a cumulative credit event when half the window has
been released (`max(1, selected_window / 2)`), when the sender would otherwise
stall, and on error/end.

`StoreBulkStatus` request is `stream_id:u32`. Response:

```text
stream_id:u32
direction:u8
state:u8              0=receiving, 1=finishing, 2=complete, 3=failed
available_credits:u8
reserved:u8           zero
durable_offset:u32
total_size:u32
running_crc32:u32
```

`running_crc32` is the finalized CRC32-IEEE value of the durable prefix, not an
internal pre-XOR accumulator. It is zero when `durable_offset` is zero.

Status is a stream barrier, not a racy snapshot. On upload, pause data
admission, let every already-admitted descriptor finish, reclaim its slots,
then respond with the resulting durable offset and the full selected credit
count. On readback, pause production, discard/reclaim unacknowledged outbound
slots, reset the producer to the host-acknowledged durable offset, then respond
with full credits. A repeated status request returns the same quiescent state
until new valid data/credits advance it.

`StoreBulkFinish` request is `stream_id:u32`. Successful response:

```text
stream_id:u32
result:u8             reuse StoreUploadCommitResult values
reserved:[u8;3]       zero
total_bytes:u32
computed_crc32:u32
```

For upload, finish verifies exact length and CRC before publishing and queuing
the library refresh. For readback, the host sends finish after the end-of-data
event and compares the returned size and CRC with its locally streamed result.

`StoreBulkAbort` request is `stream_id:u32`; success response echoes the same
stream ID. It is idempotent for the most recently aborted stream so a lost
abort response can be queried once without resurrecting state.
The idempotent tombstone lasts until the next successful bulk open or reset.

Only one bulk stream may be active. Legacy storage operations that would
mutate the same file return `Status::BadRequest` while it is active. HELLO,
STATUS, trace GET, bulk status, and bulk abort remain serviceable.

## 4. Locked RAM and task architecture

### 4.1 Four-slot production pool

Use these initial production constants:

```text
TRANSFER_DATA_BYTES       = 3584
TRANSFER_SLOT_COUNT       = 4
TRANSFER_ACK_BATCH        = 2
TRANSFER_IDLE_TIMEOUT_MS  = 60000
SD_BATCH_BLOCKS           = 8
BULK_INCREMENTAL_RAM_MAX  = 20 KiB
MIN_LINKED_DRAM_HEADROOM  = 48 KiB
```

Each transfer slot contains its 3584-byte data buffer plus only bounded
metadata: generation, frame sequence, offset, length, direction, and state.
Slot states are explicit:

```text
Free -> RxReady -> StorageOwned -> Free              upload
Free -> StorageOwned -> TxReady -> AwaitingAck -> Free  readback
```

Repurpose the current 4096-byte `OWNER_TRANSFER` allocation as slot zero rather
than retaining it alongside the pool. Four slots contain 14,336 data bytes;
reusing the existing allocation makes the incremental slot cost approximately
10.5 KiB. The separate eight-block SD buffer costs 4096 bytes. Descriptor and
control queues must keep the whole new allocation below 20 KiB.

Add compile-time size assertions for:

- slot pool;
- data and return descriptor channels;
- separate control channel;
- active stream/session state;
- SD block staging;
- metrics state.

The assertion covers added Phase 2 state, not pre-existing `SerialState` RX/TX
buffers. Also inspect the complete ELF; a local assertion does not replace the
48 KiB linked-headroom gate.

### 4.2 Queues and cancellation

Use a monotonically wrapping nonzero stream generation/session ID. Every data
descriptor carries it. Abort, timeout, reset, storage failure, or completion
increments/clears the active generation before slots are recycled. The storage
worker discards a descriptor whose generation no longer matches, returning its
slot without touching storage.

Provide:

- data-to-storage channel, capacity equal to slot count;
- storage-to-USB/slot-return channel, capacity equal to slot count;
- control channel, capacity at least two, for abort and status/error work;
- one active-stream state object.

Do not copy SquidScript's BLE queue representation, which embeds a complete
payload in each command. Reuse its bounded-depth, stale-session, and separate
control-lane concepts only.

### 4.3 USB and storage ownership

`diagnostic_task` remains the sole USB RX/TX owner. It must stop awaiting each
data storage operation directly. It:

1. drains USB;
2. decodes and validates one complete frame;
3. copies bulk data directly into a free slot;
4. queues a slot descriptor;
5. returns to USB service;
6. sends credit/data events produced by the storage worker.

The storage-owner task remains the sole SD/FAT owner. Extend the existing
diagnostic client rather than creating a second SD owner. Control commands may
wait for an exact owner response, but bulk data completion returns through the
bounded slot/credit channel.

Because SD calls are synchronous, do not describe this as concurrent until the
metrics show nonzero overlap. If a multi-block call prevents USB servicing long
enough to approach RX capacity, reduce the bounded SD run length or yield
between calls; do not enlarge RX blindly.

## 5. Throughput model and selection gates

An instrumentation-only legacy run establishes `T_baseline`. Calculate the
best evidence-backed model after isolated USB and SD measurements:

```text
T_floor = T_usb_best + T_sd_best + T_fixed_best - T_overlap_proven
T_target = T_baseline - 0.70 * (T_baseline - T_floor)
```

Definitions:

- `T_usb_best`: measured USB/framing/credit time for the selected bounded mode.
- `T_sd_best`: measured total storage time for the selected strategy, including
  preparation that is required for every upload.
- `T_fixed_best`: measured non-overlappable CRC, control, and finish work.
- `T_overlap_proven`: timestamped interval where USB and SD active flags were
  simultaneously set; use zero if instrumentation cannot prove it.

Round the target duration up to the next 0.1 second. A result may be called a
material optimization only when the median data phase improves at least 20%.
If the model predicts a 2–4× opportunity, the result must capture at least 70%
of that removable time. Library-refresh time is reported separately and is not
part of bytes-per-second, but it remains in end-to-end UX reporting.

Selection rules:

- **Multi-block SD:** enable only if SD consumes at least 15% of the data phase
  and eight-block calls improve median SD time at least 20% over single-block.
- **Extent/preallocation strategy:** select only if total preparation plus data
  plus commit is at least 10% faster than the mounted incremental writer. Count
  zero-fill blocks as real work.
- **Window:** test 2, 4, and, when RAM-eligible, 8. Select the smallest eligible
  window within 5% of the fastest. Window 8 must also beat window 4 by at least
  10%.
- **Larger frame:** consider approximately 8 KiB only when USB/framing/credit
  overhead remains at least 20% of wall time, the model predicts at least 15%
  additional improvement, no duplicate frame-sized lifetime is introduced,
  and 48 KiB linked headroom remains.

## 6. Implementation tasks

Maintain the todo tracker while executing. Do not start a later performance
task while the preceding task's tests or required live checkpoint are failing.

### Task 0 — Re-ground and protect current work

- [x] Read all cold-start references listed above.
- [x] Run `git status --short`, record `git rev-parse HEAD`, and inspect recent
  transfer commits. Preserve unrelated or user-owned changes.
- [x] Confirm the current CLI/module names and protocol values from source; do
  not copy old path descriptions from the completed Phase 1 plan.
- [x] Confirm device ownership. If it is not explicitly free, record the block
  in `HANDOFF.md` and perform no hardware command.
- [x] Create a Phase 2 acceptance matrix in `HANDOFF.md` with requirement,
  implementation, automated test, live evidence, and status columns.

**Gate:** Worktree scope is understood, the device constraint is recorded, and
no current source behavior contradicts this plan.

### Task 1 — Add protocol codecs and host state-machine tests

Primary areas:

- `firmware/crates/binbook-diagnostic-protocol`
- `crates/binbook/src` transport/upload/readback modules
- their existing protocol and fake-peer tests

- [x] Add failing codec tests for all capability bits, opcode numeric values,
  payload layouts, maximum data size, reserved bytes, short payloads, invalid
  values, and round trips.
- [x] Implement protocol codecs without allocation in the firmware crate.
- [x] Add fake-peer tests for capability fallback and transfer-limit
  validation.
- [x] Add a transport-level frame router that can distinguish response and
  event frames while preserving unmatched complete frames for the active
  session. It must reject duplicate expected responses, wrong kind/opcode,
  wrong sequence, wrong stream ID, bad offsets, and malformed events.
- [x] Model bulk upload and readback as host-testable state machines before
  connecting them to serial I/O.
- [x] Add `--transfer-mode auto|legacy|bulk`, default `auto`; `bulk` fails
  clearly when unsupported. Add `--window auto|1|2|4|8` for controlled
  measurement; firmware may clamp downward but the host reports the selected
  value.
- [x] Add `--transfer-report <path>` to upload and diagnostic readback. The
  versioned JSON report contains negotiated limits, trace metrics, byte counts,
  phase durations, rate, retries/status queries, CRC, and result. Successful
  upload/readback reports, atomic replacement, numeric CRCs, exact limits,
  optional capability-gated trace retrieval/clear, and host phase accounting
  are implemented and tested. Post-negotiation errors emit reports. Preflight
  failures intentionally do not create a schema-v1 report because no honest
  selected mode or negotiated limits exist yet; stderr remains authoritative
  for those failures. A future schema revision may add a preflight result
  without fabricating locked fields.

The transfer report has this stable top-level schema; metric field names match
the wire snapshot exactly:

```text
schema_version: 1
operation: "upload" | "readback"
mode_requested: "auto" | "legacy" | "bulk"
mode_selected: "legacy" | "bulk"
name: string
bytes: u64
expected_crc32: u32 | null
computed_crc32: u32 | null
limits: { max_data_bytes, legacy_upload_window, legacy_read_window,
          bulk_slots, rx_capacity_bytes, max_frame_bytes, idle_timeout_ms }
host: { prepare_ms, data_ms, finish_ms, library_ms, end_to_end_ms,
        bytes_per_second, status_queries, retries }
firmware: null | { all StoreTransferTrace snapshot fields }
result: { status: "ok" | "error", library_visible: bool | null,
          library_refresh_pending: bool,
          cleanup_confirmed: bool | null, protocol_error_delta: u32 }
```

Write the report to a sibling temporary path and rename it only after complete
JSON serialization, so interruption cannot leave a valid-looking partial
report. JSON numeric CRC fields are decimal; human output may continue showing
hexadecimal CRC32.

**Gate:** Protocol and host fake-peer tests pass; existing Phase 1 legacy tests
remain unchanged and green.

### Task 2 — Add non-perturbing phase instrumentation

Primary areas:

- USB diagnostic task and serial framing
- diagnostic owner/storage request routing
- board-local measured SD block-device wrapper
- library refresh trigger/completion

- [x] Add a fixed `TransferMetrics` aggregate behind `diagnostic-console`.
  Use explicit start/stop methods and a test clock.
- [x] Track an active-phase bitmask and update accumulated time on each phase
  transition. Increment `usb_sd_overlap_us` only while both phase bits are set.
- [x] Timestamp diagnostic owner requests when queued; accumulate wait and
  execution separately.
- [x] Wrap the real `embedded_sdmmc::BlockDevice` at the firmware boundary with
  a generic measured wrapper. Do not add Embassy or ESP dependencies to
  `embedded-sd-storage`.
- [x] Aggregate physical block-call counts, blocks, and elapsed time at the
  synchronous observer boundary. Do not queue one start/complete pair per call:
  a long owner operation cannot yield to drain such a queue. Nested or
  unmatched observer events mark the trace saturated.
- [x] Propagate an optional trace ID through upload commit and the queued
  library refresh so refresh time is attributed to the initiating transfer.
- [x] Add fake-clock tests proving nesting, saturation, overlap accounting,
  read/write separation, trace-ID isolation, clear behavior, and no per-frame
  log growth.
- [x] Add CLI trace start/get/clear support used internally by
  `--transfer-report`; normal human output should not dump the full metric
  record unless requested.
- [x] Advertise `CAP_STORAGE_TRANSFER_METRICS` now that forced-legacy upload
  and readback traces reach terminal success/failure/abort states. Keep
  `CAP_STORAGE_BULK_V1` unadvertised until the Task 5 live gate passes.
- [x] On legacy upload CRC mismatch, do not invoke commit or publish the file;
  leave the session available for the host's abort cleanup and report the
  failed terminal trace.
- [x] Terminalize legacy readback traces as failed for missing files,
  out-of-range offsets, storage failures, and response-encoding failures. This
  prevents a rejected read from leaving the single trace slot active and
  blocking the next transfer.

#### Task 2b — Trace every physical SD write transaction

This diagnostic is required before selecting any further SD optimization. It
must be compiled out of ordinary release firmware and must collect into bounded
RAM during the transfer; printing a line during each write would perturb the
USB and SD timings being measured.

- [x] Begin with host tests for an SD-SPI observer state machine. Feed exact
  command, data-token, 512-byte payload, response, busy-poll, and stop-token
  sequences for CMD24 and CMD25. Require rejection/anomaly accounting for
  malformed, nested, truncated, and over-capacity sequences.
- [x] Extend each measured block-device event with its exact starting block.
  Correlate start/completion events without losing calls and record the gap
  from the preceding physical SD call.
- [x] For every physical write call, record start block, block count, CMD24 or
  CMD25, command/setup microseconds, payload SPI-transfer microseconds,
  card-non-busy wait microseconds, total call microseconds, and inter-call gap.
  Use a fixed-capacity diagnostic capture with an explicit overflow flag and
  publish the capacity in the report; never silently drop transactions.
- [x] Parse the mounted card's MBR/BPB geometry and classify block traffic as
  boot/reserved, FAT, root/directory, or data. Report reads and writes for FAT
  and directory sectors separately. A host FAT-image test must prove the
  classifier against known sector ranges and distinguish metadata traffic from
  file-payload traffic; do not infer classification merely from call order.
- [x] Add a paged diagnostic retrieval command and host JSON report support so
  capture retrieval occurs only after the timed transfer. Preserve exact
  opcode, sequence, status, payload-length, and reserved-byte validation.
- [x] On the X4, reset the capture and upload the same disposable 1 MiB fixture
  across the single-block baseline and selected multi-block checkpoints,
  forced single-block and selected eight-block modes, retrieve all pages, and
  reconcile transaction counts and times with the aggregate transfer trace.
  Require zero capture overflow and zero protocol-error delta.
- [x] Use the trace to rank only evidence-backed candidates: fewer metadata
  scans/writes, longer contiguous CMD25 runs, reduced setup gaps, or reduced
  card-busy time. Do not raise the SD clock, enlarge buffers, or change FAT
  behavior without a discriminating before/after trace and byte-identical
  readback plus CRC/SHA verification.

The observable signals are the actual block-device call boundaries, decoded
SD command bytes, measured SPI operation intervals, returned busy-poll bytes,
and BPB-derived sector regions. Together these distinguish host/owner gaps,
FAT metadata amplification, command overhead, payload line time, and card
programming time; no one of those signals is accepted as evidence for another.

After the device is released, flash this instrumentation-only build with bulk
disabled/forced legacy and measure three 1 MiB uploads and three readbacks. Use
the same device, card, cable, port, fixture, firmware features, and disposable
name for every comparison.

**Gate:** Metrics reconcile with wall time within 5% or one USB scheduling
quantum, physical block counts are plausible, protocol errors remain zero, and
the instrumentation-only median does not regress Phase 1 by more than 5%.

### Task 3 — Add caller-buffered multi-block SD I/O

Primary area: `crates/embedded-sd-storage` and mounted-storage construction.

- [x] Write fake-block-device tests that require one multi-block call for an
  aligned 3584-byte write and bounded calls for data larger than eight blocks.
- [x] Change `BlockDeviceStream` to borrow caller-provided block scratch. Add an
  owned-mount constructor accepting `&mut [Block; 8]`; update firmware to
  allocate that buffer in a `StaticCell` and pass it in. Do not hide an
  additional `[Block; 8]` inside an async future.
- [x] For aligned full blocks, copy up to eight blocks into scratch and call
  `BlockDevice::write` once. For reads, fill up to eight blocks in one call and
  copy out. Keep single-block read-modify-write for unaligned boundaries.
- [x] Apply the same caller scratch to extent helpers; remove local block-sized
  temporaries where the caller buffer is authoritative.
- [x] Test partial prefix/suffix preservation, exact block indices, extent
  boundaries, injected read/write failures, zero-length rejection, and
  transfers larger than scratch.
- [x] Build and inspect target RAM before hardware. Reject hidden future growth
  or duplicate scratch.

When the device is free, compare single-block and eight-block variants with the
same traced 1 MiB upload/readback. Apply the multi-block selection gate before
continuing.

Live selection: eight-block upload reduced median physical SD time from
20.494 s to 11.953 s (41.7%) and end-to-end time from 31.602 s to 19.117 s
(39.5%). All controlled readbacks were byte-identical with zero protocol-error
deltas. Keep the eight-block caller buffer.

**Gate:** Selected behavior is byte-identical, block-call metrics match the
expected batching, live protocol errors are zero, and the hardware result is
recorded before Task 4.

### Task 4 — Evaluate SquidScript-derived extent staging

- [x] Add fake-device accounting around the existing
  `SdStorage::begin_upload` path and prove that its zero-fill preparation writes
  at least the expected file-size blocks before real data. The 3,584-byte test
  proves seven preparation payload blocks followed by seven real payload block
  writes; FAT metadata only increases the preparation total.
- [x] Compare current mounted incremental allocation with zero-fill plus cached
  extents using total physical blocks and total preparation/data/commit time.
- [x] If FAT allocation is below 15% of the data phase, stop: retain mounted
  allocation and document why extent work is not justified.
- [x] FAT allocation exceeded the activation threshold while whole-file
  zero-fill failed the total-time gate, so implement a reserve-only extent path:
  improvement gate, implement a reserve-only extent path:
  - derive 8.3 hidden names `BBUxxxxx.TMP` (stage) and `BBBxxxxx.TMP`
    (backup) from the low 20 bits of a nonzero generation; reject a collision
    instead of overwriting an unrelated file;
  - reserve the expected cluster chain without initializing every data byte;
  - cache a bounded extent vector and reject `UploadTooFragmented` when full;
  - accept only contiguous exact offsets and overwrite the complete expected
    length;
  - verify byte count and CRC before publication;
  - exclude `BBU*.TMP`, `BBB*.TMP`, and the journal files from library scans;
  - persist publication state in CRC-protected A/B metadata records named
    `XFERA.DAT` and `XFERB.DAT`; choose the valid record with the greatest
    wrapping generation;
  - encode each 148-byte record as `BBXJ` magic at 0, version 1 at 4, phase at
    5, target-name length at 6, flags at 7, generation at 8, expected size at
    12, expected CRC32 at 16, a zero-padded 121-byte target name at 20, three
    zero reserved bytes at 141, and CRC32 of bytes `0..144` at 144;
  - use phases `0=clear`, `1=receiving`, `2=verified`, `3=old-moved`, and
    `4=published`; write and verify the inactive A/B slot before crossing each
    phase barrier;
  - on publish, rename an existing target to the backup, persist `old-moved`,
    rename stage to target, persist `published`, delete backup, then clear the
    journal;
  - recovery for `receiving` or `verified` deletes the stage and preserves the
    target; `old-moved` restores backup when target is absent, otherwise keeps
    target, then deletes stage/backup; `published` keeps target and deletes
    stage/backup; every successful recovery clears the journal;
  - abort/reset invalidates the generation, deletes its stage, restores a moved
    backup when necessary, and clears the journal;
  - never expose or publish reserved data before exact length and CRC succeed.
- [x] Add fragmented FAT-image tests, capacity failure tests, interrupted
  publication recovery, old-file preservation, and proof that incomplete
  stages never appear in the library.
- [x] Live-test only strategies that pass host correctness and physical-I/O
  accounting. Select using the total-time gate, not data-write time alone.

**Gate:** One storage strategy is selected by the locked rule and its integrity,
abort, recovery, fragmentation, and live timing evidence are complete.

Current selection result: reject SquidScript's whole-file zero-fill, but
activate the reserve-only branch. The complete small trace proved metadata
write amplification and selected a one-sector write-back cache (380 to 71
physical calls). The controlled 1,022,165-byte run still made 591 writes, with
repeated mirrored FAT-sector CMD24 calls during incremental growth; physical
SD writes consumed 10.217 s of an 18.371 s data phase. Commit then consumed
21.306 s. This is sufficient evidence to implement reserve-only allocation and
commit-scan reduction, but not to claim either selected until their live total
time, exact readback, bounded fragmentation, reset cleanup, and zero-error
gates pass.

### Task 5 — Implement the four-slot firmware pipeline

Current cold-start checkpoint: metadata-only upload/readback slot machines,
the single-representation four-buffer lease pool, and both storage-owner
coordinators are complete. Their entry modules are
`firmware/crates/binbook-fw/src/bulk_upload_runtime.rs` and
`firmware/crates/binbook-fw/src/bulk_readback_runtime.rs`; focused tests are in
the correspondingly named files under `firmware/crates/binbook-fw/tests/`.
Both coordinators are split into focused runtime, control, and types modules;
keep new production modules below 250 logical lines. The mounted diagnostic
task decodes bulk commands, submits owner requests without awaiting each data
operation, consumes owner completions concurrently with USB input, and encodes
credit/data/control responses through `runtime/diagnostic_bulk.rs`. Do not advertise
`CAP_STORAGE_BULK_V1` before timeout cleanup, target RAM
gate, and live checkpoint all pass.

The direction runtimes must share one physical `BulkLeasePool`. They expose a
guarded lease-pool handoff only after sessions, owner requests, sent slots, and
pending controls are quiescent. A host test proves the same four buffers move
from a completed upload into readback. Do not instantiate separate upload and
readback arrays in the diagnostic task; that would silently double payload RAM
from 14,336 to 28,672 bytes.

The runtime integration is split by responsibility: `diagnostic_bulk.rs`
owns coordinator state and owner completion handling,
`diagnostic_bulk_action.rs` handles decoded protocol actions,
`diagnostic_bulk_owner.rs` maps owner events, and
`diagnostic_bulk_timeout.rs` owns timeout cleanup. At the current checkpoint
these contain 194, 93, 83, and 30 logical lines respectively. The complete set
of new Phase 2 production modules is below the required 250-logical-line
limit; preserve that property when adding live-checkpoint fixes.

`firmware/crates/binbook-fw/src/bulk_transfer_coordinator.rs` is the pure
non-blocking direction/owner coordinator. It admits four upload writes before
any completion, records at most eight owner operations by sequence, accepts
control-lane abort completion ahead of queued writes, and reclaims stale write
leases by sequence rather than assuming FIFO cleanup order. Keep the actual
Embassy task limited to decoding/encoding frames, channel send/receive, and
driving this coordinator; do not reintroduce an await-per-data-frame path.

- [x] Add failing state-machine tests for all slot transitions, full-pool
  backpressure, stale generation rejection, control-lane priority, abort, idle
  timeout, storage failure, and reset.
- [x] Introduce the four-slot pool and descriptor channels with compile-time RAM
  assertions. Repurpose the existing owner transfer buffer as slot zero.
- [x] Refactor the USB diagnostic task so accepted bulk data is queued without
  awaiting its SD write. Keep one USB owner and one SD owner.
- [x] Implement upload cumulative credits from durably completed SD writes.
- [x] Implement readback fill/data/ack slot transitions and EOF handling.
- [x] Batch credits at two released slots, but send immediately before a stall,
  on status, end, or error.
- [x] Service HELLO, STATUS, trace GET, bulk status, and bulk abort when the data
  pool is full.
- [x] Add a 60-second named idle timeout. Timeout invalidates queued descriptors,
  returns all slots, and removes incomplete storage state.
- [x] Track actual serial RX-buffer high-water, occupied-slot high-water,
  full-pool episodes, and explicitly observed USB/SD overlap metrics.
- [x] Keep owner-response correlation in a fixed-capacity, sequence-keyed reply
  tracker. Its overflow path must report an explicit error but still submit any
  owner request already admitted by the coordinator so storage state and
  leases drain. Reserve capacity before deferred finish mutates coordinator
  state, and consume pending replies on response-encoding failure.

Build and inspect memory, then run the four-slot live checkpoint when the board
is free.

Current host/target checkpoint (2026-07-15): the diagnostic release image links
with `.data=2,972`, `.bss=279,960`, `.stack=32,560`, and 72,704 bytes of linked
DRAM headroom. This proves only the build/RAM gate. It does not authorize bulk
capability advertisement or establish a throughput result.

**Gate:** Four-slot bulk upload/readback passes host tests, target build, RAM
gate, live boot, mid-window abort, status responsiveness, complete integrity,
and zero protocol errors.

### Task 6 — Integrate host bulk streaming and UX

- [x] In `auto` mode, query HELLO and limits, then select bulk only when all
  required capabilities and limits validate. Preserve the exact Phase 1 legacy
  fallback.
- [x] Upload keeps at most the negotiated window of host packets. Progress uses
  cumulative durable offsets from credit events.
- [x] Readback writes ascending data events to the buffered output, returns
  credits only after the write succeeds, and removes partial output on failure.
- [x] On timeout, issue one bulk status query. Resume only from the returned
  exact durable offset. If no matching stream exists, abort/restart explicitly;
  never resend an uncertain mutating frame.
- [x] After an upload status barrier, discard all local outstanding packets and
  seek the source to the returned durable offset. After a readback barrier,
  flush and truncate the host output to the returned host-acknowledged offset
  before granting new credits. Reject an output that cannot seek/truncate for
  resumable bulk readback rather than appending ambiguous bytes.
- [x] Show phases `Preparing storage`, `Uploading`/`Reading`, `Verifying CRC`,
  `Publishing`, and `Updating library`, with percentage, bytes, rolling durable
  rate, elapsed time, and ETA.
- [x] Preserve TTY/non-TTY/`--progress never` behavior and JSON compatibility.
- [x] Validate finish size and CRC. Full hardware acceptance still requires an
  external complete readback, `cmp`, and SHA-256.

**Gate:** Fake peers cover success and every failure mode; forced legacy and
automatic bulk both work; progress never exceeds durable bytes.

### Task 7 — Evaluate 2/4/8 windows and only then larger frames

Live decision: the four-slot diagnostic readback reached all four slots but was
slower than the best legacy path and measured zero USB/SD overlap. Upload RX
capacity is 8,252 bytes, only two maximum 4,126-byte frames, so inbound window
four is correctly clamped. Because four does not improve throughput, window
eight cannot satisfy its additional 10% gate and no eight-slot build is
justified. Framing/USB overhead is below the larger-frame gate; retain 4,126
byte frames.

- [x] Measure windows 2 and 4 using the same four-slot firmware by requesting a
  lower limit.
- [x] Apply the locked stop rule before an eight-slot build: window four showed
  no gain and therefore could not satisfy the additional 10% requirement.
- [x] For every eligible build, record `.data`, `.bss`, configured stack, end address,
  linked DRAM headroom, slot high-water, RX high-water, and live stability.
- [x] Reject any build below 48 KiB linked headroom or with an unexplained
  frame-sized async temporary.
- [x] Select the window using the locked smallest-within-5% rule; enforce the
  additional 10% requirement for window 8.
- [x] Recompute the throughput model. The larger-frame gate was not met, so
  record the decision and keep 4126-byte frames.
- [x] The larger-frame condition is false; no frame-size implementation was
  activated. If future measurements cross it, first audit every use of `MAX_FRAME_BYTES`, add bounded
  negotiated frame codecs without multiplying all legacy buffers, repeat all
  RAM and malformed-frame tests, then perform a separate live checkpoint.

**Gate:** The production window and frame size are selected entirely from the
documented rules, with no unmeasured tuning.

### Task 8 — Compatibility, recovery, and adversarial review

The earlier direct-final-name cleanup failure is superseded by journaled hidden
staging. On the current selected image, a source truncated after more than 18
frames produced exact local EOF, an acknowledged abort, terminal
`state=aborted`, `cleanup_confirmed=true`, independent `StoreRead NotFound`, no
published/listed disposable name, and zero protocol-error delta.

- [x] New host against old firmware: legacy upload/readback and existing CLI
  outputs remain correct.
- [x] Old host against new firmware: existing opcodes, responses, windows, and
  status behavior remain correct.
- [x] Force legacy against new firmware and verify exact per-response checks.
- [x] Inject host source truncation after more than one credit batch; require
  abort confirmation and independent `NotFound`/list absence.
- [x] Drop a credit event and prove status recovery without a duplicate write.
- [x] Inject wrong stream, wrong offset, duplicate event, bad CRC, premature
  EOF, and idle timeout live. Storage-owner failure injection is verified at
  its exact software boundary; physical media manipulation is not treated as
  broader storage-error evidence.
- [x] Begin from a discriminating nonempty state when testing cleanup and
  replacement preservation.
- [x] Independently query STATUS after the negative hardware matrix and require
  zero unexpected protocol errors.
- [x] Adversarially inspect every new opcode dispatch branch for placeholder
  responses, ignored flags, hard-coded status, or routed-but-unexecuted work.

**Gate:** All compatibility and recovery cells in the acceptance matrix have
implementation, automated proof, and observed live evidence.

Current live transport evidence includes byte-by-byte request delivery, exact
two-frame batching, malformed-frame recovery followed by a decoded STATUS, and
a clean reflash/final STATUS with zero protocol errors. The complete injected
bulk fault matrix remains diagnostic work; bulk is not advertised by the
selected image.

### Task 9 — Final live throughput and integrity gate

Do not start until the device is explicitly released. Run sequentially.

Host setup:

```bash
: "${PORT:?set PORT after the host device preflight}"
export PORT
export OUT="target/diag-evidence/serial-transfer-phase-2"
mkdir -p "$OUT"
export SYSTEMD_PREFIX="$(brew --prefix systemd)"
export PKG_CONFIG_PATH="$SYSTEMD_PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
export LIBRARY_PATH="$SYSTEMD_PREFIX/lib:${LIBRARY_PATH:-}"
export LD_LIBRARY_PATH="$SYSTEMD_PREFIX/lib:${LD_LIBRARY_PATH:-}"
```

Build and flash the selected diagnostic image:

```bash
cargo build -p binbook --features serial-device
FW_FEATURES="firmware-bin,diagnostic-console,sd-storage" \
  firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Capture boot serial for at least 15 seconds with the exact pyserial command in
`AGENTS.md`; wait for it to exit. Then establish independent baselines:

```bash
target/debug/binbook diag hello --port "$PORT"
target/debug/binbook diag status --port "$PORT"
```

Use the tracked fixture for controlled medians:

```bash
FIXTURE="firmware/crates/binbook-fw/fixtures/nav_probe.binbook"
NAME="serial-transfer-phase-2.binbook"
```

Before each run, inspect state with:

```bash
target/debug/binbook list --port "$PORT"
```

If the disposable name is present, delete that exact name and list again to
prove absence:

```bash
target/debug/binbook delete --port "$PORT" --name "$NAME"
target/debug/binbook list --port "$PORT"
```

Do not hide a delete failure with `|| true`, and do not delete pre-existing
books.

Repeat these command shapes with `RUN=1`, `2`, and `3`:

```bash
RUN=1
target/debug/binbook upload \
  --port "$PORT" --file "$FIXTURE" --name "$NAME" \
  --transfer-mode legacy --window auto --progress never \
  --transfer-report "$OUT/legacy-upload-$RUN.json"
target/debug/binbook diag storage read \
  --port "$PORT" --path "$NAME" \
  --output "$OUT/legacy-readback-$RUN.binbook" \
  --transfer-mode legacy --window auto \
  --transfer-report "$OUT/legacy-readback-$RUN.json"
cmp "$FIXTURE" "$OUT/legacy-readback-$RUN.binbook"
sha256sum "$FIXTURE" "$OUT/legacy-readback-$RUN.binbook"
target/debug/binbook diag status --port "$PORT"
```

Then repeat with `--transfer-mode bulk` and `bulk-*` output names. Use
`--transfer-mode auto` once as a separate negotiation proof. The diagnostic
read command must reject transfer-only flags when `--offset` or `--max-bytes`
selects the existing one-frame range-read path.

For the final book, set `BOOK` from the current relative artifact path recorded
in `HANDOFF.md`, verify its local size/CRC/SHA first, then use the same bulk
upload/readback command shapes with a distinct destination and output. Do not
assume a host-local artifact exists merely because an older handoff names it.

- [x] Run three forced-legacy uploads and readbacks with transfer reports.
- [x] Run sufficient diagnostic bulk upload/readback trials to apply the
  selection gate. Bulk was slower and remained unadvertised, so it is not a
  selected production mode requiring three final acceptance pairs.
- [x] Delete and independently confirm absence between disposable upload runs.
- [x] Record beginning and ending STATUS for each evaluated group.
- [x] Calculate median duration/rate, `T_floor`, `T_target`, captured removable
  time, and whether the 20% material-improvement gate passed.
- [x] Upload the 24,671,670-byte final-book artifact with the selected mode.
- [x] Read it completely back to `$OUT`, run `cmp`, and calculate SHA-256 for
  source and readback independently.
- [x] Require matching commit CRC, readback CRC, size, `cmp`, SHA-256, library
  entry, and final STATUS.
- [x] Require zero framing, sequence, status, offset, credit, RX-overrun, CRC,
  or other protocol errors.
- [x] Record flash command/output, application size, boot capture, trace JSON,
  starting/ending state, cleanup evidence, and integrity evidence in
  `HANDOFF.md`.

Upload throughput itself does not change display pixels, so no webcam claim is
required. If final verification opens or navigates the book and makes a visual
claim, follow the display-test and webcam disciplines separately.

**Gate:** The selected implementation meets the derived target, all integrity
and cleanup requirements, the RAM gates, and every acceptance-matrix row.

### Task 10 — Documentation and final checks

- [x] Update `docs/specs/2026-07-01-diagnostic-storage-extension-design.md` with
  current capability negotiation, bulk state machine, bounded memory, fallback,
  resume, and cleanup behavior.
- [x] Update CLI and firmware reference docs with exact current commands and
  output semantics.
- [x] Update `HANDOFF.md` as a current-state snapshot, distinguishing verified
  behavior, transport-only acknowledgements, unverified results, and failures.
- [x] Search `docs/ROADMAP.md` for `serial`, `upload`, `readback`, `bulk`,
  `window`, and `throughput`; remove or rewrite completed/stale items and leave
  only genuine future work.
- [x] Search for stale capability names, old window limits, deleted symbols, and
  obsolete commands across docs and README files.
- [x] Run the full verification suite below and `git diff --check`.
- [x] Perform the final adversarial completion review required by `AGENTS.md`.

## 7. Required automated commands

Run focused tests after their task, then all of these before completion:

```bash
cargo fmt --all -- --check
cargo test -p binbook-diagnostic-protocol
cargo test -p embedded-sd-storage
cargo test -p binbook-fw --features diagnostic-console,sd-storage
cargo test -p binbook --features serial-device
cargo test --workspace
cargo clippy -p embedded-sd-storage --all-targets -- -D warnings
cargo clippy -p binbook-fw --all-targets --features diagnostic-console,sd-storage -- -D warnings
cargo clippy -p binbook --all-targets --features serial-device -- -D warnings
uv run pytest -q
git diff --check
```

Build both the normal and diagnostic target images with pinned nightly tools:

```bash
cd firmware
RUSTC="$(rustup which --toolchain nightly rustc)" \
  rustup run nightly cargo build -p binbook-fw \
  --features firmware-bin,sd-storage \
  --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" \
  rustup run nightly cargo build -p binbook-fw \
  --features firmware-bin,diagnostic-console,sd-storage \
  --target riscv32imc-unknown-none-elf --release
```

Use available `size`, `readelf`, linker map, or `cargo size` tooling to record
RAM. This host currently provides the GNU RISC-V tools, so the cold-start
command is:

```bash
ELF="target/riscv32imc-unknown-none-elf/release/binbook-fw"
riscv64-unknown-elf-size -A "$ELF"
riscv64-unknown-elf-readelf -l "$ELF"
```

ESP32-C3 DRAM for this linker layout ends at `0x3fce0000`. From every `LOAD`
segment whose virtual address is in `0x3fc80000..0x3fce0000`, calculate
`VirtAddr + MemSiz`; linked headroom is `0x3fce0000 - max(end)`. The current
diagnostic ELF ends its stack segment at `0x3fcce400`, leaving 72,704 bytes.
Record the new arithmetic rather than copying that baseline. If the GNU tools
are unavailable, use equivalent installed tools; install LLVM components only
when dependency installation is authorized. Do not substitute
application/flash size for RAM evidence.

## 8. Acceptance matrix template

The executing agent must maintain this in `HANDOFF.md` and fill every live
evidence cell before claiming completion:

| Requirement | Implementation | Automated proof | Live evidence |
| --- | --- | --- | --- |
| Separate USB/framing/owner/SD/CRC/commit/library timings | transfer metrics and measured block device | fake clock, codec, saturation, overlap tests | trace report reconciles with wall time |
| Bounded negotiated limits | limits capability/opcode | old/new capability and invalid-limit tests | HELLO + limits output |
| Four-slot RAM staging | slot pool and descriptor channels | state transitions, budget assertions | ELF RAM + slot/RX high-water |
| Window 4/8 selection | request/clamp logic | 2/4/8 negotiation tests | controlled median comparison |
| Multi-block SD | caller scratch and block batching | call-count/boundary/error tests | physical call/block trace comparison |
| Extent strategy decision | mounted/extent accounting | FAT image, fragmentation, recovery tests | total-time and physical-block comparison |
| Bulk upload credits | upload state machine | credit, lost-ack, stale-session tests | complete upload and trace |
| Bulk readback credits | read state machine | EOF, output failure, CRC tests | full device readback |
| Legacy fallback | unchanged v2 path | old/new fake peers | forced legacy hardware run |
| Abort/reset/timeout cleanup | control lane and generation invalidation | discriminating cleanup tests | independent absence/status checks |
| Honest progress | durable offset reporting | fake clock/peer TTY and JSON tests | captured CLI output |
| Exact integrity | finish CRC and full readback | mismatch/premature EOF tests | commit CRC + `cmp` + SHA-256 |
| Zero protocol errors | strict routing and validation | malformed/duplicate/wrong-ID tests | final STATUS and trace delta |
| Derived throughput target | phase model | calculation tests | controlled medians meet target |

No row may be marked complete from compilation, an `Ok` response, a parsed
command, or transport acknowledgement alone.
