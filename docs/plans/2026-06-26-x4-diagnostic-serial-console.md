# Xteink X4 Diagnostic Serial Console Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task in the main thread. Steps use checkbox (`- [ ]`) syntax for tracking. Do not delegate to subagents in this repo.

**Goal:** Build a feature-gated diagnostic serial console that can drive the real Xteink X4 device, retrieve logs, and run display probes through compact COBS-framed binary packets.

**Architecture:** Add a shared no-std protocol crate for COBS framing, CRC, fixed packet headers, and opcode payloads. Wire the protocol into `binbook-fw` only behind `diagnostic-console`, route key/page commands through existing navigation/render paths, and extend the Rust CLI behind `serial-device`.

**Tech Stack:** Rust no-std firmware crates, ESP32-C3 `esp-hal` USB Serial/JTAG, Rust CLI with optional `serialport`, host tests with `cargo test`, pinned nightly firmware build, real Xteink X4 hardware verification.

---

## Ground Rules

- Use TDD for every task: write the failing test first, run it, implement the smallest passing change, run the focused tests, then run the broader relevant checks.
- Do not use heap allocation in firmware-facing protocol code.
- Do not add JSON, CBOR, protobuf, or SquidScript protocol compatibility.
- Keep all diagnostic console firmware code behind `diagnostic-console`.
- Do not let `debug-log` be required for diagnostic packet transport.
- Convert current diagnostic `dbgprintln!` sites into structured diagnostic log
  events where they describe navigation, input, refresh, panel mode, command
  handling, or display errors. `debug-log` may mirror these events to text, but
  must not be the only diagnostic path.
- Deduplicate idle logging. Idle must be logged as transitions or bounded
  summaries, never once per main-loop tick.
- Do not run hardware or serial commands in parallel with any other command that may own `/dev/ttyACM0`.
- Hardware verification is a completion gate. Do not mark this plan complete until the real device has been flashed, driven over serial, visually checked, and `HANDOFF.md` updated with evidence.

## File Map

- Create `firmware/crates/binbook-diagnostic-protocol/`: shared no-std protocol crate used by firmware and CLI.
- Modify `firmware/Cargo.toml`: add the new protocol crate to the firmware workspace.
- Modify `firmware/crates/binbook-fw/Cargo.toml`: add optional protocol dependency and `diagnostic-console` feature.
- Modify `firmware/crates/binbook-fw/src/main.rs`: initialize USB Serial/JTAG and poll diagnostic packets under the feature.
- Create `firmware/crates/binbook-fw/src/diag.rs`: firmware command dispatch, log access, status responses, and display probe routing.
- Create `firmware/crates/binbook-fw/src/diag_log.rs`: SRAM log ring and compact crash summary model.
- Modify `firmware/crates/binbook-fw/src/lib.rs`: export diagnostic modules only when enabled or testable.
- Modify `firmware/crates/binbook-fw/src/input.rs`: expose a logical command path if needed so serial `KEY` and physical buttons share page-turn behavior.
- Modify `firmware/crates/binbook-fw/src/display.rs`: expose named diagnostic display probes behind the feature.
- Modify `firmware/crates/binbook-fw/tests/firmware_logic.rs`: add firmware-facing tests for routing, logs, gates, and probes.
- Modify `cli/Cargo.toml`: depend on the shared protocol crate and keep serial transport behind `serial-device`.
- Modify `cli/src/lib.rs`: replace text protocol helpers with binary protocol helpers or add them beside existing helpers until old tests are updated.
- Modify `cli/src/main.rs`: add `diag` subcommands and serial request/response handling.
- Modify `cli/tests/protocol.rs`: add CLI protocol and command formatting tests.
- Modify `docs/reference/xteink-x4-firmware-flashing.md`: document diagnostic build, flash, and CLI workflow.
- Modify `HANDOFF.md`: record final verification evidence.

## Task 1: Shared Protocol Crate Skeleton

