# Serial Upload Throughput and Progress Plan

**Status:** Implemented and verified on live Xteink X4 hardware on 2026-07-15.

**Goal:** Make BinBook uploads over the Xteink X4 USB diagnostic connection
materially faster and make the host command report honest preparation,
transfer, verification, and library-update progress.

**Primary acceptance target:** Using the tracked 1,022,165-byte navigation
fixture, the median of three uploads on the same device, SD card, cable, port,
firmware feature set, and starting library state is at least 4 times faster than
the measured pre-change baseline. Every run must commit with the expected CRC,
read back byte-identically, leave zero new protocol errors, and leave no partial
file after an injected host-side failure.

**Authoritative references:**

- `AGENTS.md` and `AGENTS.local.md`
- `docs/reference/rust-development-standards.md`
- `docs/reference/xteink-x4-agent-device-verification.md`
- `docs/reference/xteink-x4-firmware-flashing.md`
- `docs/specs/2026-07-01-diagnostic-storage-extension-design.md`
- `firmware/crates/binbook-diagnostic-protocol` for the actual wire format

The dated storage design says that the host pipelines writes and adapts to
HELLO's maximum frame size, but the current host implementation does neither.
Treat the current source and protocol crate as the starting behavior; update
the design document when the implementation is changed.

## Current-path findings and hypotheses

The upload path is:

```text
host file
  -> binbook CLI: whole-file read + CRC
  -> one 4,088-byte StoreUploadWrite frame
  -> wait for its acknowledgement
  -> firmware USB Serial/JTAG polling loop
  -> diagnostic transfer buffer
  -> mounted storage-owner request
  -> persistent FAT write session
  -> 4 MHz SPI SD block device
  -> acknowledgement
```

Source inspection identifies the following causes, in priority order.

1. **The firmware polling loop imposes a low ceiling before storage is
   involved.** `firmware/crates/binbook-fw/src/runtime/diagnostic_console.rs`
   drains at most 64 USB bytes, queries the diagnostic aggregator, and then
   sleeps for 10 ms. A maximum 4,126-byte encoded frame therefore needs about
   65 loop iterations and roughly 650 ms merely to enter firmware. The implied
   ceiling is about 6.4 KB/s, excluding SD writes and acknowledgements.
2. **The host is stop-and-wait.**
   `crates/binbook/src/diag_cli.rs::upload_file` sends one chunk and blocks until
   its response before sending the next. The original design's bounded
   pipelining was not implemented.
3. **Chunk size fights the SD sector size.** Each request spends eight payload
   bytes on upload ID and offset, leaving 4,088 data bytes. Successive offsets
   are therefore not 512-byte aligned.
4. **The block stream always read-modify-writes.**
   `crates/embedded-sd-storage/src/block_stream.rs` reads a 512-byte block before
   every write, even when the caller replaces the complete aligned block. It
   also submits blocks one at a time. This adds avoidable SPI traffic. The first
   safe optimization is to skip the read for complete aligned blocks; do not add
   a hidden multi-block scratch array merely to batch calls.
5. **The SD clock is 4 MHz.** This is the verified X4/SquidScript target setting,
   so it is a real upper bound but not the first knob to turn. Keep it unchanged
   in the primary implementation. A clock sweep is a separate board-
   qualification experiment only if post-change evidence remains bus-bound.
6. **The current host UX is both silent and over-buffered.** It reads the entire
   file into a `Vec`, prints no progress until completion, ignores the returned
   `bytes_accepted`, uses a fixed commit sequence of 100, and exits the process
   from deep helper branches. The top-level `binbook upload` command is only a
   placeholder while the working command is nested under `diag storage`.
7. **“Uploaded” does not mean “visible in the library yet.”** Commit queues a
   background library refresh. Existing live evidence puts that refresh around
   5.4–5.6 seconds. The CLI currently prints success before explaining this
   state.

The `115200` host setting is not the leading hypothesis. The firmware uses the
ESP32-C3 USB Serial/JTAG peripheral, and the local `esp-hal` implementation moves
USB packets in 64-byte endpoint chunks without configuring a UART baud clock.
Do not change the configured line rate as a performance fix unless a controlled
live comparison proves that it affects throughput.

