# Xteink X4 Agent-Driven Device Verification

## Navigation Burst Diagnostic

Build and flash with `FW_FEATURES="firmware-bin,diagnostic-console" firmware/scripts/flash-xteink-x4-nav-probe.sh`. Capture boot serial for at least 15 seconds, then establish independent HELLO and STATUS baselines. STATUS must report `page_count=16`, zero protocol errors, and `last_error=0`.

```bash
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline python firmware/scripts/run-x4-nav-burst-diagnostic.py --port /dev/ttyACM0 --video-device /dev/video1 --rounds 10 --inter-key-ms 0 --output-dir /tmp/x4-nav-burst
```

After it exits, query STATUS and paginate LOG independently. Compare every contact-sheet `PAGE NN` label with JSONL `expected_to`; CLI success alone is insufficient. Record the MP4, JSONL, transcript, contact sheets, exact commands, relevant output, first divergence or clean result, and acceptance matrix in `HANDOFF.md`.

## Purpose

This runbook is the authoritative procedure for an AI agent to build, flash,
drive, and verify the BinBook diagnostic firmware on an Xteink X4. The serial
console is a hardware-verification interface, not merely a debugging shell.
A command is verified only when its request, response, resulting state, and any
required visible panel outcome are all checked.

Use `docs/specs/2026-06-26-x4-diagnostic-serial-console-design.md` for protocol
semantics and `BINBOOK_FORMAT_SPEC.md` for file-format semantics.

## Safety And Ownership Rules

- Run all flash, USB, serial, and device commands with host escalation. Never use
  sandbox `/dev` visibility as evidence.
- Only one process may own `/dev/ttyACM0` at a time. Do not run monitor, CLI,
  hardware tests, or flashing concurrently.
- Run hardware commands sequentially and wait for each process to exit.
- Do not resend a timed-out mutating command blindly. Query STATUS or the
  affected state first to determine whether it executed.
- After flashing, USB may re-enumerate. A safe read-only HELLO may be retried
  once after the port returns; record the initial timeout if one occurred.
- When `diagnostic-console` and `debug-log` are both enabled, packet transport
  owns USB Serial/JTAG and text logging compiles out.

The default port is the verified `/dev/ttyACM0`. Override it only when an
escalated host check identifies a different device:

```bash
export PORT="${PORT:-/dev/ttyACM0}"
```

## Host Prerequisites

Use the pinned nightly toolchain for firmware. On this Homebrew-based host,
provide the current systemd prefix when building the serial CLI:

```bash
export SYSTEMD_PREFIX="$(brew --prefix systemd)"
export PKG_CONFIG_PATH="$SYSTEMD_PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
export LIBRARY_PATH="$SYSTEMD_PREFIX/lib:${LIBRARY_PATH:-}"
export LD_LIBRARY_PATH="$SYSTEMD_PREFIX/lib:${LD_LIBRARY_PATH:-}"
```

Compile the device-gated tests before touching hardware:

```bash
cd cli
cargo test --features serial-device --test hardware_diagnostic -- --list
cd ..
```

The output must list:

- `hardware_byte_by_byte_status_request`
- `hardware_two_frame_batched_request`
- `hardware_malformed_frame_does_not_wedge_stream`

## Verification Procedure

### 1. Run Host Behavior Tests

```bash
cd firmware
cargo test --workspace --features diagnostic-console
cargo test --workspace
cd ../cli
cargo test
cargo test --features serial-device
cd ..
uv run pytest -q
```

Feature-enabled firmware tests are mandatory; default workspace tests do not
cover code excluded by `cfg(feature = "diagnostic-console")`.

### 2. Flash The Diagnostic Image