**Files:**
- Create: `firmware/crates/binbook-diagnostic-protocol/Cargo.toml`
- Create: `firmware/crates/binbook-diagnostic-protocol/src/lib.rs`
- Create: `firmware/crates/binbook-diagnostic-protocol/tests/codec.rs`
- Modify: `firmware/Cargo.toml`

- [ ] **Step 1: Write failing protocol constants and header tests**

Create `firmware/crates/binbook-diagnostic-protocol/tests/codec.rs` with tests that assert:

- `PROTOCOL_VERSION == 1`
- `MAX_FRAME_BYTES == 512`
- `FRAME_DELIMITER == 0`
- encoding `HELLO` request sequence `7` produces a COBS-delimited frame ending in `0x00`
- decoding that frame returns kind `Request`, opcode `Hello`, sequence `7`, and empty payload

- [ ] **Step 2: Run the failing test**

Run:

```bash
cd firmware && cargo test -p binbook-diagnostic-protocol --test codec
```

Expected: fail because the crate does not exist.

- [ ] **Step 3: Implement the minimal crate**

Implement:

- `#![no_std]`
- constants from the design spec
- `FrameKind`, `Opcode`, `Status`, `FrameHeader`, `FrameRef`
- `encode_frame_into()`
- `decode_frame()`
- COBS encode/decode with caller-owned buffers
- CRC-16/CCITT-FALSE

Use fixed slices only. Return typed errors such as `OutputTooSmall`, `BadMagic`, `BadCrc`, `FrameTooLarge`, `BadCobs`, and `UnknownOpcode`.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cd firmware && cargo test -p binbook-diagnostic-protocol --test codec
```

Expected: pass.

- [ ] **Step 5: Run workspace host tests**

Run:

```bash
cd firmware && cargo test --workspace
```

Expected: pass.

## Task 2: Protocol Payloads And Error Cases

**Files:**
- Modify: `firmware/crates/binbook-diagnostic-protocol/src/lib.rs`
- Modify: `firmware/crates/binbook-diagnostic-protocol/tests/codec.rs`

- [ ] **Step 1: Add failing payload tests**

Add tests for:

- `KEY RIGHT press` encodes to a one-byte key plus one-byte action and decodes back.
- `PAGE goto 3` encodes an action plus `u32` page index and decodes back.
- `STATUS` response encodes current page, page count, panel mode, dropped log count, and last error code.
- `LOG_GET` request encodes cursor sequence and max byte budget.
- malformed CRC is rejected.
- an encoded frame larger than `MAX_FRAME_BYTES` is rejected before payload parsing.

- [ ] **Step 2: Run the failing tests**

Run:

```bash
cd firmware && cargo test -p binbook-diagnostic-protocol --test codec
```

Expected: fail because typed payload helpers are missing.

- [ ] **Step 3: Implement typed payload helpers**

Implement small helper functions and enums:

- `KeyCode`
- `KeyAction`
- `PageAction`
- `PanelModeCode`
- `ProbeCode`
- `encode_key_payload()`, `decode_key_payload()`
- `encode_page_payload()`, `decode_page_payload()`
- `encode_status_payload()`, `decode_status_payload()`
- `encode_log_get_payload()`, `decode_log_get_payload()`
- `encode_probe_payload()`, `decode_probe_payload()`

Keep payload structs plain and copyable. Avoid formatting and allocation.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cd firmware && cargo test -p binbook-diagnostic-protocol --test codec
```

Expected: pass.

## Task 3: Firmware Feature Gate And Source-Level Protection

**Files:**
- Modify: `firmware/crates/binbook-fw/Cargo.toml`
- Modify: `firmware/crates/binbook-fw/src/lib.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Add failing feature-gate tests**

In `firmware_logic.rs`, add source-level tests that assert:

- `Cargo.toml` contains `diagnostic-console`.
- `main.rs` contains `#[cfg(feature = "diagnostic-console")]` near diagnostic serial setup.
- `main.rs` does not call diagnostic polling outside a `diagnostic-console` cfg block.
- `debug-log` and `diagnostic-console` appear as separate feature names.

