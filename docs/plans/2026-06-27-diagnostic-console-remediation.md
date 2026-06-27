# Xteink X4 Diagnostic Console Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task in the main thread. Do not delegate to subagents. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Replace transport-only diagnostic acknowledgements with a feature-gated, stateful console whose commands perform and report the behavior required by the design specification.

**Architecture:** Make `binbook-diagnostic-protocol` the single authority for every V1 payload layout and record encoding. Split firmware work into a no-hardware command engine, fixed-buffer stream transport, structured log store, flash-backed crash store, and hardware action executor in `main.rs`; make the CLI consume the same typed payload helpers and validate opcode, sequence, status, and payload before reporting success.

**Tech Stack:** Rust `no_std`, fixed caller-owned buffers, COBS, CRC-16/CCITT-FALSE, CRC32-IEEE, `esp-hal` USB Serial/JTAG, `esp-storage` through a board-local `xteink-hal::Flash` adapter, `serialport`, Cargo host tests, pinned nightly RISC-V firmware builds, Python reference tests, and live Xteink X4 verification.

---

## Scope And Completion Rules

- Treat `docs/specs/2026-06-26-x4-diagnostic-serial-console-design.md` as authoritative for command behavior.
- Preserve the frame header and opcode numbers already defined in `binbook-diagnostic-protocol`.
- Keep every firmware allocation, module, dependency, crash write, probe path, and USB diagnostic path behind `diagnostic-console`.
- Do not allocate on the firmware heap. Protocol, logging, command dispatch, crash persistence, and serial framing use caller-owned arrays and slices.
- A response is successful only after its action has completed. Display and flash failures return a non-`Ok` response and update `last_error`.
- Every implementation task starts with a failing test, uses a discriminating precondition, and ends with its focused feature-enabled tests passing.
- Run firmware behavior tests with `--features diagnostic-console`; default workspace tests do not prove feature behavior.
- Never accept parser success, an enum routing result, or an empty `Ok` response as command evidence.
- Do not run two commands that may own `/dev/ttyACM0` concurrently.
- Do not mark this plan complete until Task 11 records live hardware evidence and Task 12 has no blank acceptance-matrix cells.

## Canonical V1 Payload Layouts

Task 1 must encode these layouts as named constants and typed helpers. Later tasks must not hand-assemble them.

| Payload | Layout |
| --- | --- |
| `HELLO` response | `protocol_version:u8`, `max_frame_bytes:u16`, `capabilities:u32`, `firmware_name_len:u8`, `firmware_name[firmware_name_len]`, `target_len:u8`, `target[target_len]`; each string is ASCII and at most `HELLO_ID_MAX_BYTES = 16` |
| `KEY` request | `key:u8`, `action:u8` |
| `PAGE` request | non-goto: `action:u8`; goto: `action:u8`, `page_index:u32` |
| `PAGE` response | `current_page:u32` after the action completes |
| `STATUS` response | `current_page:u32`, `page_count:u32`, `panel_mode:u8`, `dropped_log_count:u32`, `protocol_error_count:u32`, `last_error:i32` |
| `LOG_GET` request | `cursor_sequence:u32`, `max_bytes:u16` |
| `LOG_GET` response | `next_cursor:u32`, `dropped_log_count:u32`, `record_count:u16`, followed by zero or more 24-byte log records |
| Log record | `sequence:u32`, `tick_ms:u32`, `level:u8`, `subsystem:u8`, `event:u16`, `arg0:i32`, `arg1:i32`, `arg2:i32` |
| `CRASH_GET` response | `present:u8`; when present is 1, followed by one 128-byte crash summary |
| `DISPLAY_PROBE` request | `probe:u8` |

Capability bits are `CAP_KEY = 1 << 0`, `CAP_PAGE = 1 << 1`, `CAP_STATUS = 1 << 2`, `CAP_LOG = 1 << 3`, `CAP_CRASH = 1 << 4`, and `CAP_DISPLAY_PROBE = 1 << 5`. Firmware identity is `binbook-fw`; target identity is `xteink-x4`.

The fixed 128-byte crash summary is:

```text
0..4     magic = "BBCR"
4        version = 1
5        flags
6        copied_log_count (0..4)
7        panel_mode
8..12    boot_counter:u32
12..16   last_error:i32
16..20   last_page:u32
20..24   last_log_sequence:u32
24..120  four 24-byte log record slots
120..124 reserved = 0
124..128 crc32:u32 over bytes 0..124
```

## File Map

- Modify `firmware/crates/binbook-diagnostic-protocol/src/lib.rs`: strict frame codec and all typed V1 payload codecs.
- Modify `firmware/crates/binbook-diagnostic-protocol/tests/codec.rs`: byte-layout, malformed-frame, and capacity tests.
- Modify `firmware/crates/binbook-fw/src/diag.rs`: fixed-buffer stream state, live command context, dispatch, and response construction.
- Modify `firmware/crates/binbook-fw/src/diag_log.rs`: specified 24-byte records, sequence-cursor ring, bounded dedupe, and 128-byte crash summary.
- Create `firmware/crates/binbook-fw/src/diag_flash.rs`: crash-sector storage over `xteink_hal::Flash`.
- Modify `firmware/crates/binbook-fw/src/display.rs`: concrete full-refresh, clear-white, and corner-window probe functions.
- Modify `firmware/crates/binbook-fw/src/refresh.rs`: explicit refresh-state invalidation/reset after probes when required.
- Modify `firmware/crates/binbook-fw/src/main.rs`: board adapters, command execution, structured events, partial TX handling, and response-after-action ordering.
- Modify `firmware/crates/binbook-fw/src/lib.rs`: export diagnostic modules only under `diagnostic-console`.
- Modify `firmware/crates/binbook-fw/Cargo.toml`: optional flash dependencies included only by `diagnostic-console`.
- Modify `firmware/crates/binbook-fw/tests/firmware_logic.rs`: replace transport-only assertions with behavioral tests.
- Modify `firmware/crates/ssd1677-driver/src/lib.rs`: only if a probe needs a missing reusable driver primitive; cover it with driver tests.
- Modify `cli/src/lib.rs`: typed request builders, response formatting, delimiter framing, and sequence matching.
- Modify `cli/src/main.rs`: correct command selection, timeouts, error exits, and output.
- Modify `cli/tests/protocol.rs`: request and response behavior tests.
- Create `cli/tests/serial_transport.rs`: fake-stream fragmentation, batching, sequence, timeout, and status tests.
- Create `cli/tests/hardware_diagnostic.rs`: ignored, serial-device-gated hardware stream tests.
- Modify `docs/reference/xteink-x4-firmware-flashing.md`: accurate feature compatibility and verified command workflow.
- Modify `HANDOFF.md`: current state and exact evidence, with transport-only results explicitly distinguished.