## Locked design decisions

- Preserve protocol v2 framing and the 4,096-byte maximum payload in the first
  optimization pass.
- Derive an upload data size that is the largest multiple of 512 fitting after
  the eight-byte write header: 3,584 bytes. This trades 504 bytes of nominal
  frame capacity for stable sector-aligned file offsets.
- Convert USB Serial/JTAG handling to interrupt-driven async I/O. Do not replace
  the 10 ms sleep with an aggressive busy-poll.
- Query the diagnostic aggregator only when a complete frame needs dispatch or
  a completion/status synchronization actually needs service.
- Keep bounded, caller-owned firmware buffers. Do not enlarge the protocol
  frame, RX ring, task stack, or diagnostic transfer buffer in the primary
  implementation.
- Optimize full-sector overwrite in `embedded-sd-storage`; do not put FAT or
  block-write logic into `binbook-fw`.
- Implement a bounded two-frame upload window only after the event-driven RX
  path and max-frame batch tests pass. Advertise it with a new capability bit;
  the host falls back to stop-and-wait when the bit is absent.
- Advance progress only from validated acknowledgements. Bytes written to the
  host OS or merely sent over USB do not count as device-accepted bytes.
- Use the top-level `binbook upload` command for complete uploads. Keep
  diagnostic `storage begin`/`abort` for explicit session and cleanup probes.
- Default the destination name to the local UTF-8 filename. Retain explicit
  `--name`/`--path` overrides.
- Default progress mode to `auto`: a single updating line on a TTY; stable
  newline-delimited phase messages when stderr is not a TTY; no ANSI escapes in
  captured output. Support `--progress always|auto|never`.
- Stream the host file twice with bounded buffers: first for size and CRC32,
  then for upload. Do not retain the complete book in RAM.
- On a transfer error, drain responses for requests already in flight, attempt
  `StoreAbort` once when the upload ID is known, report whether cleanup was
  confirmed, and return a typed error. Do not silently retry a mutating write.
- Report library refresh as a distinct phase. The default command waits with a
  bounded, non-starving one-second `StoreList` poll until the uploaded name and
  size appear, or reports `uploaded; library refresh still pending` on timeout.
  `--no-wait-library` skips this phase. For a same-name/same-size replacement,
  list visibility alone is not identity proof; CRC commit and optional readback
  remain the integrity evidence.

## Observable acceptance criteria

1. A TTY user sees filename, total size, acknowledged bytes, percentage,
   rolling transfer rate, elapsed time, and ETA during transfer, followed by
   explicit `Verifying` and `Updating library` phases.
2. Non-TTY output contains no carriage returns or ANSI escapes and remains
   useful in logs. `--progress never` suppresses intermediate output.
3. Progress never exceeds the sum of validated `bytes_accepted` responses, and
   a short, duplicate, wrong-sequence, wrong-opcode, non-OK, or missing response
   fails rather than falsely reaching 100%.
4. Files larger than the 4 KiB firmware transfer buffer upload through multiple
   chunks without host whole-file buffering or firmware buffer growth.
5. The firmware no longer has a 10 ms-per-64-byte RX cadence; byte-fragmented
   and two-max-frame batched requests remain correctly decoded and correlated.
6. Complete aligned block overwrites do not perform a read first; partial block
   writes still preserve untouched bytes through read-modify-write.
7. New firmware advertises bounded upload pipelining; new host software uses a
   two-frame window only when advertised and remains compatible with protocol-v2
   firmware lacking the capability.
8. A failed upload is aborted and independently absent. A successful upload
   has matching CRC, name, size, complete byte-for-byte readback, and becomes
   visible through the refreshed library index.
9. Median 1 MiB throughput improves by at least 4 times over the controlled
   baseline, with zero new protocol errors and no regression in HELLO, STATUS,
   fragmented-frame, batched-frame, malformed-frame recovery, list, read,
   delete, or abort behavior.
10. Static RAM and firmware image deltas are measured. No unexplained large
    async future, task-stack, duplicate frame buffer, or hidden 4/8 KiB scratch
    allocation is accepted.

## Task 1: Capture a controlled pre-change baseline after device release