- [ ] **Step 2: Run the failing tests**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic diagnostic_console_feature_gate -- --nocapture
```

Expected: fail because the feature and cfg blocks are missing.

- [ ] **Step 3: Add the feature and cfg scaffolding**

Add:

- optional dependency on `binbook-diagnostic-protocol`
- `diagnostic-console = ["dep:binbook-diagnostic-protocol"]`
- `#[cfg(feature = "diagnostic-console")] pub mod diag;`
- `#[cfg(feature = "diagnostic-console")] pub mod diag_log;`

In `main.rs`, add only the cfg-scoped structure needed for tests. Do not implement serial behavior in this task.

- [ ] **Step 4: Run focused and default builds**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic diagnostic_console_feature_gate -- --nocapture
cd firmware && cargo test --workspace
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
```

Expected: all pass. The non-diagnostic firmware build must not require diagnostic code.

## Task 4: SRAM Diagnostic Log Ring

**Files:**
- Create: `firmware/crates/binbook-fw/src/diag_log.rs`
- Modify: `firmware/crates/binbook-fw/src/lib.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Add failing log ring tests**

Add tests that cover:

- pushing records assigns ascending sequence numbers;
- reading from cursor `0` returns records in order;
- pushing more than capacity overwrites oldest records and increments dropped count;
- clearing resets readable records and dropped count;
- formatting is not required in firmware records.
- repeated idle events are deduplicated instead of consuming one record per loop;
- an idle transition followed by a non-idle event records the suppressed idle
  repeat count in the transition or summary record.

- [ ] **Step 2: Run the failing tests**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic diag_log_ -- --nocapture
```

Expected: fail because `diag_log` does not exist.

- [ ] **Step 3: Implement the ring**

Implement:

- `DiagLogRecord`
- `DiagLevel`
- `DiagSubsystem`
- `DiagEvent`
- `DiagLog<const N: usize>`
- `DiagDeduper`
- `push()`
- `push_deduped()`
- `read_from(cursor, out_records)`
- `clear()`
- `dropped_records()`
- `next_sequence()`

Use arrays and indices only. No heap. Use named constants for default capacity.
Use named constants for idle summary cadence, such as `IDLE_SUMMARY_MS`.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic diag_log_ -- --nocapture
```

Expected: pass.

## Task 5: Compact Flash Crash Summary Model

**Files:**
- Modify: `firmware/crates/binbook-fw/src/diag_log.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Add failing crash summary tests**

Add tests that cover:

- encoding a crash summary writes magic, version, last error, last page, panel mode, copied log count, and CRC32;
- decoding rejects bad magic;
- decoding rejects bad CRC;
- an empty all-`0xFF` flash sector reports no summary instead of an error.

- [ ] **Step 2: Run the failing tests**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic diag_crash_ -- --nocapture
```

Expected: fail because crash summary helpers are missing.

- [ ] **Step 3: Implement crash summary encode/decode**

Implement a fixed binary record in `diag_log.rs`. Keep flash storage integration out of this task; this task only serializes and validates a bounded summary buffer.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic diag_crash_ -- --nocapture
```

Expected: pass.

## Task 6: Firmware Command Dispatch Without Hardware

**Files:**
- Create: `firmware/crates/binbook-fw/src/diag.rs`
- Modify: `firmware/crates/binbook-fw/src/input.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Add failing dispatch tests**

Add tests that use a fake reader state and assert:

- `KEY RIGHT press` resolves to the same next-page result as `Button::Right`.
- `KEY LEFT press` resolves to the same previous-page result as `Button::Left`.
- `PAGE next`, `PAGE previous`, and `PAGE goto` clamp at book edges.
- `STATUS` includes current page, page count, dropped log count, and last error.
- command receipt and page changes push log records.

- [ ] **Step 2: Run the failing tests**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic diag_dispatch_ -- --nocapture
```

Expected: fail because dispatch code is missing.

- [ ] **Step 3: Implement testable dispatch core**

Implement hardware-free dispatch functions that accept:

- decoded opcode and payload;
- current page and page count;
- mutable log ring;
- a callback or enum result for render requests.

Do not touch USB serial or display hardware in this task. The dispatch core must be host-testable.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic diag_dispatch_ -- --nocapture
```