## Task 1: Canonical Typed Payloads And Strict Frames

**Files:**
- Modify: `firmware/crates/binbook-diagnostic-protocol/src/lib.rs`
- Modify: `firmware/crates/binbook-diagnostic-protocol/tests/codec.rs`

**Discriminating preconditions:** use page index `0x0102_0304`, counters larger than `u16::MAX`, negative `last_error`, nonempty identities, and frames whose declared length differs from their actual payload.

- [x] **Step 1: Write failing codec tests**

Add tests named:

```rust
page_goto_uses_action_plus_full_u32_le
status_preserves_u32_fields_and_signed_error
hello_contains_identity_version_frame_limit_and_capabilities
log_get_preserves_cursor_and_budget
log_record_is_exactly_24_bytes
crash_response_distinguishes_empty_from_present
encode_rejects_header_payload_length_mismatch
decode_rejects_trailing_raw_bytes_after_declared_payload
decode_rejects_encoded_frame_larger_than_maximum
cobs_encode_returns_output_too_small_instead_of_panicking
decode_preserves_unknown_opcode_and_safe_sequence_for_error_response
```

For `page_goto_uses_action_plus_full_u32_le`, assert the exact payload is `[PageAction::Goto as u8, 0x04, 0x03, 0x02, 0x01]`. For status, round-trip `current_page=70_001`, `page_count=80_002`, `dropped=90_003`, `protocol_errors=100_004`, and `last_error=-12`.

- [x] **Step 2: Run the focused tests and confirm failure**

```bash
cd firmware && cargo test -p binbook-diagnostic-protocol --test codec -- --nocapture
```

Expected: the new helper imports or exact-layout assertions fail; no test may pass by manually constructing the same bytes inside the test.

- [x] **Step 3: Implement the typed payload API**

Add copyable payload structs and fixed-slice helpers:

```rust
pub fn encode_hello_response(value: &HelloResponse<'_>, out: &mut [u8]) -> Result<usize, ProtocolError>;
pub fn decode_hello_response(payload: &[u8]) -> Result<HelloResponseRef<'_>, ProtocolError>;
pub fn encode_key_payload(key: KeyCode, action: KeyAction, out: &mut [u8]) -> Result<usize, ProtocolError>;
pub fn decode_key_payload(payload: &[u8]) -> Result<KeyPayload, ProtocolError>;
pub fn encode_page_payload(action: PageAction, page: Option<u32>, out: &mut [u8]) -> Result<usize, ProtocolError>;
pub fn decode_page_payload(payload: &[u8]) -> Result<PagePayload, ProtocolError>;
pub fn encode_page_response(page: u32, out: &mut [u8]) -> Result<usize, ProtocolError>;
pub fn encode_status_payload(value: StatusPayload, out: &mut [u8]) -> Result<usize, ProtocolError>;
pub fn decode_status_payload(payload: &[u8]) -> Result<StatusPayload, ProtocolError>;
pub fn encode_log_get_payload(value: LogGetPayload, out: &mut [u8]) -> Result<usize, ProtocolError>;
pub fn decode_log_get_payload(payload: &[u8]) -> Result<LogGetPayload, ProtocolError>;
pub fn encode_log_record(value: LogRecordPayload, out: &mut [u8]) -> Result<usize, ProtocolError>;
pub fn decode_log_record(payload: &[u8]) -> Result<LogRecordPayload, ProtocolError>;
pub fn encode_probe_payload(probe: ProbeCode, out: &mut [u8]) -> Result<usize, ProtocolError>;
pub fn decode_probe_payload(payload: &[u8]) -> Result<ProbeCode, ProtocolError>;
```

Add `BadPayloadLength`, `BadVersion`, and `InvalidValue` error variants. Add `RawFrameHeader { kind:u8, opcode:u8, status:u8, sequence:u16, payload_len:u16 }` so a CRC-valid frame with an unknown opcode still exposes its safe sequence and can receive `BadRequest`; convert to typed enums only after envelope validation. Make `encode_frame()` derive the encoded payload length from `payload.len()` and reject a mismatching `FrameHeader.payload_len`. Make `decode_frame()` require `raw_len == 10 + payload_len + 2`, reject `input.len() > MAX_FRAME_BYTES` before decoding, and bounds-check every COBS write.

Define `DiagLevelCode`, `DiagSubsystemCode`, and `DiagEventCode` in this shared crate so firmware emits and the CLI formats the same numeric values. Do not duplicate event-number tables in firmware and host code.

- [x] **Step 4: Run protocol tests**

```bash
cd firmware && cargo test -p binbook-diagnostic-protocol --test codec -- --nocapture
```

Expected: all codec tests pass, including exact byte assertions and undersized-buffer cases.

## Task 2: Spec-Compliant SRAM Log Records, Cursors, And Idle Dedupe