**Files:** `HANDOFF.md` only. Do not change tracked source before this baseline
unless a clean worktree cannot be obtained without touching another agent's
work.

- [x] Confirm with the user/other agent that the device is free. Then check the
  worktree and record the exact commit and firmware features. Do not switch
  branches or disturb unrelated changes.
- [x] Build the current serial CLI and the current SD diagnostic image using the
  pinned toolchains. Flash only after exclusive ownership is confirmed:

  ```bash
  export PORT="${PORT:-/dev/ttyACM0}"
  cargo build -p binbook --features serial-device
  FW_FEATURES="firmware-bin,diagnostic-console,sd-storage" \
    firmware/scripts/flash-xteink-x4-nav-probe.sh
  ```

- [x] Capture boot output for at least 15 seconds with this command, then
  establish independent HELLO and STATUS baselines. Wait for this process to
  exit before running either CLI command:

  ```bash
  uv run --with pyserial --no-project python3 -c "
  import serial, time, sys
  ser = serial.Serial('$PORT', 115200, timeout=1)
  ser.dtr = False; ser.rts = False; time.sleep(0.05)
  ser.rts = True; time.sleep(0.05); ser.rts = False; time.sleep(0.1)
  start = time.time()
  while time.time() - start < 15:
      data = ser.read(ser.in_waiting or 1)
      if data:
          sys.stdout.buffer.write(data)
          sys.stdout.flush()
  ser.close()
  "
  target/debug/binbook diag hello --port "$PORT"
  target/debug/binbook diag status --port "$PORT"
  ```
- [x] Use the tracked fixture and a disposable destination:

  ```bash
  FIXTURE=firmware/crates/binbook-fw/fixtures/nav_probe.binbook
  NAME=serial-upload-baseline.binbook
  ```

- [x] Ensure the disposable name is absent, upload it three times with delete
  between runs, and measure complete command wall time with `/usr/bin/time -p`.
  Record local size and CRC, begin time, per-run elapsed time, median throughput,
  commit output, and STATUS protocol-error count. Do not infer throughput from
  the 4,096-byte historical smoke upload. Use this command shape for each run:

  ```bash
  /usr/bin/time -p target/debug/binbook upload \
    --port "$PORT" --name "$NAME" --file "$FIXTURE"
  target/debug/binbook diag status --port "$PORT"
  ```
- [x] After one run, poll list until the new name appears, read the entire file
  back, and require `cmp` plus matching SHA-256. Delete it and independently
  confirm absence:

  ```bash
  OUT="${OUT:-target/diag-evidence/serial-upload-baseline}"
  mkdir -p "$OUT"
  target/debug/binbook diag storage list --port "$PORT"
  target/debug/binbook diag storage read --port "$PORT" --path "$NAME" \
    --output "$OUT/readback.binbook"
  cmp "$FIXTURE" "$OUT/readback.binbook"
  sha256sum "$FIXTURE" "$OUT/readback.binbook"
  target/debug/binbook diag storage delete --port "$PORT" --path "$NAME"
  target/debug/binbook diag storage list --port "$PORT"
  ```
- [x] Record the starting and ending library state and restore it exactly. Put
  host-local artifact locations only in `HANDOFF.md`, never in tracked docs.

**Gate:** A reproducible median baseline for the 1,022,165-byte file exists,
including independent readback and zero-error STATUS. If exclusive device access
is unavailable, leave this task pending; do not substitute a sandbox or model
number.

## Task 2: Refactor the host upload core and add honest progress

**Owning crate:** `crates/binbook`

**Files:**

- Modify `crates/binbook/src/args.rs`
- Modify `crates/binbook/src/main.rs`
- Refactor `crates/binbook/src/diag_cli.rs`
- Add focused modules such as `crates/binbook/src/upload.rs` and
  `crates/binbook/src/progress.rs` if needed to keep files focused
- Modify `crates/binbook/src/serial_transport.rs`
- Add `crates/binbook/tests/upload.rs`
- Extend `crates/binbook/tests/serial_transport.rs`, `help.rs`, and `protocol.rs`

