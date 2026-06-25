# SquidScript & Xteink X4 Reference

Consolidated research for firmware work. Updated 2026-06-24.

---

## 1. SquidScript Overview

SquidScript is a full-stack platform for constrained display devices — not just firmware.

| Layer | What | Where |
|-------|------|-------|
| Language | Event-driven scripting language | `docs/language_spec.md` |
| Compiler | Rust CLI producing SQBC bytecode | `compiler/rust/crates/squidc-core/` |
| VM | Shared bytecode runtime (host + firmware) | `compiler/rust/crates/squidvm-core/` |
| FFI bridge | C ABI staticlib linking VM to Zephyr | `compiler/rust/crates/squidvm-ffi/` |
| Firmware | Zephyr RTOS C firmware | `firmware/zephyr/` |
| Simulator | Browser-based TypeScript simulator | `simulator/browser/` |
| Device protocol | Serial install/launch protocol | `compiler/rust/crates/squid-device-protocol/` |

**Key insight**: SquidScript apps never get raw framebuffer or filesystem access. The firmware owns all hardware through "service" calls. This is an opinionated sandbox.

**License**: AGPL-3.0

---

## 2. SquidScript Firmware Architecture (Zephyr)

### Build System

Dual: Rust workspace + CMake/Zephyr. The CLI orchestrates both:
```
cargo run -p squidc -- target build --target xteink-x4
cargo run -p squidc -- target flash --target xteink-x4
```

Code generation: Python scripts convert `targets/*.target.json` into C headers and Kconfig values.

### Firmware Source Layout

```
firmware/zephyr/src/
├── main.c                          # Entry point, UART polling, protocol dispatch, deep sleep
├── vm_runtime_display.c            # Display service: draw ops → backend rasterization
├── vm_runtime_display_backend.h    # Backend interface (rasterize_clear/text/rect/binbook/flush)
├── ssd1677_gdeq0426t82_display.c  # SSD1677 driver: SPI commands, framebuffer, BinBook streaming
├── ssd1677_gray2.c/.h              # GRAY2 plane decomposition, dithering, refresh cadence
├── vm_runtime_binbook.c            # BinBook reader from LittleFS
├── device_protocol.c/.h            # Serial device protocol
├── serial_transport.c/.h           # Serial framing
├── app_store.c/.h                  # LittleFS mount, registry scan
├── ble_file_transfer.c             # BLE GATT file transfer
├── xteink_x4_button_probe.c/.h    # ADC ladder button probing
├── vm_runtime_wifi.c               # Wi-Fi service
├── vm_runtime_ble.c                # BLE service
├── vm_runtime_indicator_gpio.c     # LED indicator
├── debug_log.c/.h                  # Debug log buffer
└── squidvm_ffi.h                   # C header for Rust VM FFI
```

### Display Pipeline

Two modes:

**Composed mode** (app screens):
1. App calls `display.clear()`, `display.text()`, `display.rect()`
2. VM runtime calls `sq_display_backend_rasterize_*` → renders into 48KB framebuffer
3. `flush_framebuffer()` sends full frame over SPI to SSD1677

**Streaming mode** (BinBook pages):
1. App calls `binbook.readPage(index)` → gets drawable handle
2. `sq_display_backend_rasterize_binbook()` opens file, decompresses planes directly to SPI
3. No framebuffer needed — rows stream from flash through PackBits decompressor

### Refresh Cadence

- Every 5 pages: full grayscale refresh (MSB + LSB planes)
- Otherwise: BW differential partial refresh (fast, some ghosting)
- Dirty-window optimization: diffs previous vs current display ops

---

## 3. Xteink X4 Hardware

### MCU

- **Chip**: ESP32-C3 (RISC-V, 1 core, 160 MHz)
- **RAM**: 400 KB internal SRAM, 0 KB PSRAM
- **Flash**: 16 MB

### Display

- **Controller**: SSD1677 (Solomon Systech)
- **Panel**: Good Display GDEQ0426T82 (4.26-inch e-paper)
- **Physical resolution**: 800 × 480
- **Logical resolution**: 480 × 800 (portrait)
- **Rotation**: 270° clockwise (logical → physical)
- **Colors**: B/W native, 4-level grayscale via dual-RAM-plane trick
- **Supported formats**: GRAY1_PACKED (1 bpp), GRAY2_PACKED (2 bpp, default)

#### SSD1677 Key Commands