**Files:**
- Modify: `firmware/crates/binbook-fw/src/diag_log.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

**Discriminating preconditions:** overflow a four-record ring with sequences `0..7`, request cursor `4`, use nonzero ticks and three signed arguments, then clear a nonempty ring with a nonzero dropped count.

- [x] **Step 1: Write failing log behavior tests**

Add feature-gated tests:

```rust
diag_log_records_full_layout_and_tick
diag_log_cursor_is_sequence_after_overwrite
diag_log_cursor_before_oldest_starts_at_oldest_retained
diag_log_clear_removes_records_and_dropped_but_keeps_sequence_monotonic
diag_idle_transition_records_suppressed_count
diag_idle_summary_is_bounded_by_idle_summary_ms
```

The overwrite test must push eight records into `DiagLog<4>`, call `read_from_sequence(4, ...)`, and assert returned sequences are exactly `[4, 5, 6, 7]`; this prevents treating cursor `4` as an array offset. The clear test must prove the ring is nonempty and `dropped_records() > 0` before calling `clear()`.

- [x] **Step 2: Run the focused tests and confirm failure**

```bash
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_log_ -- --nocapture
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_idle_ -- --nocapture
```

Expected: failures show the old `code:u8`, `arg:u16`, zero tick, offset cursor, and missing active/idle transition behavior.

- [x] **Step 3: Implement the specified log model**

Replace `DiagEvent` and `DiagLogRecord` fields with `event:u16`, `arg0:i32`, `arg1:i32`, and `arg2:i32`; pass `tick_ms` into `push()`. Rename the default capacity to `DIAG_LOG_RECORDS` and set it to 256. Implement:

```rust
pub fn push(&mut self, tick_ms: u32, event: DiagEvent) -> u32;
pub fn read_from_sequence(&self, cursor: u32, out: &mut [DiagLogRecord]) -> LogReadResult;
pub fn oldest_sequence(&self) -> Option<u32>;
pub fn newest_sequence(&self) -> Option<u32>;
pub fn clear(&mut self);
pub fn enter_idle(&mut self, log: &mut DiagLog<N>, tick_ms: u32);
pub fn observe_idle_tick(&mut self, log: &mut DiagLog<N>, tick_ms: u32);
pub fn leave_idle(&mut self, log: &mut DiagLog<N>, tick_ms: u32);
```

Use `ADC_SAMPLE_INTERVAL_MS` and `IDLE_SUMMARY_MS` named constants. Preserve monotonically increasing sequence numbers across `clear()` so an old host cursor cannot alias new records.

- [x] **Step 4: Run feature-enabled log tests**

```bash
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_log_ -- --nocapture
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_idle_ -- --nocapture
```

Expected: all pass with nonzero ticks and exact retained sequences.

## Task 3: Stream-Safe Firmware RX And Partial TX

**Files:**
- Modify: `firmware/crates/binbook-fw/src/diag.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`
- Create: `cli/tests/hardware_diagnostic.rs`

**Discriminating preconditions:** split one request before its delimiter, feed two complete requests in one slice, prefix malformed bytes before a valid frame, and expose a TX sink that accepts only three bytes per write.

- [x] **Step 1: Write failing stream tests**

Add:

```rust
diag_serial_keeps_partial_frame_until_delimiter
diag_serial_yields_two_batched_frames_in_order
diag_serial_recovers_after_oversized_frame_at_next_delimiter
diag_serial_counts_malformed_frame_and_continues
diag_serial_partial_tx_preserves_unsent_suffix
```

The partial-frame test must call `feed_rx()` twice and assert no command is produced after the first call. The batched test must encode sequences 41 and 42, concatenate both frames, and assert both are returned in order.

Also create `cli/tests/hardware_diagnostic.rs` behind `serial-device`, mark its tests `#[ignore]`, and encode the same byte-by-byte and two-frame batched cases for later execution against `/dev/ttyACM0`. Add a third case that sends a CRC-invalid frame with a safely readable sequence, then sends STATUS and asserts `protocol_error_count` increased without wedging the stream. At this task the required assertion is that the ignored hardware tests compile and appear under `cargo test -- --list`; they are not evidence until Task 11 runs them on the device.

- [x] **Step 2: Run tests and confirm the current clear-on-error behavior fails**

```bash
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_serial_ -- --nocapture
```

- [x] **Step 3: Implement delimiter-oriented fixed-buffer transport**

Refactor `SerialState` so `feed_rx()` scans for `FRAME_DELIMITER`, stops after one complete frame is ready, and returns the number of input bytes consumed. `main.rs` must call it repeatedly with the unconsumed suffix after draining `next_frame()`, allowing two frames in one USB read without allocating two 512-byte frame slots. Leave partial bytes untouched and add a discard-until-delimiter state after overflow. Expose:

```rust
pub fn feed_rx(&mut self, bytes: &[u8]) -> FeedResult; // includes consumed byte count
pub fn next_frame(&mut self, out: &mut [u8; MAX_FRAME_BYTES]) -> Option<usize>;
pub fn queue_tx(&mut self, frame: &[u8]) -> Result<(), TransportError>;
pub fn pending_tx(&self) -> &[u8];
pub fn consume_tx(&mut self, written: usize);
pub fn protocol_error_count(&self) -> u32;
```

Do not call `decode_frame()` until a delimiter has completed a frame. Increment the protocol error counter for malformed or oversized frames and continue at the next delimiter.

- [x] **Step 4: Run transport and codec regression tests**

```bash
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_serial_ -- --nocapture
cd firmware && cargo test -p binbook-diagnostic-protocol --test codec
```

## Task 4: Stateful HELLO, KEY, PAGE, And STATUS Execution

**Files:**
- Modify: `firmware/crates/binbook-fw/src/diag.rs`
- Modify: `firmware/crates/binbook-fw/src/input.rs`
- Modify: `firmware/crates/binbook-fw/src/main.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

**Discriminating preconditions:** start at page 3 of 8 before `goto 0`; request `goto 6` from page 2; request `goto 3` while already on page 3; use status fields larger than 65,535 and a nonzero negative error.

- [x] **Step 1: Replace weak routing tests with failing outcome tests**

Add tests that assert exact values, not just variants:

```rust
diag_key_right_matches_physical_button_target
diag_key_left_matches_physical_button_target
diag_all_key_codes_match_physical_mapping
diag_page_goto_zero_from_nonzero_targets_zero
diag_page_goto_nonadjacent_targets_exact_page
diag_page_goto_current_is_no_render_and_stays_current
diag_page_next_and_previous_clamp_at_edges
diag_hello_response_has_all_required_fields
diag_status_response_uses_live_state_without_truncation
diag_invalid_page_payload_returns_bad_request
```

Represent expected actions as `CommandAction::RenderPage { target_page }`, not one-step `PageTurn` for goto. Assert `RIGHT` and physical `Button::Right` both resolve through the same `target_page_for_button()` helper.

- [x] **Step 2: Run failing command tests**

```bash
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_page_ -- --nocapture
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_hello_ -- --nocapture
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_status_ -- --nocapture
```

- [x] **Step 3: Implement live command context and response-after-action flow**

Add:

```rust
pub struct DiagnosticState {
    pub current_page: u32,
    pub page_count: u32,
    pub panel_mode: PanelModeCode,
    pub last_error: i32,
}