Expected: pass.

## Task 7: Display Probe Routing

**Files:**
- Modify: `firmware/crates/binbook-fw/src/display.rs`
- Modify: `firmware/crates/binbook-fw/src/diag.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Add failing probe tests**

Add tests that assert:

- `DISPLAY_PROBE window_corners` maps to a `WindowCorners` render request.
- `DISPLAY_PROBE clear_white` maps to a `ClearWhite` render request.
- unknown probe codes return an error response and log a protocol error.

- [ ] **Step 2: Run the failing tests**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic diag_probe_ -- --nocapture
```

Expected: fail because probe routing is missing.

- [ ] **Step 3: Implement probe routing**

Add a small enum for diagnostic display requests. Wire it to existing display helpers where possible. Keep hardware calls behind firmware integration code; host tests should validate routing and request selection.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic diag_probe_ -- --nocapture
```

Expected: pass.

## Task 8: USB Serial/JTAG Firmware Integration

**Files:**
- Modify: `firmware/crates/binbook-fw/src/main.rs`
- Modify: `firmware/crates/binbook-fw/src/diag.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Add source-level integration tests**

Add tests that assert:

- `main.rs` imports `esp_hal::usb_serial_jtag::UsbSerialJtag` only under `diagnostic-console`.
- diagnostic serial polling is called from the main loop only under `diagnostic-console`.
- packet handling uses fixed input/output buffers whose sizes reference protocol constants.

- [ ] **Step 2: Run the failing tests**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic diagnostic_console_usb_serial -- --nocapture
```

Expected: fail because USB Serial/JTAG integration is not wired.

- [ ] **Step 3: Integrate USB Serial/JTAG**

In `main.rs`, under `diagnostic-console`:

- initialize `UsbSerialJtag::new(peripherals.USB_DEVICE)`;
- keep fixed RX and TX buffers;
- poll available bytes without blocking the 50 ms input loop;
- decode complete COBS frames;
- dispatch commands through `diag.rs`;
- write response frames back through USB Serial/JTAG.

Avoid moving display ownership into diagnostic code. Diagnostic dispatch should return actions that `main.rs` applies through existing render helpers.

- [ ] **Step 4: Build diagnostic firmware**

Run:

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
```

Expected: pass.

- [ ] **Step 5: Verify non-diagnostic firmware still builds**

Run:

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
```

Expected: pass.

## Task 8A: Convert Current Debug Prints To Structured Logs

**Files:**
- Modify: `firmware/crates/binbook-fw/src/main.rs`
- Modify: `firmware/crates/binbook-fw/src/diag_log.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] **Step 1: Add failing migration tests**

Add source-level and behavior tests that assert:

- diagnostic events exist for firmware started, ADC sample, button event, page turn, render start, refresh decision, panel mode, and display error;
- `main.rs` does not add new navigation/input/refresh diagnostics only as `dbgprintln!` when `diagnostic-console` is enabled;
- repeated idle polling produces at most one `IdleEntered` record plus bounded `IdleSummary` records over the configured interval;
- `debug-log` text output, when present, mirrors structured events instead of being the only record of the event.

- [ ] **Step 2: Run the failing tests**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic diag_structured_logging_ -- --nocapture
```

Expected: fail because current diagnostics are only `dbgprintln!` calls and idle dedupe is not wired into the main loop.

- [ ] **Step 3: Implement structured logging macros/helpers**

Add lightweight helpers that compile to no-ops without `diagnostic-console` and push records with it enabled. Keep `dbgprintln!` available only as a mirror for `debug-log` builds. The implementation should cover current diagnostic sites in `main.rs`:

- firmware start;
- periodic bounded ADC sample;
- button event;
- select/back no-op press;
- page turn decision;
- render start;
- refresh decision;
- panel mode;
- display error.

Do not add idle records in the hot loop directly. Use `DiagDeduper` to record `IdleEntered`, suppress repeated idle ticks, and emit `IdleSummary` only at the named cadence.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic diag_structured_logging_ -- --nocapture
```