| Cmd | Name | Notes |
|-----|------|-------|
| 0x12 | SW Reset | Required on init |
| 0x10 | Deep Sleep | Reinit required after wake |
| 0x24 | BW RAM Write | Black/white plane |
| 0x26 | RED RAM Write | Grayscale overlay plane |
| 0x22 | Update Control | Triggers display refresh |
| 0x32 | LUT Write | Custom waveform table |
| 0x44/0x45 | RAM X/Y Window | Set address window |
| 0x4E/0x4F | RAM X/Y Counter | Set write cursor |

BUSY pin: active-high, 60-second timeout in driver.

#### Grayscale Rendering (from CrossPoint research)

1. Draw all non-white pixels into BW framebuffer
2. Display BW base frame
3. Build grayscale LSB buffer for dark gray pixels
4. Build grayscale MSB buffer for light gray + dark gray pixels
5. Call `displayGrayBuffer()` to apply grayscale overlay

#### Rust Firmware GRAY2 Plane Mapping

The Rust firmware renders current writer output by converting canonical
`GRAY2_PACKED` bytes into the SSD1677 secondary/red RAM plane and black RAM
plane. Hardware verification on the Xteink X4 showed that the two middle tones
must be assigned as below for the SquidScript four-gray LUT path:

| BinBook Canonical | Meaning | Red/MSB plane | Black/LSB plane |
|-------------------|---------|---------------|-----------------|
| 0 | black | active | active |
| 1 | dark gray | active | inactive |
| 2 | light gray | inactive | active |
| 3 | white | inactive | inactive |

BinBook stores canonical values. Firmware converts to native at display time.

### SPI Interface

**Bus**: SPI2 (shared between display and SD card)

| Signal | GPIO | Notes |
|--------|------|-------|
| SCK | GPIO8 | Shared clock |
| MOSI | GPIO10 | Shared data out |
| MISO | GPIO7 | SD card only (display is write-only) |
| Display CS | GPIO21 | Active-low |
| Display DC | GPIO4 | Command/data select |
| Display RST | GPIO5 | Active-low |
| Display BUSY | GPIO6 | Active-high |
| SD CS | GPIO12 | Active-low |

- SPI mode 0 (CPOL=0, CPHA=0), MSB-first, 8-bit
- Display clock: 4 MHz default (datasheet max: 20 MHz write)
- SD card clock: 400 kHz

### Buttons

7 buttons via ADC ladder + GPIO:

| Button | Pin | Type | ADC Range |
|--------|-----|------|-----------|
| BACK | GPIO1 | ADC ladder | > 2200–2500 |
| SELECT | GPIO1 | ADC ladder | > 1600–2200 |
| LEFT | GPIO1 | ADC ladder | > 750–1600 |
| RIGHT | GPIO1 | ADC ladder | ≤ 750 |
| UP | GPIO2 | ADC ladder | > 750–2200 |
| DOWN | GPIO2 | ADC ladder | ≤ 750 |
| POWER | GPIO3 | GPIO input | Pull-up, active-low, wake-capable |

Debounce: 30 ms. Poll interval: 20 ms.
POWER held 2 seconds → deep sleep. POWER+DOWN within 120 ms → force display refresh.

#### Decode Logic (from SquidScript `xteink_x4_button_probe.c`)

Each ADC channel is decoded independently. The ladder is active-low: idle reads ~4095, button presses pull the value down. The decode uses `<=` thresholds going upward from 0:

```c
// GPIO1: RIGHT/LEFT/SELECT/BACK
if (raw <= 750)   → RIGHT
if (raw <= 1600)  → LEFT
if (raw <= 2200)  → SELECT
if (raw <= 2500)  → BACK
else              → NONE (idle)

// GPIO2: DOWN/UP
if (raw <= 750)   → DOWN
if (raw <= 2200)  → UP
else              → NONE (idle)
```

The two channels are combined with `ch1_button.or(ch2_button)` — ch1 takes priority when both are active (should not happen in normal use). A raw value above 2500 on ch1 or above 2200 on ch2 means no button is pressed on that channel.

**Critical gotcha**: the idle state reads `ch1=4095, ch2=4095`. Using `>` thresholds (e.g. `ch1 > 2200 → Back`) instead of `<=` thresholds incorrectly decodes the idle state as a button press.

### Power

- Battery: ADC voltage divider on GPIO0, 2x multiplier, 3.0–4.2V range
- USB detect: GPIO20, active-high
- Deep sleep: `esp_sleep_enable_timer_wakeup()` + `sys_poweroff()`
- Wake source: POWER button (GPIO3)
- SSD1677 deep sleep: command 0x10, requires full reinit on wake