pub struct CommandContext<'a, const N: usize> {
    pub state: &'a mut DiagnosticState,
    pub log: &'a mut DiagLog<N>,
    pub protocol_error_count: u32,
    pub tick_ms: u32,
}

pub fn dispatch_command<const N: usize>(
    header: FrameHeader,
    payload: &[u8],
    context: &mut CommandContext<'_, N>,
) -> DispatchResult;
```

Validate `FrameKind::Request` and request `Status::Ok`. Return `BadRequest` for invalid lengths/values. `main.rs` must execute `RenderPage { target_page }` through the same `render_current_page()` used by physical buttons, update `current_page` only after success, then build the `PAGE` or `KEY` response. Extend the CLI timeout later rather than sending a false success before rendering.

- [x] **Step 4: Run focused tests and feature builds**

```bash
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_page_ -- --nocapture
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_hello_ -- --nocapture
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_status_ -- --nocapture
```

## Task 5: LOG_GET, LOG_CLEAR, And Structured Runtime Events

**Files:**
- Modify: `firmware/crates/binbook-fw/src/diag.rs`
- Modify: `firmware/crates/binbook-fw/src/diag_log.rs`
- Modify: `firmware/crates/binbook-fw/src/main.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

**Discriminating preconditions:** generate command receipt, page render start, render result, and idle events before retrieval; request a cursor after ring overwrite; clear only after proving records and dropped count are nonzero.

- [x] **Step 1: Write failing end-to-end log command tests**

Add:

```rust
diag_log_get_returns_known_command_and_render_records
diag_log_get_honors_sequence_cursor_after_overwrite
diag_log_get_honors_byte_budget_on_record_boundaries
diag_log_clear_clears_nonempty_ring_and_dropped_count
diag_log_clear_response_is_not_log_get
diag_command_error_is_logged_immediately
diag_refresh_panel_and_display_error_events_are_emitted
```

The byte-budget test must use a budget that fits exactly two records and assert `record_count == 2` plus a `next_cursor` equal to the next sequence. The clear test must fetch at least one record before clearing and fetch zero afterward.

- [x] **Step 2: Run failing log command tests**

```bash
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_log_get_ -- --nocapture
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_log_clear_ -- --nocapture
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_runtime_event_ -- --nocapture
```

- [x] **Step 3: Connect logs to dispatch and rendering**

Pass the live ring through `CommandContext`. Add named `u16` events for command receipt/error, key, page decision, render start/success/failure, refresh decision, panel mode, ADC sample, idle entered/summary/left, and display error. Change `render_current_page()` to return `HalResult<RenderReport>` containing the refresh decision and final panel mode; log success or failure in the caller with the current tick.

Implement `LOG_GET` by encoding whole records only into the response budget, capped by `MAX_PAYLOAD_BYTES`. Implement `LOG_CLEAR` as a distinct opcode that clears a proven live ring and returns the post-clear next sequence and dropped count.

- [x] **Step 4: Run focused tests**

```bash
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_log_ -- --nocapture
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_runtime_event_ -- --nocapture
```

## Task 6: Complete Crash Summary And Flash Persistence

**Files:**
- Create: `firmware/crates/binbook-fw/src/diag_flash.rs`
- Modify: `firmware/crates/binbook-fw/src/diag_log.rs`
- Modify: `firmware/crates/binbook-fw/src/flash.rs`
- Modify: `firmware/crates/binbook-fw/src/lib.rs`
- Modify: `firmware/crates/binbook-fw/src/diag.rs`
- Modify: `firmware/crates/binbook-fw/src/main.rs`
- Modify: `firmware/crates/binbook-fw/Cargo.toml`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

**Discriminating preconditions:** initialize flash to `0xFF`, write a summary with four distinct recent records, recreate the store to simulate reset, corrupt one CRC byte, and clear a known-present record.

- [x] **Step 1: Write failing crash model and store tests**

Add:

```rust
diag_crash_summary_roundtrips_all_required_fields_and_four_records
diag_crash_store_empty_flash_returns_none
diag_crash_store_survives_reopen
diag_crash_store_rejects_bad_crc
diag_crash_clear_erases_known_present_summary
diag_crash_get_distinguishes_empty_and_present
diag_crash_clear_uses_distinct_opcode
diag_crash_sector_does_not_overlap_file_payload_region
diag_crash_store_writes_only_on_fatal_or_explicit_clear
```

The reopen test must drop the first `CrashStore<MockFlash>`, construct a second store over the same bytes, and compare every field. The clear test must assert `Some` before clear and `None` afterward.

- [x] **Step 2: Run failing crash tests**

```bash
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_crash_ -- --nocapture
```

- [x] **Step 3: Implement bounded persistence**

Use `CRASH_RECORD_BYTES = 128`, `CRASH_LOG_RECORDS = 4`, and CRC32-IEEE. Store `boot_counter = 0` when no durable boot counter source is available; do not invent one. Reserve the final 4 KiB sector of the existing 192 KiB storage region:

```rust
pub const CRASH_SECTOR_SIZE: u32 = 4096;
pub const CRASH_SECTOR_OFFSET: u32 = STORAGE_OFFSET + STORAGE_SIZE - CRASH_SECTOR_SIZE;
```

Reduce or validate file-payload bounds so files cannot overlap that sector. Implement `CrashStore<F: xteink_hal::Flash>::read()`, `write_fatal()`, and `clear()`. Do not write from the idle loop; write only on a fatal error path or an explicit internal fatal-summary flush.

Under `diagnostic-console`, add `esp-storage = { version = "0.9.0", features = ["esp32c3"], optional = true }` and `embedded-storage = { version = "0.3.1", optional = true }`. In `main.rs`, define the board-local newtype `X4Flash(esp_storage::FlashStorage)` and implement `xteink_hal::Flash` through `ReadNorFlash`/`NorFlash`, mapping failures to `HalError::Flash`.

- [x] **Step 4: Wire CRASH_GET and CRASH_CLEAR**

