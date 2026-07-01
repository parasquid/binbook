# Xteink X4 BinBook Firmware Flashing

This document records the working flash path for the BinBook bare-metal Rust firmware. It is separate from SquidScript's `squidc target flash` flow.

## Scope

- Target board: Xteink X4, ESP32-C3 over USB JTAG serial.
- Firmware artifact: repository-root `target/riscv32imc-unknown-none-elf/release/binbook-fw`.
- Current firmware behavior: 16-page labeled navigation probe with directional button and diagnostic KEY page turns.
- Flash tool: `espflash 4.4.0`.

SquidScript's command:

```bash
cargo run -p squidc -- target flash --target xteink-x4
```

is not the right command for this repo's Rust artifact. It wraps Zephyr's `west flash` for SquidScript's `build/zephyr/xteink-x4/zephyr.bin`.

## Prerequisites

Install `espflash` if it is not already installed:

```bash
cargo install espflash
```

The flash wrapper defaults to:

```text
${HOME}/.cargo/bin/espflash
```

The Xteink X4 is expected at:

```text
/dev/ttyACM0
```

If the device appears elsewhere, set `ESPFLASH_PORT` to the correct path.

## Working command

From the repo root:

```bash
firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Use the wrapper above for navigation-probe firmware flashing. Do not use an arbitrary `cargo build --target riscv32imc-unknown-none-elf` from `PATH`; if `cargo` and `rustc` resolve from different toolchain managers, the build can fail with a missing `core` crate or reject nightly-only flags even when the target is installed. The wrapper pins rustup's nightly `cargo` and `rustc`.

Equivalent explicit command:

```bash
ROOT="$(pwd)"
(
  cd firmware
  RUSTC="$(rustup which --toolchain nightly rustc)" \
    rustup run nightly cargo build \
    -p binbook-fw \
    --features firmware-bin \
    --target riscv32imc-unknown-none-elf \
    --release
)

${ESPFLASH:-${HOME}/.cargo/bin/espflash} flash \
  --non-interactive \
  --chip esp32c3 \
  --port "${ESPFLASH_PORT:-/dev/ttyACM0}" \
  --flash-size 16mb \
  "${ROOT}/target/riscv32imc-unknown-none-elf/release/binbook-fw"
```

Do not pass `--monitor` to the flash command. Let flashing and USB re-enumeration finish, then capture debug text with the pyserial command in `AGENTS.md` or use the diagnostic CLI for packet firmware; headless `espflash monitor` cannot initialize its input reader on this host.

## Firmware requirements for `espflash`

The `binbook-fw` binary must include an ESP-IDF app descriptor:

```rust
esp_bootloader_esp_idf::esp_app_desc!();
```

Without this descriptor, `espflash` connects to the chip but refuses the image with an error saying the ESP-IDF App Descriptor is missing.

## Verified smoke flash result

On 2026-06-25, the previous four-corner smoke firmware flashed successfully with:

```text
Chip type:         esp32c3 (revision v0.4)
Crystal frequency: 40 MHz
Flash size:        16MB
Features:          WiFi, BLE
App/part. size:    90,784/16,384,000 bytes, 0.55%
Flashing has completed!
```

Verified display result after reset on 2026-06-25:

- four filled black 128×96 boxes,
- one box at each physical display corner,
- no center vertical stripe.

The previous smoke-test display behavior was a clear screen followed by four filled physical probe boxes:

- one filled black 128×96 box at physical coordinate `(0, 0)`,
- one filled black 128×96 box at physical coordinate `(672, 0)`,
- one filled black 128×96 box at physical coordinate `(0, 384)`,
- one filled black 128×96 box at physical coordinate `(672, 384)`.

The smoke firmware first cleared both SSD1677 RAM planes to white and performed a full refresh. It then wrote four 128×96 black windows using SSD1677 window writes and performed another full refresh. That milestone focused on reset, init, RAM-window writes, coordinate coverage, and full refresh.

## Current navigation probe

The current `binbook-fw` binary initializes the SSD1677 grayscale path, opens `firmware/crates/binbook-fw/fixtures/nav_probe.binbook` via `include_bytes!`, and renders page 0. The fixture contains 16 labeled `GRAY2_PACKED` pages stored at `800x480` with RLE PackBits compression.

Every page retains the orientation/calibration frame and a large `PAGE NN` label. Distinct checker, stripe, text, diagonal, crosshatch, rectangle, bar, quadrant, dot, and asymmetric X patterns identify individual pages.

Button mapping:

- `Right` / `Down` — next page
- `Left` / `Up` — previous page
- `Back` / `Select` / `Power` — ignored

Navigation clamps at edges: previous on page 0 does nothing; next on page 15 does nothing.

Boot displays `PAGE 00`. Directional input navigates through `PAGE 01` to `PAGE 15` and backward through the same sequence.

The verified Rust GRAY2 plane mapping for the Xteink X4 grayscale LUT is:

| BinBook value | Meaning | SSD1677 secondary/red RAM | SSD1677 black RAM |
|---------------|---------|---------------------------|-------------------|
| 0 | black | active | active |
| 1 | dark gray | active | inactive |
| 2 | light gray | inactive | active |
| 3 | white | inactive | inactive |

For this absolute four-gray LUT, `active` is encoded as RAM bit `1` and
`inactive` as RAM bit `0`. The staged native slots use different differential
semantics. `full-refresh-current` reconstructs absolute planes while streaming:
`red = !(base | (msb & !lsb))` and `black = !(base | lsb)`.

## Driver details captured from bring-up

The Rust SSD1677 driver must match SquidScript's working SSD1677 path:

- Data entry mode: `0x03`, X-increment/Y-increment horizontal write mode.
- Hardware reset: physical reset high for 20 ms, low for 20 ms, high for 200 ms, then wait for ready.
- Init sequence: `0x12`, wait ready, `0x18 = [0x80]`, `0x0C = [0xAE, 0xC7, 0xC3, 0xC0, 0x80]`, `0x01 = [0xDF, 0x01, 0x02]`, `0x11 = [0x03]`, `0x3C = [0x01]`, then full window.
- X RAM address range command `0x44`: four bytes, little-endian 16-bit physical pixel coordinates: `x0_lo, x0_hi, x1_lo, x1_hi`.
- Y RAM address range command `0x45`: four bytes, little-endian 16-bit physical row coordinates: `y0_lo, y0_hi, y1_lo, y1_hi`.
- X counter command `0x4E`: two bytes, little-endian 16-bit physical pixel coordinate.
- Y counter command `0x4F`: two bytes, little-endian 16-bit physical row coordinate.
- Clear/probe path: clear both `0x26` secondary/red RAM and `0x24` black RAM to white before drawing probe windows.
- Full refresh sequence: `0x22 = [0xF7]`, then `0x20`.
- Partial refresh sequence: `0x21 = [0x00, 0x00]`, `0x22 = [0xFC]`, then `0x20`.
- Grayscale init additionally writes the SquidScript four-gray LUT with command `0x32`, gate voltage `0x03`, source voltage `0x04`, and VCOM voltage `0x2C`.
- Staged grayscale refinement loads the pinned 105-byte CrossPoint differential
  LUT plus firmware-owned voltage bytes, sends `0x21 = [0x00]`, then uses
  `0x22 = [0x0C]` when the controller is already powered or `0x22 = [0xCC]`
  after a powered-down full seed, followed by `0x20` and a BUSY wait. It
  performs no reset.
- Absolute `0xC7` grayscale remains an explicit full-probe mechanism only; it
  is not used for normal page refinement.

Do not convert X coordinates to byte addresses for these commands. A prior Rust version sent `0x44 = [0x00, 0x63]` and `0x4E = [0x00]`, which produced malformed multi-stripe output even though the panel refreshed.

## Staged-grayscale diagnostic build

Build the staged-grayscale diagnostic firmware with:

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
```