### Memory Budget

| Resource | Size |
|----------|------|
| Main/protocol stack | 4,864 B |
| VM work stack | 24,576 B |
| Display worker stack | 4,096 B |
| Heap pool | 65,536 B |
| Framebuffer (1-bit) | 48,000 B |
| Max bytecode | 32,768 B |
| Max serialized state | 8,192 B |
| Max display ops | 48 |
| DRAM headroom target | 48 KB |

### Flash Partition Layout (16 MB)

| Partition | Offset | Size |
|-----------|--------|------|
| Slot 0 (firmware) | 0x20000 | 7,936 KB |
| Slot 1 (OTA) | 0x7E0000 | 7,936 KB |
| Storage (LittleFS) | 0xFB0000 | 192 KB |
| Scratch | 0xFE0000 | 124 KB |
| Coredump | 0xFFF000 | 4 KB |

---

## 4. SquidScript BinBook Integration

### How SquidScript Reads BinBook

`vm_runtime_binbook.c`:
- Opens `.binbook` files from LittleFS
- Validates header magic, section table
- Reads page index entries (128 bytes each)
- Exposes page metadata to display backend
- `sq_display_backend_rasterize_binbook()` streams planes directly from flash

### Key Files

- `firmware/zephyr/src/vm_runtime_binbook.c` — BinBook reader
- `firmware/zephyr/src/ssd1677_gdeq0426t82_display.c` — Display driver with BinBook streaming
- `firmware/zephyr/src/ssd1677_gray2.c` — GRAY2 plane decomposition

### What SquidScript Does With BinBook

1. Validates file on load
2. Reads chapter index for navigation
3. Streams page planes on demand (no full-file buffering)
4. Converts canonical GRAY2 → native XTH pixel mapping
5. Manages refresh cadence (full vs partial)

---

## 5. Implications for Fresh Firmware

### What to Reuse (Reference Only)

- SPI pin assignments and init sequences from `ssd1677_gdeq0426t82_display.c`
- SSD1677 command definitions and timing
- GRAY2 plane decomposition logic from `ssd1677_gray2.c`
- Button ADC ladder ranges and debounce logic
- Power management patterns (deep sleep, wake source)

### What to Redesign

- **No Zephyr** — user wants lean no_std
- **No VM/runtime** — pure BinBook reader, no scripting layer
- **No service abstraction** — direct hardware access
- **Modular crates** — so components can be reimported by SquidScript later

### Modularity Constraint

The firmware should be structured as independent crates that SquidScript could adopt:

| Crate | Responsibility | Reusable by SquidScript? |
|-------|---------------|--------------------------|
| `binbook-core` | Format parsing, validation, page indexing | Yes — SquidScript already has a C reader, could replace |
| `binbook-decompress` | RLE_PACKBITS, LZ4 decompression | Yes — currently inline in display driver |
| `ssd1677-driver` | SPI command layer, init sequences | Yes — currently C code |
| `gray2-render` | GRAY2 plane decomposition, dithering | Yes — currently C code |
| `xteink-hal` | GPIO, SPI, ADC, power abstractions | Partially — Zephyr HAL is different |
| `firmware` | Binary entry point, app logic | No — too specific |

---

## 6. Reference File Paths

### In This Repo (binbook)

- `BINBOOK_FORMAT_SPEC.md` — Authoritative format spec
- `docs/reference/xteink-x4-grayscale-research.md` — Display grayscale findings
- `docs/reference/squidscript-and-xteink-reference.md` — This document

### In SquidScript (reference only)

- `targets/xteink-x4.target.json` — Hardware definition
- `firmware/zephyr/boards/xteink_x4.overlay` — Device tree overlay
- `firmware/zephyr/src/ssd1677_gdeq0426t82_display.c` — Display driver
- `firmware/zephyr/src/ssd1677_gray2.c` — GRAY2 decomposition
- `firmware/zephyr/src/xteink_x4_button_probe.c` — Button handling
- `firmware/zephyr/src/vm_runtime_binbook.c` — BinBook reader
- `firmware/zephyr/prj.conf` — Zephyr config
- `scripts/x4-firmware-flash.sh` — Flash script
- `docs/ssd1677_gdeq0426t82_agent_reference.md` — SSD1677 agent reference
- `docs/targets/xteink-x4.md` — Target documentation