`CRASH_GET` reads and validates flash, returning `present=0` for erased flash, `present=1` plus 128 bytes for a valid summary, and `InternalError` for invalid CRC or flash failure. `CRASH_CLEAR` erases the sector and verifies a subsequent read is empty before returning `Ok`. Initialize the store once in `main.rs`; fatal display failures call `write_fatal()` with the live page, panel mode, error, last sequence, and four most recent records before returning the command error response.

- [x] **Step 5: Run crash tests and diagnostic build**

```bash
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_crash_ -- --nocapture
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
```

## Task 7: Execute All Display Probes And Report Hardware Failures

**Files:**
- Modify: `firmware/crates/binbook-fw/src/display.rs`
- Modify: `firmware/crates/binbook-fw/src/refresh.rs`
- Modify: `firmware/crates/binbook-fw/src/diag.rs`
- Modify: `firmware/crates/binbook-fw/src/main.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`
- Modify: `firmware/crates/ssd1677-driver/src/lib.rs`

**Discriminating preconditions:** begin on page 2, make a fake display fail on its second plane write, and assert each probe leaves a distinct observable call trace rather than merely producing a routing enum.

- [x] **Step 1: Write failing probe execution tests**

Add:

```rust
diag_probe_full_refresh_executes_current_page_full_path
diag_probe_clear_white_writes_both_planes_and_refreshes
diag_probe_window_corners_writes_all_four_physical_corners_on_both_planes
diag_probe_failure_returns_internal_error_and_logs_display_error
diag_probe_success_logs_render_result
```

At the driver layer, assert the corner probe issues windows `(0,0,128,96)`, `(672,0,128,96)`, `(0,384,128,96)`, and `(672,384,128,96)` to both RAM planes. The failure test must inject a real `HalError`, not return a preselected response status.

- [x] **Step 2: Run failing probe tests**

```bash
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_probe_ -- --nocapture
cd firmware && cargo test -p ssd1677-driver probe_ -- --nocapture
```

- [x] **Step 3: Implement concrete probe functions**

Add `#[cfg(feature = "diagnostic-console")]` functions `display_full_refresh_current()`, `display_clear_white_probe()`, and `display_window_corners_probe()` in `display.rs`. Full refresh must bypass a same-page `RefreshDecision::Noop` and stream the current page through the grayscale full-refresh path. Clear white must initialize BW mode, clear both RAM planes, refresh, and invalidate differential refresh history. Window corners must initialize BW mode, clear both planes to white, write the known corner windows to both planes, perform a full refresh, and invalidate refresh history.

Make `main.rs` execute the selected function, then build the response. Do not send `Ok` before the function returns. On failure, preserve the current page, set `last_error`, log `DisplayError`, and return `InternalError`.

- [x] **Step 4: Run probe tests and diagnostic build**

```bash
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diag_probe_ -- --nocapture
cd firmware && cargo test -p ssd1677-driver
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
```

## Task 8: Correct CLI Requests, Responses, And Sequence Validation

**Files:**
- Modify: `cli/src/lib.rs`
- Modify: `cli/src/main.rs`
- Modify: `cli/tests/protocol.rs`
- Create: `cli/tests/serial_transport.rs`

**Discriminating preconditions:** encode goto page `0x0102_0304`, build clear commands with nonempty stores assumed, place a mismatched-sequence valid frame before the expected response, split delimiters across reads, and return a non-`Ok` status with a valid CRC.

- [x] **Step 1: Write failing CLI behavior tests**

Add:

```rust
cli_page_goto_encodes_full_u32
cli_page_next_and_previous_use_page_action_values
cli_page_first_last_and_current_use_page_action_values
cli_logs_since_encodes_cursor_and_budget
cli_logs_clear_sends_log_clear
cli_crash_clear_sends_crash_clear
cli_probe_supports_all_three_probe_codes
cli_status_decodes_canonical_u32_layout
cli_hello_formats_identity_and_capabilities
cli_logs_formats_event_names_and_sequences
cli_crash_formats_empty_and_present_distinctly
serial_transport_reassembles_fragmented_cobs_frame
serial_transport_skips_mismatched_sequence
serial_transport_rejects_non_ok_status
serial_transport_times_out_without_matching_response
```

- [x] **Step 2: Run tests and confirm failure**

```bash
cd cli && cargo test --test protocol -- --nocapture
cd cli && cargo test --features serial-device --test serial_transport -- --nocapture
```

- [x] **Step 3: Use only shared typed codecs**

Replace `page_goto_request(sequence, u16)` with `page_request(sequence, PagePayload)` using `u32`. Extend the clap `PageAction` with `first`, `last`, and `current`, and `ProbeCommand` with `full-refresh-current` and `clear-white`. Add separate builders for log get/clear, crash get/clear, and every probe. Add testable `format_response()` and make non-`Ok`, wrong opcode, wrong kind, or wrong sequence return an error that causes exit code 1.

Replace literal `BB` scanning with delimiter accumulation. Change `send_and_receive()` to accept expected opcode and sequence, decode each complete frame, skip unrelated valid frames, and wait up to a command-specific timeout: 2 seconds for read-only commands, 30 seconds for page/key rendering, and 60 seconds for display probes.

- [x] **Step 4: Run CLI tests**

```bash
cd cli && cargo test
cd cli && cargo test --features serial-device
```

Expected: tests prove exact request bytes and semantic response validation, not only clap parsing.

## Task 9: Resolve `debug-log` And Diagnostic USB Ownership

**Files:**
- Modify: `firmware/crates/binbook-fw/src/main.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`
- Modify: `docs/reference/xteink-x4-firmware-flashing.md`
- Modify: `docs/specs/2026-06-26-x4-diagnostic-serial-console-design.md`

**Discriminating precondition:** build with both `diagnostic-console` and `debug-log`; the resulting code must leave USB Serial/JTAG exclusively available to the packet console.

- [x] **Step 1: Write a failing feature-combination test**

Add a source/compile behavior test named `diagnostic_console_takes_usb_ownership_over_debug_log` that requires the `dbgprintln!` expansion to be a no-op whenever `diagnostic-console` is enabled, even if `debug-log` is also enabled. It must reject the current `#[cfg(feature = "debug-log")] use esp_println::println;` condition.

- [x] **Step 2: Run the failing test**

```bash
cd firmware && cargo test -p binbook-fw --features diagnostic-console,debug-log --test firmware_logic diagnostic_console_takes_usb_ownership_over_debug_log -- --nocapture
```