- [x] Write failing tests first with a fake serial peer, fake clock, and captured
  output. Use a source file larger than two upload chunks. Require:
  - two-pass streaming CRC/size and bounded reads;
  - derived 3,584-byte chunks at offsets `0`, `3584`, and `7168`;
  - monotonic nonzero sequence allocation through write and commit, without the
    fixed sequence 100;
  - exact response opcode, sequence, status, and `bytes_accepted == sent_len`;
  - progress advances on acknowledgement, not request write;
  - stable TTY, non-TTY, and `never` renderings;
  - a deterministic rate/ETA calculation with zero-duration protection;
  - abort attempted after an injected middle-chunk failure;
  - no 100% or success line after a short acknowledgement or commit CRC
    mismatch;
  - both top-level and diagnostic command surfaces invoke the same upload core.
- [x] Run the focused tests and confirm they fail for the intended missing
  behavior:

  ```bash
  cargo test -p binbook --features serial-device --test upload
  cargo test -p binbook --features serial-device --test serial_transport
  ```

- [x] Move upload mechanics out of the process-exiting CLI branch into a typed
  `Result<UploadSummary, UploadError>` API. Keep formatting and exit policy at
  the command boundary.
- [x] Compute CRC32 and size with a bounded first-pass buffer. Seek/reopen for
  the transfer pass and read exactly one aligned upload chunk at a time.
- [x] Add an injectable progress sink. Throttle visual refreshes to at most ten
  per second while always rendering phase changes and final state. Write
  progress to stderr so stdout can retain the final result or JSON.
- [x] Validate every response and accepted length before updating progress.
  Preserve typed transport, protocol, local-I/O, device-status, CRC, cleanup,
  and library-wait failures.
- [x] Make `binbook upload` functional. Preserve existing flags where practical
  and allow destination-name inference. Keep diagnostic begin/abort commands as
  the lower-level session and cleanup probes.
- [x] Add `--progress always|auto|never`, `--no-wait-library`, and a deterministic
  machine-readable final summary (`--json`) containing bytes, CRC32, elapsed
  milliseconds, acknowledged throughput, and library visibility. JSON mode
  implies non-interactive progress unless explicitly forced.
- [x] Poll library visibility no more than once per second with a bounded
  timeout so diagnostic reads do not monopolize the storage owner during the
  background scan.

**Gate:** All fake-peer tests pass; a short or malformed acknowledgement cannot
produce 100%; the large test file is never read into one host `Vec`; the
top-level CLI uses the tested typed implementation.

## Task 3: Remove the firmware's polling-latency ceiling

**Owning crate:** `firmware/crates/binbook-fw`

**Files:**

- Modify `firmware/crates/binbook-fw/Cargo.toml` only if the async embedded-I/O
  trait must be an explicit target dependency
- Modify `firmware/crates/binbook-fw/src/runtime/diagnostic_console.rs`
- Modify `firmware/crates/binbook-fw/src/diag/serial.rs` and
  `firmware/crates/binbook-fw/src/diag/runtime.rs` only for host-testable queue
  or readiness seams
- Extend `firmware/crates/binbook-fw/tests/diagnostic_serial.rs` and
  `diagnostic_runtime.rs`
- Extend `crates/binbook/tests/hardware_diagnostic.rs` with a two-maximum-frame
  batch test, kept ignored and exact like the existing device tests

- [x] Add failing host tests that feed a maximum encoded frame in 64-byte
  fragments and two maximum frames back-to-back. Require exact frame order,
  no dropped bytes, no protocol error, and no dispatch before a delimiter.
  Fill the RX ring near its boundary so a toy one-frame test cannot pass while
  the actual upload path overflows.
- [x] Add a failing completion-wakeup test or extracted scheduler test proving a
  pending display response can wake and transmit even when no new USB request
  arrives. This prevents an async RX wait from wedging existing navigation
  commands.
- [x] Convert `UsbSerialJtag` to async mode and wait on USB RX interrupts instead
  of the unconditional 10 ms timer. Use a bounded `select` between USB input,
  diagnostic completion, and any genuinely periodic synchronization. Do not
  busy-poll.
- [x] Drain all currently available endpoint bytes into `SerialState`, dispatch
  buffered complete frames in order, and use async TX/flush behavior. Query the
  aggregator only for dispatch/completion work, not once per 64-byte fragment.
