# Xteink X4 Diagnostic Serial Console Design

Date: 2026-06-26
Status: Planned
Target: Xteink X4 BinBook firmware on ESP32-C3 USB Serial/JTAG

## Summary

The diagnostic serial console lets host tooling drive real Xteink X4 firmware
over USB serial while keeping the firmware suitable for constrained devices.
The first version is a debug/development facility, not a production user
interface. It can inject logical button events, navigate reader pages, request
status, retrieve logs, retrieve crash summaries, and run display probes.

The protocol is not SquidScript-compatible. It uses a small BinBook-specific
binary packet format because the design goal is low RAM use, compact packets,
and predictable firmware behavior.

## Goals

- Drive the actual device through serial commands while the firmware runs.
- Exercise the same firmware paths used by physical buttons wherever possible.
- Retrieve diagnostics without relying on unsolicited serial prints.
- Preserve recent runtime logs in SRAM and compact reset-surviving crash
  summaries in flash.
- Compile the entire diagnostic console out of release firmware through a
  single Cargo feature.

## Non-Goals

- Do not add JSON, CBOR, protobuf, or SquidScript TLV frames to the firmware.
- Do not implement content upload/list/delete in this feature.
- Do not turn serial debug control into a required production protocol.
- Do not stream logs continuously over serial; the host asks for logs.
- Do not write every log record to flash.

## Protocol

Use COBS-framed binary packets over ESP32-C3 USB Serial/JTAG. COBS provides
packet boundaries on the serial byte stream, with `0x00` as the frame delimiter.
The BinBook protocol defines the header, opcodes, payloads, and status codes.

Frame format before COBS encoding:

```text
magic[2]      = "BB"
version:u8    = 1
kind:u8       = 1 request, 2 response, 3 event
opcode:u8
status:u8     = 0 ok, nonzero error in responses
sequence:u16  = host-selected sequence echoed by responses
payload_len:u16
payload[payload_len]
crc16:u16     = CRC-16/CCITT-FALSE over all preceding frame bytes
```

Named constants:

- `PROTOCOL_VERSION = 1`
- `MAX_FRAME_BYTES = 512`
- `MAX_PAYLOAD_BYTES = 496`
- `FRAME_DELIMITER = 0x00`

Payloads are fixed binary structs or short fixed records. Strings are avoided in
firmware-facing payloads unless explicitly bounded by the opcode. Unknown
opcodes and malformed frames produce error responses when a sequence can be
read safely; otherwise the frame is dropped and a protocol error counter is
incremented.

## Opcodes

V1 opcodes:

| Opcode | Direction | Purpose |
| --- | --- | --- |
| `HELLO` | request/response | Return firmware identity, protocol version, max frame size, and enabled diagnostic capabilities. |
| `KEY` | request/response | Inject a logical key event: left, right, up, down, select, back, or power. |
| `PAGE` | request/response | Navigate next, previous, first, last, goto index, or report current page. |
| `STATUS` | request/response | Return current page, page count, display panel mode, log counters, and last error code. |
| `LOG_GET` | request/response | Read SRAM log records from a sequence cursor with a byte budget. |
| `LOG_CLEAR` | request/response | Clear the SRAM log ring and dropped-record counter. |
| `CRASH_GET` | request/response | Read the compact flash crash summary. |
| `CRASH_CLEAR` | request/response | Clear the compact flash crash summary. |
| `DISPLAY_PROBE` | request/response | Run a named diagnostic display probe when `diagnostic-console` is enabled. |

`KEY` must feed the same logical page-turn path as physical button events.
`PAGE` may directly request reader actions, but it must use the same render
function as normal navigation after it resolves the target page.

Initial display probes:

- `full_refresh_current`: render the current page through the normal full
  refresh path.
- `clear_white`: clear both SSD1677 planes to white and refresh.
- `window_corners`: write a known physical-corner pattern for coordinate and
  window diagnostics.

## Logging

The firmware keeps two diagnostic stores when `diagnostic-console` is enabled.
Existing development-only `dbgprintln!` diagnostics in the firmware should be
converted to structured diagnostic log events when they describe navigation,
ADC/button sampling, refresh decisions, panel mode, command handling, or display
errors. `debug-log` may mirror selected records to serial text for quick manual
sessions, but the packet log is the authoritative diagnostic path when the host
owns the serial channel.