```bash
FW_FEATURES="firmware-bin,diagnostic-console" \
  firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Record the chip, flash size, application size, and the final flash result. Do
not start another serial process until flashing exits and the USB port returns.

### 3. Capture The Boot Record

Run the 15-second pyserial command in `AGENTS.md`. It must own the port alone.
Record the relevant bootloader, partition, segment-load, and application-load
lines in `HANDOFF.md`. Packet firmware intentionally emits no debug text after
boot when it owns USB.

### 4. Verify HELLO And Baseline STATUS

```bash
cd cli
cargo run --features serial-device -- diag hello --port "$PORT"
cargo run --features serial-device -- diag status --port "$PORT"
```

HELLO must report protocol 1, maximum frame size 512, firmware `binbook-fw`,
target `xteink-x4`, and KEY, PAGE, STATUS, LOG, CRASH, and DISPLAY_PROBE
capabilities. STATUS must decode current page, page count, panel mode, dropped
logs, protocol errors, and signed last error.

For every response, verify response kind, opcode, sequence, status, and typed
payload. The CLI performs these checks and exits nonzero on a mismatch.

### 5. Verify KEY Through Shared Navigation

Use a discriminating starting state and query state independently:

```bash
cargo run --features serial-device -- diag page --port "$PORT" goto 0
cargo run --features serial-device -- diag key --port "$PORT" RIGHT
cargo run --features serial-device -- diag status --port "$PORT"
cargo run --features serial-device -- diag key --port "$PORT" LEFT
cargo run --features serial-device -- diag status --port "$PORT"
```

Required state transition: `0 -> 1 -> 0`. If visible navigation is an acceptance
criterion, obtain a user or camera observation in addition to STATUS.

### 6. Verify Every PAGE Action

```bash
cargo run --features serial-device -- diag page --port "$PORT" goto 3
cargo run --features serial-device -- diag status --port "$PORT"
cargo run --features serial-device -- diag page --port "$PORT" goto 0
cargo run --features serial-device -- diag status --port "$PORT"
cargo run --features serial-device -- diag page --port "$PORT" next
cargo run --features serial-device -- diag page --port "$PORT" previous
cargo run --features serial-device -- diag page --port "$PORT" last
cargo run --features serial-device -- diag status --port "$PORT"
cargo run --features serial-device -- diag page --port "$PORT" first
cargo run --features serial-device -- diag status --port "$PORT"
cargo run --features serial-device -- diag page --port "$PORT" current
```

Required results are `goto 3 -> 3`, `goto 0 -> 0`, `next -> 1`, `previous -> 0`,
`last -> 3`, `first -> 0`, and `current -> 0`. `current` must not cause another
render.

### 7. Verify Structured Logs And Clear Semantics

First generate a known page render, then retrieve from a cursor that reaches
its records:

```bash
cargo run --features serial-device -- diag logs --port "$PORT" --since 0
cargo run --features serial-device -- diag logs --port "$PORT" --clear
cargo run --features serial-device -- diag logs --port "$PORT" --since 0
cargo run --features serial-device -- diag page --port "$PORT" next
cargo run --features serial-device -- diag logs --port "$PORT" --since <returned-cursor>
```

`<returned-cursor>` is a runtime value printed by the preceding LOG response,
not a fixed protocol constant. Continue from each returned cursor when the
bounded response does not yet include the generated render.

Verify increasing sequences and the current aggregator events `CMD_RECEIPT`,
`REFRESH_PHASE`, `PANEL_MODE`, `CONTROLLER_RAM_STATE`, `PAGE_TURN`, and
`TURN_DEQUEUED`. Probe operations additionally emit `RENDER_START` and
`RENDER_SUCCESS`. After clear, no pre-clear record may reappear; the retrieval
command's own receipt is a valid new record.

### 8. Verify Crash Flash Behavior

```bash
cargo run --features serial-device -- diag crash --port "$PORT" --clear
cargo run --features serial-device -- diag crash --port "$PORT"
```

The second command must print `crash=empty`. Present-summary persistence and
bad-CRC rejection are verified with host fake-flash tests; do not induce a
fatal device fault solely to manufacture hardware evidence.

### 9. Verify Visible Display Probes

Run one command at a time and pause for user or camera confirmation after each:

```bash
cargo run --features serial-device -- diag probe --port "$PORT" window-corners
cargo run --features serial-device -- diag probe --port "$PORT" clear-white
cargo run --features serial-device -- diag probe --port "$PORT" full-refresh-current
```

Required visible outcomes:

1. Four black 128x96 rectangles, one at each physical corner.
2. A uniformly white panel.
3. The current page restored by a grayscale full refresh. For page 0 of the
   navigation fixture, this is a gray-band pattern with asymmetric edge markers.

An `ok` response alone is transport evidence and does not satisfy these checks.
Record who or what observed each visible result.

### 10. Verify Fragmented, Batched, And Malformed Streams

Run the three transport tests explicitly. Do not use an unfiltered
`--ignored`: that also selects the staged-gray exercise, whose zero-error
precondition intentionally conflicts with the malformed-frame test.

```bash
cargo test --features serial-device --test hardware_diagnostic \
  hardware_byte_by_byte_status_request -- \
  --ignored --exact --nocapture --test-threads=1
cargo test --features serial-device --test hardware_diagnostic \
  hardware_two_frame_batched_request -- \
  --ignored --exact --nocapture --test-threads=1