- [x] Preserve one-owner USB semantics: packet diagnostics still suppress text
  debug logging when `diagnostic-console` is enabled.
- [x] Keep the current RX/TX/frame/transfer buffer sizes. Inspect the release ELF
  section sizes and compiler size output before and after; investigate async
  future growth rather than increasing a task stack or static allocation.
- [x] Run focused and feature-gated tests:

  ```bash
  cargo test -p binbook-fw --features diagnostic-console
  cargo test -p binbook-fw --features diagnostic-console,sd-storage
  cargo test -p binbook --features serial-device --test serial_transport
  ```

**Gate:** Fragmented and two-max-frame streams pass on the host, existing
completion behavior remains live without incoming USB data, both firmware
feature suites pass, and there is no unexplained buffer or task-state growth.

## Task 4: Align upload writes and remove unnecessary sector reads

**Owning crate:** `crates/embedded-sd-storage`

**Files:**

- Modify `crates/embedded-sd-storage/src/block_stream.rs`
- Extend `crates/embedded-sd-storage/tests/fat_image.rs` or add a focused block
  stream test module using a counting `BlockDevice`
- Keep the aligned host chunk derivation in `crates/binbook/src/upload.rs`

- [x] Write failing counting-device tests for:
  - a complete aligned 512-byte overwrite: zero block reads and one block write;
  - several complete aligned blocks: no pre-reads and exact stored bytes;
  - unaligned prefix/suffix writes: required reads occur and untouched bytes are
    preserved;
  - a 3,584-byte upload followed by another 3,584-byte upload: exact contiguous
    bytes and no data-sector pre-reads;
  - a payload larger than the obvious scratch block.
- [x] Confirm the full-sector test fails because `write_bytes` currently reads
  every destination block.
- [x] In `BlockDeviceStream::write_bytes`, retain read-modify-write for partial
  blocks. For a full aligned block, copy the caller's 512 bytes into the existing
  single `Block` scratch and write it directly without reading first.
- [x] Do not add unsafe slice casts, a multi-block fixed array, heap allocation,
  or a repo-specific transport abstraction. Multi-block commands may be a later
  measured optimization with explicit caller-owned staging.
- [x] Run reusable-crate gates:

  ```bash
  cargo test -p embedded-sd-storage
  cargo clippy -p embedded-sd-storage --all-targets --all-features -- -D warnings
  cargo check -p embedded-sd-storage --no-default-features \
    --target riscv32imc-unknown-none-elf
  ```

**Gate:** Counting tests prove reads were removed only for safe full-block
overwrites; FAT image tests preserve file correctness; the reusable crate stays
`no_std` and independently compilable.

## Task 5: Add negotiated, bounded two-frame upload pipelining

**Owning crates:** `binbook-diagnostic-protocol`, `binbook-fw`, and `binbook`

**Files:**

- Modify `firmware/crates/binbook-diagnostic-protocol/src/types.rs` and codec
  tests
- Modify firmware diagnostic upload handling only as required by max-frame
  batching evidence
- Modify `crates/binbook/src/upload.rs` and serial transport tests
- Update `docs/specs/2026-07-01-diagnostic-storage-extension-design.md`

- [x] Add a new `CAP_STORAGE_UPLOAD_WINDOW_2` capability bit without changing
  protocol version or existing payload layouts. Add exact HELLO capability tests.
- [x] Add failing fake-peer tests where the host sends two ascending write
  frames before waiting, receives fragmented responses, validates both exact
  sequences and accepted lengths, and advances progress for each acknowledgement.
- [x] Add fallback tests proving a peer without the new capability never has
  more than one write in flight.
- [x] Add failure tests where the first response fails while the second request
  is already in flight. The host must collect/correlate outstanding responses,
  stop issuing new writes, attempt abort, and report the uncertain/confirmed
  cleanup state accurately.
- [x] Use a window of exactly two. Do not expose a larger tuning flag until the
  firmware RX ring, transfer-buffer lifetime, and hardware evidence support it.
- [x] Keep writes strictly ascending because firmware CRC and FAT append state
  are sequential. Pipelining overlaps transport latency; it does not authorize
  parallel or out-of-order SD writes.