The removed `deferred-gray-probe` feature must not be used. BinBook carries
waveform hint `2`; LUT revision `1` and voltage bytes remain firmware-owned.
Structured overlay, activation, controller-state, and base-sync evidence is
retrieved through the diagnostic protocol.

## Diagnostic console build

For complete AI-agent hardware verification, including discriminating state checks, visual gates, stream tests, failure handling, and evidence requirements, follow [`xteink-x4-agent-device-verification.md`](xteink-x4-agent-device-verification.md).

To build firmware with the diagnostic serial console enabled:

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
```

Flash with diagnostic console:

```bash
FW_FEATURES="firmware-bin,diagnostic-console" firmware/scripts/flash-xteink-x4-nav-probe.sh
```

The diagnostic console uses USB Serial/JTAG (`/dev/ttyACM0`) for COBS-framed binary protocol communication. Only one process can own the serial port at a time.

The `diagnostic-console` feature is separate from `debug-log`. When both are
enabled, the binary packet console owns USB Serial/JTAG and `dbgprintln!`
compiles to a no-op; text output is not mirrored onto the packet stream. The
combined build remains supported:

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console,debug-log --target riscv32imc-unknown-none-elf --release
```

CLI commands for diagnostic console (requires `serial-device` feature):

```bash
SYSTEMD_PREFIX="$(brew --prefix systemd)"
export PKG_CONFIG_PATH="$SYSTEMD_PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
export LIBRARY_PATH="$SYSTEMD_PREFIX/lib:${LIBRARY_PATH:-}"
export LD_LIBRARY_PATH="$SYSTEMD_PREFIX/lib:${LD_LIBRARY_PATH:-}"
cargo build -p binbook --features serial-device
target/debug/binbook diag hello --port /dev/ttyACM0
target/debug/binbook diag key --port /dev/ttyACM0 RIGHT
target/debug/binbook diag page --port /dev/ttyACM0 next
target/debug/binbook diag page --port /dev/ttyACM0 goto 3
target/debug/binbook diag status --port /dev/ttyACM0
target/debug/binbook diag logs --port /dev/ttyACM0 --since 0
target/debug/binbook diag logs --port /dev/ttyACM0 --clear
target/debug/binbook diag crash --port /dev/ttyACM0
target/debug/binbook diag crash --port /dev/ttyACM0 --clear
target/debug/binbook diag probe --port /dev/ttyACM0 window-corners
```