Expected: pass.

- [ ] **Step 5: Build both firmware variants**

Run:

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console,debug-log --target riscv32imc-unknown-none-elf --release
```

Expected: both builds pass. Non-diagnostic firmware must not allocate diagnostic log storage.

## Task 9: CLI Protocol Helpers

**Files:**
- Modify: `cli/Cargo.toml`
- Modify: `cli/src/lib.rs`
- Modify: `cli/tests/protocol.rs`

- [ ] **Step 1: Add failing CLI protocol tests**

Add tests that assert:

- `diag_hello_request(1)` encodes a valid protocol `HELLO` request.
- `diag_key_request(2, RIGHT)` encodes a valid `KEY` request.
- `diag_page_goto_request(3, 3)` encodes a valid `PAGE` request.
- CLI can decode a `STATUS` response into printable fields.
- existing default `cargo test` does not require opening a serial port.

- [ ] **Step 2: Run the failing tests**

Run:

```bash
cd cli && cargo test --test protocol
```

Expected: fail because CLI helpers are missing.

- [ ] **Step 3: Implement CLI helpers**

Depend on `binbook-diagnostic-protocol` by path. Add helper functions in `cli/src/lib.rs` that build request frames and parse response payloads. Keep actual serial I/O out of these helpers.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cd cli && cargo test --test protocol
```

Expected: pass.

## Task 10: CLI Serial Commands

**Files:**
- Modify: `cli/src/main.rs`
- Modify: `cli/Cargo.toml`
- Modify: `cli/tests/protocol.rs`

- [ ] **Step 1: Add failing CLI command tests**

Add tests or parser-level checks for:

- `binbook-cli diag hello --port /dev/ttyACM0`
- `binbook-cli diag key --port /dev/ttyACM0 RIGHT`
- `binbook-cli diag page --port /dev/ttyACM0 next`
- `binbook-cli diag page --port /dev/ttyACM0 goto 3`
- `binbook-cli diag status --port /dev/ttyACM0`
- `binbook-cli diag logs --port /dev/ttyACM0 --since 0`
- `binbook-cli diag logs --port /dev/ttyACM0 --clear`
- `binbook-cli diag crash --port /dev/ttyACM0`
- `binbook-cli diag crash --port /dev/ttyACM0 --clear`
- `binbook-cli diag probe --port /dev/ttyACM0 window-corners`

- [ ] **Step 2: Run the failing tests**

Run:

```bash
cd cli && cargo test --features serial-device
```

Expected: fail because subcommands are missing.

- [ ] **Step 3: Implement CLI serial transport**

Behind `serial-device`:

- open the requested serial port;
- send one COBS-delimited request;
- wait for a response with the same sequence;
- apply a bounded timeout;
- print clear errors for timeout, bad frame, bad status, and port-open failure.

When `serial-device` is disabled, `diag` commands should produce a compile-time absence or a clear runtime message depending on the simplest clap integration that keeps default tests passing.

- [ ] **Step 4: Run CLI checks**

Run:

```bash
cd cli && cargo test
cd cli && cargo test --features serial-device
```

Expected: pass.

## Task 11: Documentation

**Files:**
- Modify: `docs/reference/xteink-x4-firmware-flashing.md`
- Modify: `docs/specs/2026-06-26-x4-diagnostic-serial-console-design.md` if implementation decisions changed
- Modify: `HANDOFF.md`

- [ ] **Step 1: Update flashing reference**

Document:

- diagnostic firmware build command;
- diagnostic flash command using `FW_FEATURES="firmware-bin,diagnostic-console"`;
- CLI command examples;
- warning that only one process can own `/dev/ttyACM0`;
- note that `debug-log` is separate from `diagnostic-console`.

- [ ] **Step 2: Update handoff before hardware verification**