- [x] Update the storage design's current-state statements: negotiated window,
  aligned data size, fallback behavior, and acknowledgement semantics.

**Gate:** Capability negotiation, fallback, two-frame success, and partial-
window failure tests pass. A host cannot pipeline against old firmware merely
because it knows protocol v2.

## Task 6: Run the complete automated and target-build matrix

- [x] Start the final matrix from a clean Cargo build so stale feature artifacts
  cannot mask target failures:

  ```bash
  cargo clean
  cargo test -p binbook-diagnostic-protocol
  cargo test -p embedded-sd-storage
  cargo test -p binbook-fw --features diagnostic-console
  cargo test -p binbook-fw --features diagnostic-console,sd-storage
  cargo test -p binbook --features serial-device
  cargo test --workspace
  uv run pytest -q
  cargo fmt --all -- --check
  cargo clippy -p embedded-sd-storage --all-targets --all-features -- -D warnings
  cargo clippy -p binbook-fw --all-targets \
    --features diagnostic-console,sd-storage -- -D warnings
  cargo clippy -p binbook --all-targets --features serial-device -- -D warnings
  git diff --check
  ```

- [x] Build the normal and diagnostic SD images with rustup's pinned nightly
  tools, not arbitrary `cargo`/`rustc` from `PATH`:

  ```bash
  cd firmware && \
    RUSTC="$(rustup which --toolchain nightly rustc)" \
    rustup run nightly cargo build -p binbook-fw \
      --features firmware-bin,sd-storage \
      --target riscv32imc-unknown-none-elf --release

  cd firmware && \
    RUSTC="$(rustup which --toolchain nightly rustc)" \
    rustup run nightly cargo build -p binbook-fw \
      --features firmware-bin,diagnostic-console,sd-storage \
      --target riscv32imc-unknown-none-elf --release
  ```

- [x] Record release image and static section sizes before/after. Inspect any
  material increase for duplicated protocol frames, async state, or transfer
  buffers.

**Gate:** Every focused, feature-gated, workspace, Python, format, Clippy,
target-build, and diff check passes. Passing builds are not hardware evidence.

## Task 6A: Optimize bounded readback symmetrically

**Owning crates:** `binbook-diagnostic-protocol`, `binbook-fw`, and `binbook`

- [x] Add `CAP_STORAGE_READ_WINDOW_2` without changing protocol version or
  payload layouts; require stop-and-wait when the capability is absent.
- [x] Stream response bytes directly to a buffered output and remove partial
  output on failure; never materialize the complete book in a host `Vec`.
- [x] Send exactly two ascending ranges when negotiated and validate opcode,
  sequence, status, length, and EOF before writing either response.
- [x] Cache the file size in the mounted storage owner across consecutive reads
  and invalidate it on mutations.
- [x] Test negotiated batching, legacy fallback, and a speculative second
  response after the first response reaches EOF.
- [x] Read the complete 24,671,670-byte final book back from live hardware,
  require `cmp` and matching SHA-256, then independently query STATUS.

**Observed gate:** the legacy full read had not completed after 32m47s and was
stopped. The optimized read completed in 268.24 seconds; all 24,671,670 bytes
matched, SHA-256 was
`ac1f2c94c91e7fa8d67f9d11a56c5865c077e28c173925c357dd712866a77835`,
and STATUS retained zero protocol errors and `last_error=0`.

## Task 7: Live throughput, integrity, recovery, and compatibility gate

Run this task only after the other agent releases the device. Follow
`docs/reference/xteink-x4-agent-device-verification.md` sequentially and keep a
single owner of `$PORT`.

- [x] Flash the exact candidate image:

  ```bash
  export PORT="${PORT:-/dev/ttyACM0}"
  FW_FEATURES="firmware-bin,diagnostic-console,sd-storage" \
    firmware/scripts/flash-xteink-x4-nav-probe.sh
  ```