- [x] **Step 3: Make packet transport authoritative**

Gate `esp_println` imports and the printing macro with `all(feature = "debug-log", not(feature = "diagnostic-console"))`. When both features are selected, compile structured logs and packet transport but no serial text prints. Update the design and flashing reference to state this precedence explicitly; remove the claim that text mirroring works while the packet console owns USB.

- [x] **Step 4: Build every supported feature combination**

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,debug-log --target riscv32imc-unknown-none-elf --release
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console,debug-log --target riscv32imc-unknown-none-elf --release
```

## Task 10: Replace False-Positive Tests With Integrated Behavioral Coverage

**Files:**
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`
- Modify: `firmware/crates/binbook-diagnostic-protocol/tests/codec.rs`
- Modify: `cli/tests/protocol.rs`
- Modify: `cli/tests/serial_transport.rs`

**Discriminating preconditions:** the test fixture starts on page 3, contains a nonempty log with dropped records, contains a present crash summary, injects one display failure, and includes one unrelated response sequence before the expected one.

- [x] **Step 1: Add a failing integrated acceptance test**

Create `diagnostic_command_acceptance_fixture_rejects_noops`. It must drive encoded requests through frame parsing, dispatch, a fake action executor, response encoding, and host response decoding for all nine opcodes. Assert state before and after each mutating command. The test must fail if any opcode returns empty `Ok` without its specified effect.

- [x] **Step 2: Run the integrated test and confirm it detects remaining gaps**

```bash
cd firmware && cargo test -p binbook-fw --features diagnostic-console --test firmware_logic diagnostic_command_acceptance_fixture_rejects_noops -- --nocapture
```

- [x] **Step 3: Remove or strengthen weak tests**

Delete assertions that only search source strings or accept any enum variant when a behavioral test now covers the requirement. In key tests, assert the returned target page. In edge tests, assert the exact clamped page. In probe tests, assert hardware call traces. Keep feature-gate tests only for proving compile-time absence.

- [x] **Step 4: Run clean full host verification**

```bash
cd firmware && cargo clean && cargo test --workspace --features diagnostic-console
cd firmware && cargo test --workspace
cd cli && cargo test
cd cli && cargo test --features serial-device
uv run pytest -q
if rg -n "extern crate alloc|Vec<|Box<|String<" firmware/crates/binbook-diagnostic-protocol/src firmware/crates/binbook-fw/src/diag.rs firmware/crates/binbook-fw/src/diag_log.rs firmware/crates/binbook-fw/src/diag_flash.rs; then exit 1; fi
if rg -n "serde_json|ciborium|prost|protobuf|SquidScript|Opcode::(List|Upload|Delete)" firmware/crates/binbook-diagnostic-protocol firmware/crates/binbook-fw/src/diag.rs; then exit 1; fi
git diff --check
```

Expected: all tests pass; fixed-buffer and forbidden-protocol audits print no matches; the feature-enabled firmware command is recorded separately from the default workspace command.

## Task 11: Live Hardware Verification With Discriminating State

**Files:**
- Modify: `HANDOFF.md`

**Discriminating preconditions:** page goto starts from page 2 or 3, log clear starts with retrieved records, crash clear records before/after state, and each visible probe records the panel state before and after execution.

- [x] **Step 1: Run a failing hardware-evidence precondition check**

Before touching the device, add a temporary `Hardware Remediation Verification` section to `HANDOFF.md` with rows for flash, boot capture, HELLO, STATUS, KEY, PAGE, logs, crash, three probes, stream fragmentation, batching, and combined-feature ownership. Leave the result cells blank, then run:

```bash
python3 - <<'PY'
from pathlib import Path
text = Path('HANDOFF.md').read_text()
for evidence in [
    'flash', 'boot-capture', 'hello', 'status', 'key', 'page', 'logs',
    'crash', 'probe-full', 'probe-clear', 'probe-corners', 'fragmented',
    'batched', 'combined-features',
]:
    marker = f'| HW-{evidence} |'
    row = next(line for line in text.splitlines() if line.startswith(marker))
    cells = [cell.strip() for cell in row.strip('|').split('|')]
    assert len(cells) == 2 and cells[1], f'missing evidence: {evidence}'
PY
```

Expected: fail on `HW-flash`. Keep the validator and fill each result immediately after the corresponding hardware check.

- [x] **Step 2: Run the hardware test compile check before hardware access**

```bash
cd cli && cargo test --features serial-device --test hardware_diagnostic -- --list
```

Expected: the ignored tests compile and are listed; do not run them concurrently with other serial commands.

- [x] **Step 3: Flash diagnostic firmware**

Run from the repository root with host/device access:

```bash
FW_FEATURES="firmware-bin,diagnostic-console" firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Expected: flash completes and the device boots the four-page navigation fixture.

- [x] **Step 4: Capture the mandatory 15-second serial boot record**

Run the exact `uv run --with pyserial --no-project python3 -c ...` monitor command from `AGENTS.md`. Record its full relevant output in `HANDOFF.md`. Do not run the CLI while the monitor owns the port.

- [x] **Step 5: Verify HELLO and live STATUS payloads**

```bash
cd cli && cargo run --features serial-device -- diag hello --port /dev/ttyACM0
cd cli && cargo run --features serial-device -- diag status --port /dev/ttyACM0
```

Expected: HELLO prints protocol 1, max frame 512, all six capability names, firmware `binbook-fw`, and target `xteink-x4`. STATUS prints live page, page count 4, actual panel mode, dropped count, protocol error count, and last error.

- [x] **Step 6: Verify KEY through the physical navigation path**

Run sequentially:

```bash
cd cli && cargo run --features serial-device -- diag page --port /dev/ttyACM0 goto 0
cd cli && cargo run --features serial-device -- diag key --port /dev/ttyACM0 RIGHT
cd cli && cargo run --features serial-device -- diag status --port /dev/ttyACM0
cd cli && cargo run --features serial-device -- diag key --port /dev/ttyACM0 LEFT
cd cli && cargo run --features serial-device -- diag status --port /dev/ttyACM0
```

Expected: visible page and independent STATUS move `0 -> 1 -> 0`; record the visible result, not only the acknowledgements.

- [x] **Step 7: Verify direct PAGE actions from discriminating starting states**

```bash
cd cli && cargo run --features serial-device -- diag page --port /dev/ttyACM0 goto 3
cd cli && cargo run --features serial-device -- diag status --port /dev/ttyACM0
cd cli && cargo run --features serial-device -- diag page --port /dev/ttyACM0 goto 0
cd cli && cargo run --features serial-device -- diag status --port /dev/ttyACM0
cd cli && cargo run --features serial-device -- diag page --port /dev/ttyACM0 next
cd cli && cargo run --features serial-device -- diag page --port /dev/ttyACM0 previous
cd cli && cargo run --features serial-device -- diag page --port /dev/ttyACM0 last
cd cli && cargo run --features serial-device -- diag status --port /dev/ttyACM0
cd cli && cargo run --features serial-device -- diag page --port /dev/ttyACM0 first
cd cli && cargo run --features serial-device -- diag status --port /dev/ttyACM0
cd cli && cargo run --features serial-device -- diag page --port /dev/ttyACM0 current
```

Expected: `goto 3` reaches exactly 3, `goto 0` reaches exactly 0 from 3, next/previous use their actual action codes, last/first reach 3/0, and current reports 0 without an extra render. Record before/after status and visual page results.

- [x] **Step 8: Verify nonempty logs and clear semantics**

```bash
cd cli && cargo run --features serial-device -- diag logs --port /dev/ttyACM0 --since 0
cd cli && cargo run --features serial-device -- diag logs --port /dev/ttyACM0 --clear
cd cli && cargo run --features serial-device -- diag logs --port /dev/ttyACM0 --since 0
```

Expected: the first response contains increasing command, page, render, refresh, and panel records; clear reports `LogClear`; the final retrieval contains no pre-clear records.

- [x] **Step 9: Verify crash empty/clear behavior**

```bash
cd cli && cargo run --features serial-device -- diag crash --port /dev/ttyACM0 --clear
cd cli && cargo run --features serial-device -- diag crash --port /dev/ttyACM0
```

Expected: clear reports `CrashClear`; get explicitly prints `no crash summary`. Automated Task 6 tests remain the required evidence for present-summary persistence and bad CRC because hardware verification must not induce a fatal fault merely to create a record.

- [x] **Step 10: Verify every display probe visibly**

Run one at a time and record the visible result after each:

```bash
cd cli && cargo run --features serial-device -- diag probe --port /dev/ttyACM0 window-corners
cd cli && cargo run --features serial-device -- diag probe --port /dev/ttyACM0 clear-white
cd cli && cargo run --features serial-device -- diag probe --port /dev/ttyACM0 full-refresh-current
```

Expected: four physical corner boxes, then an all-white panel, then the current page restored by a full refresh. A zero-length `Ok` without the visible result fails this task.

- [x] **Step 11: Run live fragmented and batched stream checks**

```bash
cd cli && cargo test --features serial-device --test hardware_diagnostic -- --ignored --nocapture --test-threads=1
```

Expected: byte-by-byte and two-frame batched requests return the matching sequences without timeouts; the malformed-frame test receives an error or observes the incremented protocol-error counter and then completes a valid STATUS request.

- [x] **Step 12: Verify combined feature ownership**

```bash
FW_FEATURES="firmware-bin,diagnostic-console,debug-log" firmware/scripts/flash-xteink-x4-nav-probe.sh
cd cli && cargo run --features serial-device -- diag hello --port /dev/ttyACM0
```

Expected: HELLO still succeeds because packet transport owns USB and text printing is compiled out. Reflash `firmware-bin,diagnostic-console` afterward so the device is left in the documented diagnostic configuration.

- [x] **Step 13: Replace false completion text in HANDOFF.md**

Rewrite the current-state, host evidence, hardware evidence, blockers, and remaining-work sections. Include exact commands, outputs, initial state, final state, visual observations, and failures. Do not retain the old claim that empty `Ok` responses proved behavior.

## Task 12: Final Acceptance Matrix And Adversarial Completion Audit

**Files:**
- Modify: `HANDOFF.md`
- Modify: `docs/reference/xteink-x4-firmware-flashing.md`
- Modify: `docs/plans/2026-06-27-diagnostic-console-remediation.md` only to check completed boxes during execution

**Discriminating precondition:** the evidence validator must fail if any matrix row lacks an implementation path, automated test, observed evidence, or explicit `Not applicable` hardware rationale.

- [x] **Step 1: Add the acceptance matrix with deliberately blank evidence cells and run a failing validator**

Add the matrix below to `HANDOFF.md`, initially leaving evidence cells blank. Run:

```bash
python3 - <<'PY'
from pathlib import Path
text = Path('HANDOFF.md').read_text()
required = [f'DC-{i:02d}' for i in range(1, 25)]
for requirement in required:
    row = next(line for line in text.splitlines() if line.startswith(f'| {requirement} '))
    cells = [cell.strip() for cell in row.strip('|').split('|')]
    assert len(cells) == 6, (requirement, cells)
    assert all(cells[1:]), f'{requirement} has blank evidence cells'