cargo test --features serial-device --test hardware_diagnostic \
  hardware_malformed_frame_does_not_wedge_stream -- \
  --ignored --exact --nocapture --test-threads=1
cargo run --features serial-device -- diag status --port "$PORT"
```

All three live tests must pass sequentially. The follow-up STATUS must show an
increased `protocol_error_count` after malformed input and prove valid requests
still complete.

### 11. Verify Combined Feature USB Ownership

```bash
cd ..
FW_FEATURES="firmware-bin,diagnostic-console,debug-log" \
  firmware/scripts/flash-xteink-x4-nav-probe.sh
cd cli
cargo run --features serial-device -- diag hello --port "$PORT"
```

HELLO must still decode. Reflash the normal diagnostic image afterward and
independently reconfirm HELLO:

```bash
cd ..
FW_FEATURES="firmware-bin,diagnostic-console" \
  firmware/scripts/flash-xteink-x4-nav-probe.sh
cd cli
cargo run --features serial-device -- diag hello --port "$PORT"
```

Leave the device running the normal diagnostic image.

### 12. Verify Staged Grayscale Exercise

This section validates the permanent async staged-grayscale behavior. Keep one process on
`/dev/ttyACM0` at a time and do not start the serial exercise until the boot
capture has finished.

Flash the permanent diagnostic image:

```bash
FW_FEATURES="firmware-bin,diagnostic-console" \
  firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Capture the 15-second boot record with the pyserial command from `AGENTS.md`.
Record the bootloader, partition, segment-load, and application-load lines.

Re-establish the diagnostic baseline:

```bash
cd cli
cargo run --features serial-device -- diag hello --port "$PORT"
cargo run --features serial-device -- diag status --port "$PORT"
cd ..
```

Run the autonomous exercise and tell the user the webcam observation is
beginning:

```bash
cd cli
cargo run --features serial-device -- diag exercise staged-gray --port "$PORT"
cd ..
```

While the exercise runs, verify all of these visible criteria:

1. The fast base covers the entire active panel; every non-white swatch is
   initially black and white remains white.
2. Differential refinement starts only after the 350 ms idle delay and separates
   the dark- and light-gray swatches.
3. Black and white remain visually stable; no full-panel clear, inversion,
   white flash, or black flash occurs.
4. A turn queued before activation cancels overlay streaming at a 16-row
   boundary. A turn queued after activation waits for BUSY completion.
5. A turn during background base sync cancels sync without waiting for all rows.
6. Queued intermediate pages complete in FIFO order as `2`, `3`, `2`.
7. The final `RIGHT` transition lands on page `3` and remains artifact-free.

After the exercise, query `STATUS` and `LOG` independently:

```bash
cd cli
cargo run --features serial-device -- diag status --port "$PORT"
cargo run --features serial-device -- diag logs --port "$PORT" --since 0
cd ..
```

Confirm `page=3`, `last_error=0`, `dropped_turns=0`, waveform hint `2`, LUT
revision `1`, and ordered `GRAY_OVERLAY_*`, `BW_BASE_SYNC_*`, and
`CONTROLLER_RAM_STATE` events.

Reject the candidate image if the webcam shows a full refresh, BW reversion,
corruption, clipping, or incomplete full-panel writes. Reconfirm `HELLO` and `STATUS`
before leaving the device running the permanent diagnostic image.

## Failure Handling

- Port missing or inaccessible: perform an escalated host check; do not infer
  absence from sandbox output.
- First read-only request times out immediately after flash: wait for USB
  re-enumeration, retry HELLO once, and record both outcomes.
- Mutating request times out: query STATUS, logs, crash state, or panel state
  before deciding whether a retry is safe.
- Non-OK response: record opcode, sequence, status, payload, `last_error`, and
  relevant structured logs.
- Stream test wedges: stop all port owners, verify no concurrent monitor or CLI
  process exists, then reflash before further diagnosis.
- Visible result differs: record the exact observed pattern and stop completion;
  do not convert an acknowledgement into visual evidence.

## Evidence And Completion

Update `HANDOFF.md` as a current-state snapshot. For each requirement record:

- exact command and relevant output;
- discriminating starting state;
- request and decoded response semantics;
- ending state and independent follow-up query;
- user/camera observation for visible behavior;
- failures, retries, and unresolved gaps.

Maintain an acceptance matrix mapping every requirement to its implementation
path, automated test, observed evidence, and hardware evidence. Do not mark a
firmware plan complete while any cell is blank or any visible result remains
unverified.