- [x] Capture serial output for at least 15 seconds with the AGENTS.md pyserial
  command reproduced below. Then independently query HELLO and STATUS. Require
  the new pipeline capability, expected maximum frame, zero protocol errors,
  and `last_error=0`:

  ```bash
  uv run --with pyserial --no-project python3 -c "
  import serial, time, sys
  ser = serial.Serial('$PORT', 115200, timeout=1)
  ser.dtr = False; ser.rts = False; time.sleep(0.05)
  ser.rts = True; time.sleep(0.05); ser.rts = False; time.sleep(0.1)
  start = time.time()
  while time.time() - start < 15:
      data = ser.read(ser.in_waiting or 1)
      if data:
          sys.stdout.buffer.write(data)
          sys.stdout.flush()
  ser.close()
  "
  target/debug/binbook diag hello --port "$PORT"
  target/debug/binbook diag status --port "$PORT"
  ```
- [x] Run the existing device transport tests one at a time, plus the new
  two-maximum-frame test. Use `--exact --ignored --nocapture --test-threads=1`.
  Follow malformed recovery with a fresh STATUS query and account for its one
  intentional protocol-error increment separately from upload runs. The new
  test command is:

  ```bash
  cargo test -p binbook --features serial-device --test hardware_diagnostic \
    hardware_two_max_frame_storage_write_requests -- \
    --ignored --exact --nocapture --test-threads=1
  ```
- [x] Restore a zero-error baseline by rebooting if needed, then upload the
  tracked 1,022,165-byte fixture three times to the disposable name, deleting
  and independently confirming absence between runs. Capture each JSON summary
  and terminal progress transcript. Use the new user-facing surface for timing:

  ```bash
  FIXTURE=firmware/crates/binbook-fw/fixtures/nav_probe.binbook
  NAME=serial-upload-candidate.binbook
  OUT="${OUT:-target/diag-evidence/serial-upload-candidate}"
  mkdir -p "$OUT"
  /usr/bin/time -p target/debug/binbook upload \
    --port "$PORT" --file "$FIXTURE" --name "$NAME" \
    --progress never --json >"$OUT/upload.json"
  target/debug/binbook diag storage read --port "$PORT" --path "$NAME" \
    --output "$OUT/readback.binbook"
  cmp "$FIXTURE" "$OUT/readback.binbook"
  sha256sum "$FIXTURE" "$OUT/readback.binbook"
  target/debug/binbook diag status --port "$PORT"
  ```
- [x] Require each run to show monotonic acknowledged progress, an explicit CRC
  verification phase, an explicit library-update phase, a matching list entry,
  complete readback `cmp`, matching SHA-256, and zero new protocol errors.
- [x] Compare the candidate median with Task 1. Require at least 4x improvement.
  Record preparation, transfer, verification, and library-refresh times
  separately so the background index scan is not mislabeled as USB throughput.
- [x] Exercise fallback by flashing the baseline firmware or using a committed
  fake/compatibility peer: new host must use stop-and-wait when the capability
  is absent and still upload byte-identically.
- [x] Inject one host-side transfer failure after at least two acknowledged
  chunks. Confirm abort response, then independently require `StoreRead`
  `NotFound` and no list entry. A transport acknowledgement without the follow-
  up queries is insufficient.
- [x] Run list, full read, delete, begin, abort, and final STATUS as independent
  commands. Restore the starting library contents and leave the device in the
  agreed image/state.
- [x] Update `HANDOFF.md` immediately with exact commands, full relevant output,
  starting/ending state, baseline/candidate medians, CRC/SHA evidence, cleanup
  evidence, failures, and remaining unverified claims.

No webcam result is required for upload throughput itself because this plan does
not change rendering, orientation, refresh policy, or visible display content.
If the implementation adds any panel progress UI or changes library rendering,
that expands scope and requires a fresh native-resolution webcam capture and the
display acceptance rules from AGENTS.md.

**Gate:** The real device meets the relative throughput target and every
integrity, cleanup, compatibility, and zero-regression criterion. If the speed
target fails, do not raise the SD clock immediately; use the captured phase
timings to select the next measured bottleneck.

## Contingency: only if the primary target is still missed

Use the phase evidence in this order:

1. If USB receive time still dominates, inspect async wake/drain behavior and
   endpoint backpressure with a minimal diagnostic counter. Do not increase
   buffers first.
2. If SD time dominates and full-sector read elimination is confirmed, evaluate
   safe multi-block writes with caller-owned staging and counting-device tests.