Add a section listing:

- host tests run;
- firmware builds run;
- hardware checks still pending;
- exact commands to run on hardware.

- [ ] **Step 3: Check docs for stale protocol references**

Run:

```bash
rg -n "LIST|UPLOAD|DELETE|diagnostic-console|debug-log|serial protocol|ttyACM0" docs firmware cli HANDOFF.md
```

Expected: any stale text protocol references are either removed or explicitly marked as historical/old CLI skeleton behavior.

## Task 12: Full Host Verification

**Files:**
- No code changes expected unless verification finds a bug.
- Modify: `HANDOFF.md` with final host verification evidence.

- [ ] **Step 1: Run firmware workspace tests**

Run:

```bash
cd firmware && cargo test --workspace
```

Expected: pass.

- [ ] **Step 2: Run CLI tests**

Run:

```bash
cd cli && cargo test
cd cli && cargo test --features serial-device
```

Expected: pass. If `serial-device` fails because host packages such as `libudev.pc` are missing, document the exact failure and use the repo's Homebrew guidance before installing anything.

- [ ] **Step 3: Run Python tests**

Run:

```bash
uv run pytest -q
```

Expected: pass.

- [ ] **Step 4: Build release firmware without diagnostics**

Run:

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
```

Expected: pass.

- [ ] **Step 5: Build release firmware with diagnostics**

Run:

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
```

Expected: pass.

## Task 13: Hardware Verification Completion Gate

**Files:**
- Modify: `HANDOFF.md`

Hardware and serial commands require host access. Run them escalated if the sandbox requests it. Do not run any command in parallel that may touch `/dev/ttyACM0`.

- [ ] **Step 1: Flash diagnostic firmware**

Run from repo root:

```bash
FW_FEATURES="firmware-bin,diagnostic-console" firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Expected: flash completes successfully and the device boots to the navigation probe.

- [ ] **Step 2: Verify hello**

Run:

```bash
cd cli && cargo run --features serial-device -- diag hello --port /dev/ttyACM0
```

Expected: output includes protocol version `1`, target `xteink-x4`, max frame bytes `512`, and diagnostic capabilities.

- [ ] **Step 3: Verify serial key navigation**

Run:

```bash
cd cli && cargo run --features serial-device -- diag key --port /dev/ttyACM0 RIGHT
cd cli && cargo run --features serial-device -- diag status --port /dev/ttyACM0
```

Expected: device visibly advances one page and status reports the new page.

Run:

```bash
cd cli && cargo run --features serial-device -- diag key --port /dev/ttyACM0 LEFT
cd cli && cargo run --features serial-device -- diag status --port /dev/ttyACM0
```

Expected: device visibly returns one page and status reports the new page.

- [ ] **Step 4: Verify direct page command**

Run:

```bash
cd cli && cargo run --features serial-device -- diag page --port /dev/ttyACM0 goto 0
cd cli && cargo run --features serial-device -- diag status --port /dev/ttyACM0
```

Expected: device visibly renders page 0 and status reports page 0.

- [ ] **Step 5: Verify logs**

Run:

```bash
cd cli && cargo run --features serial-device -- diag logs --port /dev/ttyACM0 --since 0
```

Expected: output includes command receipt, key/page action, and render-related log records with increasing sequence numbers.

- [ ] **Step 6: Verify crash summary**

Run:

```bash
cd cli && cargo run --features serial-device -- diag crash --port /dev/ttyACM0
```

Expected: output reports either no crash summary or a valid summary with CRC accepted.

- [ ] **Step 7: Verify display probe**

Run:

```bash
cd cli && cargo run --features serial-device -- diag probe --port /dev/ttyACM0 window-corners
```

Expected: panel visibly shows the expected corner-window pattern.

- [ ] **Step 8: Record evidence**

Update `HANDOFF.md` with:

- exact flash command and result;
- exact CLI commands and outputs;
- visual confirmation for page turns and display probe;
- any failures, retries, or remaining caveats.

Do not mark implementation complete until this evidence is recorded.
