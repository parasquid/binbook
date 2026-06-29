# Rust Modular Foundation Design

## Status and authority

This document defines the target Rust architecture for the BinBook 0.1 reference implementation and Xteink X4 firmware. `BINBOOK_FORMAT_SPEC.md` remains authoritative for every wire-format field, section layout, color value, X4 plane slot, and chunk rule.

The existing `rust/` crate is intentionally replaced rather than preserved behind a compatibility facade. SquidScript is not migrated by this refactor; it may consume these crates after its post-Zephyr firmware architecture is selected.

## Goals

- Make format, decompression, rendering, controller, and X4 display behavior independently testable `no_std` libraries.
- Keep reusable crates free of allocation, ESP32-C3, Embassy, diagnostics, CLI, and repository-file dependencies.
- Make every temporary decode/render buffer caller-owned and explicitly sized.
- Preserve Python encoder and CLI behavior and all BinBook 0.1 bytes.
- Preserve the verified X4 navigation, staged-grayscale, cancellation, and recovery behavior.

## Workspace

The repository has one root Cargo workspace:

```text
Cargo.toml
Cargo.lock
crates/
├── binbook-core/
├── binbook-decompress/
├── gray2-render/
├── ssd1677-driver/
└── xteink-x4-display/
cli/
firmware/
├── crates/binbook-diagnostic-protocol/
└── crates/binbook-fw/
```

Dependency direction is strictly layered:

```text
binbook-decompress -> binbook-core

gray2-render
ssd1677-driver -> embedded-hal, embedded-hal-async

xteink-x4-display
├── binbook-core
├── binbook-decompress
├── gray2-render
└── ssd1677-driver

binbook-fw
├── xteink-x4-display
├── binbook-diagnostic-protocol
├── embedded-storage
├── Embassy
└── esp-hal

binbook-cli
├── binbook-core
└── binbook-diagnostic-protocol
```

Library crates may not depend on `binbook-fw`, the CLI, diagnostic protocol, Embassy, `esp-hal`, or files outside their crate. `binbook-core` may not depend on decompression, rendering, or controller crates. `gray2-render` may not depend on BinBook or SSD1677 crates. `ssd1677-driver` may not depend on BinBook, X4, firmware, or custom HAL traits.

## Crate contracts

### `binbook-core`

`binbook-core` validates headers, section directories, records, bounds, typed identifiers, string references, and X4 profile metadata through a random-access source:

```rust
pub trait ReadAt {
    type Error;

    fn len(&mut self) -> Result<u64, Self::Error>;
    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), Self::Error>;
}
```

`Book<R>` stores the source and validated section/index locations only. Opening and record access use caller-provided scratch buffers. Page, plane, chunk, length, and offset values use typed newtypes or enums. Source failures remain distinguishable from format failures and undersized output buffers report exact required/provided sizes.

The public API does not include placeholder metadata, raw-number identifiers, `PageRef`, `decompress_page()`, or an aggregate of concatenated compressed planes.

### `binbook-decompress`

`binbook-decompress` owns exact PackBits and optional LZ4 decoding. `decode_exact` rejects malformed input, unsupported or disabled methods, short output, and overlong output. `PackBitsDecoder` carries partial run state across caller-selected input and output strips without rescanning.

No operation allocates or chooses a page/plane destination. Each decode writes only to the output slice selected by the caller.

### `gray2-render`

`gray2-render` owns pure typed transformations for canonical GRAY2 bytes, SSD1677 red/black plane bits, staged X4 plane reconstruction, ordered BW dithering, and row conversion. It has no BinBook section, SSD1677 command, panel refresh, or hardware knowledge.

Canonical storage remains `0=black`, `1=dark gray`, `2=light gray`, `3=white`, packed row-major MSB-first. The X4 staged slots remain overlay MSB, overlay LSB, and fast BW base.

### `ssd1677-driver`

`ssd1677-driver` owns controller commands, window/counter programming, RAM writes, refresh activation, reset, busy waits, and controller power state. It accepts `embedded_hal::spi::SpiDevice<u8>`, digital pins, synchronous `DelayNs`, and asynchronous `embedded_hal_async::delay::DelayNs`.

Driver errors preserve SPI, pin, timeout, window, and buffer categories. The driver owns chip-select behavior through `SpiDevice`. X4 LUT bytes, voltage policy, rotation, and refresh policy do not belong in this crate.

### `xteink-x4-display`

`xteink-x4-display` owns the verified X4-specific display pipeline: logical `480x800`, physical `800x480`, 270-degree clockwise rotation, 16-row chunks, three-plane validation, staged-gray cancellation, BW base synchronization, controller state, page commit timing, refresh decisions, and display probes.

Compressed and decoded buffers are borrowed through `RenderBuffers`. Page reads use `binbook_core::Book<R>` where `R: ReadAt`; decompression and plane conversion use the lower-level crates. Semantic `DisplayEvent` values cross the firmware boundary, while diagnostic numeric event mappings remain firmware-owned.

The crate does not own Embassy channels, diagnostic frames, ESP peripherals, flash partitions, physical input, or application lifecycle.

### `binbook-fw` and CLI

`binbook-fw` contains platform wiring only: ESP32-C3 peripheral setup, Embassy tasks/channels, ADC button sampling, storage adapters, diagnostic adapters, and application lifecycle. It must not contain format parsing, PackBits/LZ4 decoding, GRAY2 conversion, SSD1677 commands, or refresh state.

The CLI uses the reusable parser and diagnostic protocol but remains a host application. Python authoring and CLI behavior are unchanged by this architecture refactor.

## Memory and error boundaries

- All decode, row, and command buffers are caller-owned.
- No library allocates full-page storage or hides a fixed 8 KiB scratch buffer.
- `ReadAt` and `embedded-storage` adapters preserve their source errors.
- Format, decode, render, controller, timeout, cancellation, and firmware transport errors remain distinct at their owning layer.
- A page becomes current only after the required display operation succeeds.

## Hardware gates

Host tests cannot establish SSD1677 or visible display correctness. The driver migration and final integrated firmware each require sequential flash, at least 15 seconds of serial capture, independent HELLO/STATUS/log queries, and fresh native-resolution webcam evidence. Probe acknowledgements count only as transport evidence until controller events, resulting state, and panel output are independently verified.

The final `HANDOFF.md` acceptance matrix must link each requirement to implementation, automated proof, serial proof, and webcam proof. A missing required cell keeps the refactor incomplete.

## Exclusions

- No BinBook 0.1 wire-format change.
- No `GRAY4_PACKED` output for `xteink-x4-portrait`.
- No C ABI or compatibility facade for the removed `rust/` API.
- No SquidScript source change.
- No Python package or CLI architecture cleanup beyond imports needed for the new crates.