3. Only then run a separately documented 4/8 MHz SD clock qualification on the
   live card. The observable signal is repeated upload/readback identity and
   stable display/SD coexistence; an SPI configuration acknowledgement is not
   evidence. Test cold boot, three uploads, three complete readbacks, library
   scans, page opens, and display operations at each frequency. Keep 4 MHz if
   any CRC, mount, read, write, display, or reboot instability appears.

## Documentation and final adversarial review

- [x] Update the diagnostic storage design to describe implemented current
  behavior rather than the old promised behavior.
- [x] Update CLI help/reference documentation with top-level upload examples,
  progress modes, non-TTY behavior, library-wait semantics, and failure cleanup.
- [x] Search `docs/ROADMAP.md` for `upload`, `serial`, `pipeline`, `progress`,
  `storage`, and the eventual branch/diagnostic names. Remove or rewrite any
  item that this work completes; leave genuinely future clock or multi-block
  experiments as future work only if evidence still justifies them.
- [x] Keep host-local artifact paths and transient device state in `HANDOFF.md`,
  not tracked documents.
- [x] Adversarially try to disprove every completion claim:
  - make the second acknowledgement short;
  - return a stale same-sequence response with the wrong opcode;
  - split a response at every byte boundary;
  - batch two maximum COBS frames;
  - fail after a later request is already in flight;
  - upload a file larger than all scratch buffers;
  - replace a same-name/same-size file;
  - confirm progress never reaches success on any failure;
  - inspect the release image for duplicate 4 KiB buffers;
  - independently read back and compare the final bytes.

## Acceptance matrix template

Fill this with actual results before completion. `Not applicable` is acceptable
for webcam only while no display behavior changes.

| Requirement | Implementation path | Automated test | Observed device evidence |
| --- | --- | --- | --- |
| Honest acknowledged progress | host upload/progress modules | fake clock/peer success, short-ack, stale-response, and output-mode tests | captured TTY and JSON output matches acknowledged bytes |
| Bounded host streaming | host upload reader | file larger than several chunks; bounded-read spy | 1,022,165-byte upload succeeds without whole-file buffer |
| Event-driven USB receive | firmware diagnostic task | fragmented max frame, completion wake, two-max-frame batch | hardware fragmented/batched tests and measured phase timing |
| Aligned, read-free full-sector overwrite | embedded SD block stream | counting block-device and FAT image tests | upload/readback identity; SD phase timing |
| Negotiated two-frame window | protocol capability + host window | capability, fallback, two-frame, and failure-in-window tests | HELLO capability and successful max-frame window run |
| Failed transfer cleanup | host abort path + owner abort | injected failure and abort-correlation tests | abort response, independent NotFound read, absent list entry |
| Successful transfer integrity | CRC commit + readback | protocol/storage upload tests | CRC match, full `cmp`, SHA-256 match, name/size list entry |
| Library usability after commit | background refresh + bounded wait | library wait success/timeout tests | post-refresh list contains uploaded name/size |
| At least 4x median speedup | all paths above | Not applicable; model is not hardware timing | controlled three-run baseline/candidate medians |
| No transport/storage regression | existing diagnostic and owner paths | full feature-gated matrix | HELLO, STATUS, malformed recovery, CRUD, final zero-error STATUS |
| Bounded firmware memory | unchanged buffers + measured async state | target builds and size inspection | before/after ELF/image size recorded |
| Visible display unchanged | no display code touched | existing display suites | Live page 0 and page 4 were upright and uncropped; STATUS reported 323 pages and zero errors |
| Bounded negotiated readback | host streaming reader + owner file-size cache | negotiated, fallback, and EOF-window tests | 24,671,670 bytes in 268.24 s; `cmp` and SHA-256 exact; zero protocol errors |

Final clean target sizes: normal ELF 222,216 bytes with text/data/BSS
382,296/2,876/204,756; diagnostic ELF 249,772 bytes with text/data/BSS
361,960/2,908/256,656. The diagnostic BSS delta is explained by the existing
feature-gated protocol and storage buffers; no additional whole-book or second
4 KiB transfer buffer was introduced.