### SRAM Ring

The SRAM ring stores recent structured records:

```text
sequence:u32
tick_ms:u32
level:u8
subsystem:u8
event:u16
arg0:i32
arg1:i32
arg2:i32
```

The ring size is a named constant, initially `DIAG_LOG_RECORDS = 256`. When the
ring is full, the oldest records are overwritten and `dropped_records` is
incremented. `LOG_GET` returns records in ascending sequence order starting at
the requested cursor. The host formats event IDs into readable names.

Idle logging must be deduplicated. The main loop may log an `IdleEntered` event
when it transitions from active work to idle, and it may log a bounded
`IdleSummary` event at a named interval such as `IDLE_SUMMARY_MS = 5000`.
Repeated idle loop ticks must not produce one log record per tick. The log
deduper tracks the last low-value repeating event and suppresses repeats while
maintaining a suppressed-repeat count that can be included in the next summary
or state transition record.

High-volume sampling logs, including ADC/button samples, must also be bounded by
named sampling constants. Event-driven records such as key press, page turn,
render start, render result, command receipt, command error, and display error
are logged immediately.

### Flash Crash Summary

Flash stores only compact reset-surviving summaries for fatal errors or explicit
crash-log flushes. It does not store routine debug traffic. The record includes:

- magic/version
- boot counter if available
- last fatal error code
- last page index and panel mode
- last log sequence copied into the summary
- a small fixed number of recent structured log records
- CRC32 for the summary record

Flash writes are rare, bounded, and documented. The implementation must avoid
adding a high-frequency flash write path to the main loop.

## Feature Gates

Add one Cargo feature to `binbook-fw`:

```toml
diagnostic-console = []
```

When the feature is disabled:

- serial diagnostic RX/TX is not compiled into `main.rs`;
- SRAM diagnostic log storage is not allocated;
- flash crash-log writes are not compiled in;
- display probe command handling is not compiled in;
- existing physical button navigation still works.

The existing `debug-log` feature remains separate and continues to gate serial
printing through `dbgprintln!`. The diagnostic console must not depend on
`debug-log`. After this feature lands, diagnostic information should not be
added only as `dbgprintln!`; add a structured diagnostic log event first, then
optionally mirror it through `dbgprintln!` when `debug-log` is enabled.

## Host CLI

Extend the Rust CLI behind the existing `serial-device` feature. The default
CLI build must continue to compile and test without host serial dependencies.

User-facing commands:

```bash
binbook-cli diag hello --port /dev/ttyACM0
binbook-cli diag key --port /dev/ttyACM0 RIGHT
binbook-cli diag page --port /dev/ttyACM0 next
binbook-cli diag page --port /dev/ttyACM0 goto 3
binbook-cli diag status --port /dev/ttyACM0
binbook-cli diag logs --port /dev/ttyACM0 --since 0
binbook-cli diag logs --port /dev/ttyACM0 --clear
binbook-cli diag crash --port /dev/ttyACM0
binbook-cli diag crash --port /dev/ttyACM0 --clear
binbook-cli diag probe --port /dev/ttyACM0 window-corners
```

The CLI opens the serial port, sends one request at a time, waits for the
matching sequence response, and reports timeouts clearly. It must not run in
parallel with another process owning the same serial device.

## Testing And Verification Requirements

Development must be test-driven. Each implementation task starts with a failing
host test, then adds the smallest implementation needed to pass it.

Required host checks:

```bash
cd firmware && cargo test --workspace
cd cli && cargo test
cd cli && cargo test --features serial-device
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
```

Hardware verification is a completion gate, not optional follow-up. The
implementer must flash diagnostic firmware, send serial commands to the real
device, confirm visible page/display behavior, retrieve logs, and record the
exact command output plus visual result in `HANDOFF.md`.

Required hardware checks:

- `diag hello` returns protocol version, target, firmware name, and diagnostic
  capabilities.
- `diag key RIGHT` changes the visible page exactly as a physical Right button
  press would.
- `diag key LEFT` changes the visible page exactly as a physical Left button
  press would.
- `diag page goto 0` renders page 0 and reports page 0 in `diag status`.
- `diag logs` returns records that include command receipt and page render
  events.
- `diag crash` returns either an empty crash summary or the latest valid summary
  with a valid CRC.
- `diag probe window-corners` visibly renders the expected corner pattern.
