# Xteink X4 BinBook Firmware Flashing

This document records the working flash path for the BinBook bare-metal Rust firmware. It is separate from SquidScript's `squidc target flash` flow.

## Scope

- Target board: Xteink X4, ESP32-C3 over USB JTAG serial.
- Firmware artifact: `firmware/target/riscv32imc-unknown-none-elf/release/binbook-fw`.
- Current firmware behavior: SSD1677 display smoke test that clears the panel and draws four physical corner probe boxes.
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
firmware/scripts/flash-xteink-x4-smoke.sh
```

Use the wrapper above for smoke firmware flashing. Do not use an arbitrary `cargo build --target riscv32imc-unknown-none-elf` from `PATH`; if `cargo` and `rustc` resolve from different toolchain managers, the build can fail with a missing `core` crate or reject nightly-only flags even when the target is installed. The wrapper pins rustup's nightly `cargo` and `rustc`.

Equivalent explicit command:

```bash
cd firmware
RUSTC="$(rustup which --toolchain nightly rustc)" \
  rustup run nightly cargo build \
  -p binbook-fw \
  --features firmware-bin \
  --target riscv32imc-unknown-none-elf \
  --release

${ESPFLASH:-${HOME}/.cargo/bin/espflash} flash \
  --non-interactive \
  --chip esp32c3 \
  --port "${ESPFLASH_PORT:-/dev/ttyACM0}" \
  --flash-size 16mb \
  target/riscv32imc-unknown-none-elf/release/binbook-fw
```

Do not pass `--monitor` for this smoke test. The firmware does not currently emit a useful serial protocol, and USB reset/re-enumeration can disrupt monitor sessions.

## Firmware requirements for `espflash`

The `binbook-fw` binary must include an ESP-IDF app descriptor:

```rust
esp_bootloader_esp_idf::esp_app_desc!();
```

Without this descriptor, `espflash` connects to the chip but refuses the image with an error saying the ESP-IDF App Descriptor is missing.

## Verified flash result

On 2026-06-25, the command above flashed the four-corner smoke firmware successfully with:

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

Current smoke-test display behavior: a clear screen followed by four filled physical probe boxes:

- one filled black 128×96 box at physical coordinate `(0, 0)`,
- one filled black 128×96 box at physical coordinate `(672, 0)`,
- one filled black 128×96 box at physical coordinate `(0, 384)`,
- one filled black 128×96 box at physical coordinate `(672, 384)`.

The smoke firmware first clears both SSD1677 RAM planes to white and performs a full refresh. It then writes four 128×96 black windows using SSD1677 window writes and performs another full refresh. This keeps the hardware milestone focused on reset, init, RAM-window writes, coordinate coverage, and full refresh. It does not yet verify BinBook page decoding, flash storage, buttons, serial commands, or logical portrait orientation.

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

Do not convert X coordinates to byte addresses for these commands. A prior Rust version sent `0x44 = [0x00, 0x63]` and `0x4E = [0x00]`, which produced malformed multi-stripe output even though the panel refreshed.