PY
```

Expected: fail on the first blank row. This proves documentation cannot pass before evidence is populated.

- [x] **Step 2: Populate every matrix row from actual evidence**

Use these mandatory rows:

| ID | Requirement | Implementation path | Automated test | Observed evidence | Hardware evidence |
| --- | --- | --- | --- | --- | --- |
| DC-01 | Strict COBS/CRC frame validation and maximum sizes | `binbook-diagnostic-protocol::encode_frame/decode_frame` | `decode_rejects_*`, `cobs_encode_returns_*` | Codec test transcript | Fragmented/batched live test |
| DC-02 | HELLO identity, version, frame limit, capabilities | `diag::dispatch_command`, `diag_protocol::format_response` | `hello_contains_*`, `diag_hello_response_*` | Decoded host response | Live HELLO output |
| DC-03 | KEY shares physical navigation behavior | `diag::dispatch_command`, `input::target_page_for_button` | `diag_all_key_codes_match_physical_mapping` | Integrated fixture state delta | Visible RIGHT/LEFT plus STATUS |
| DC-04 | PAGE next/previous/first/last/goto/current | `protocol::encode_page_payload`, `diag::dispatch_command` | `diag_page_*` exact outcome tests | Integrated fixture state delta | Goto from nonzero plus all actions |
| DC-05 | STATUS reports live untruncated state | `protocol::encode_status_payload`, `diag::DiagnosticState` | `status_preserves_u32_fields_and_signed_error`, `diag_status_*` | Integrated fixture decoded payload | Live STATUS output |
| DC-06 | LOG_GET cursor and byte budget | `DiagLog::read_from_sequence`, `diag::dispatch_command` | `diag_log_get_honors_*` | Retrieved fixture records | Nonempty live log output |
| DC-07 | LOG_CLEAR clears known records and dropped count | `DiagLog::clear`, `diag::dispatch_command` | `diag_log_clear_clears_nonempty_ring_and_dropped_count` | Before/after fixture retrieval | Before/clear/after output |
| DC-08 | Crash summary complete binary model and CRC | `diag_log::CrashSummary::encode/decode` | `diag_crash_summary_roundtrips_*`, `diag_crash_store_rejects_bad_crc` | Host test transcript | Not applicable: fatal fault not induced |
| DC-09 | Crash persistence survives store reopen | `diag_flash::CrashStore` | `diag_crash_store_survives_reopen` | Reopened fake-flash state | Empty/clear live flash behavior |
| DC-10 | CRASH_GET distinguishes empty, valid, invalid | `diag_flash::CrashStore::read`, `diag::dispatch_command` | `diag_crash_get_distinguishes_empty_and_present`, bad-CRC test | Integrated fixture responses | Live explicit empty result |
| DC-11 | CRASH_CLEAR erases a known summary | `diag_flash::CrashStore::clear`, `diag::dispatch_command` | `diag_crash_clear_erases_known_present_summary` | Present-before/empty-after fixture | Live clear opcode/output |
| DC-12 | Full-refresh-current probe executes | `display::display_full_refresh_current` | `diag_probe_full_refresh_executes_current_page_full_path` | Fake display call trace | Visible restored page |
| DC-13 | Clear-white probe executes | `display::display_clear_white_probe` | `diag_probe_clear_white_writes_both_planes_and_refreshes` | Driver call trace | Visible white panel |
| DC-14 | Window-corners probe executes | `display::display_window_corners_probe` | `diag_probe_window_corners_writes_all_four_physical_corners_on_both_planes` | Driver call trace | Visible corner pattern |
| DC-15 | Structured events include ticks and three arguments | `diag_log::DiagLogRecord`, `main::render_current_page` caller | `diag_log_records_full_layout_and_tick`, `diag_refresh_panel_and_display_error_events_are_emitted` | Decoded fixture records | Live log output |
| DC-16 | Firmware RX handles partial, batched, malformed frames | `diag::SerialState` | `diag_serial_*` | Fixed-buffer stream transcript | Ignored live stream test |
| DC-17 | Host waits for matching sequence and rejects errors | `serial_transport::SerialSession::send_and_receive` | `serial_transport_*` | Fake serial transcript | Batched live test |
| DC-18 | Diagnostic feature compiles out and packet transport owns USB when enabled | Cargo features and `main.rs` cfg blocks | `diagnostic_console_takes_usb_ownership_over_debug_log`, default/feature builds | Four build transcripts | Combined-feature HELLO |
| DC-19 | Documentation distinguishes transport from behavior | `HANDOFF.md`, flashing reference | Hardware and acceptance evidence validators | Completed matrix | Exact hardware transcript |
| DC-20 | Firmware protocol and logs allocate no heap | Fixed arrays in protocol, `SerialState`, `DiagLog`, and `CrashStore` | Source/API audit plus target build | Release size/build transcript | Diagnostic firmware boots |
| DC-21 | No JSON/CBOR/protobuf/SquidScript compatibility or content management is added | Protocol crate and CLI command surface | `rg` forbidden-protocol audit and clap tests | Audit transcript | Not applicable: non-goal |
| DC-22 | Flash writes occur only for fatal summaries or explicit clear | `diag_flash::CrashStore`, fatal error caller | fake-flash write-count test | Write-count transcript | Empty/clear live flash behavior |
| DC-23 | Default CLI has no serial dependency at runtime | `serial-device` feature gates in `cli/Cargo.toml` and `cli/src/lib.rs` | `cargo test` without features | Default CLI build transcript | Not applicable: host build property |
| DC-24 | Unknown/malformed requests return an error when sequence is safe and increment counters otherwise | `protocol::RawFrameHeader`, `diag::SerialState`, `diag::dispatch_command` | unknown-opcode and malformed-frame tests | Error response/counter transcript | Live STATUS after malformed stream test |

- [x] **Step 3: Run the evidence validator and adversarial source audit**

```bash
python3 - <<'PY'
from pathlib import Path
text = Path('HANDOFF.md').read_text()
for requirement in [f'DC-{i:02d}' for i in range(1, 25)]:
    row = next(line for line in text.splitlines() if line.startswith(f'| {requirement} '))
    cells = [cell.strip() for cell in row.strip('|').split('|')]
    assert len(cells) == 6, (requirement, cells)
    assert all(cells[1:]), f'{requirement} has blank evidence cells'
for forbidden in ['all diagnostic commands work', 'payload_len=0 proves']:
    assert forbidden not in text.lower(), forbidden
PY
rg -n "Opcode::(Hello|Key|Page|Status|LogGet|LogClear|CrashGet|CrashClear|DisplayProbe)" firmware/crates/binbook-fw/src/diag.rs
rg -n "NoAction|payload_len=0|hard-coded|transport-only|unverified" HANDOFF.md firmware/crates/binbook-fw/src cli/src
```

Expected: validator passes; opcode audit shows an explicit behavior or explicit error for every opcode; any remaining `NoAction` is justified by a tested semantic no-op such as `PAGE current`.

- [x] **Step 4: Run the final completion gate**

```bash
cd firmware && cargo clean && cargo test --workspace --features diagnostic-console
cd firmware && cargo test --workspace
cd cli && cargo test
cd cli && cargo test --features serial-device
uv run pytest -q
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
git diff --check
```

Completion requires all commands above to pass, all Task 11 hardware evidence to be present, and every acceptance-matrix cell to contain a concrete path, test, and observation. If hardware is unavailable or any visible result is unverified, leave this plan incomplete and state that blocker in `HANDOFF.md`.
