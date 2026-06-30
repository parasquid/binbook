# Handoff: Rust Modular Foundation Refactor

Date: 2026-06-30

## Current state

Tasks 0–9 of `docs/plans/2026-06-30-rust-modular-foundation-refactor.md` are implemented. Task 7 passes on the attached Xteink X4 after correcting the migrated absolute-grayscale LUT boundary. Task 8 extracts the complete X4 display policy, and Task 9 reduces firmware to platform wiring. Tasks 10–13 remain.

The device is running `firmware-bin,diagnostic-console`, page 0, grayscale mode. Independent STATUS reports 16 pages, zero dropped logs, zero protocol errors, and `last_error=0`.

## Implemented foundation

- Unified root Cargo workspace.
- Reusable `binbook-core`, `binbook-decompress`, `gray2-render`, and embedded-hal 1.0 `ssd1677-driver` crates.
- Reusable `xteink-x4-display` owns X4 profile validation, caller-owned streaming buffers, staged/absolute rendering, cancellation, refresh policy, probes, and display-engine state.
- SSD1677 driver accepts `SpiDevice<u8>`, embedded-hal digital pins, sync delay, and async delay without importing `xteink-hal`.
- X4 staged and absolute waveform bytes remain outside the generic driver and are owned by `xteink-x4-display`.
- Firmware `main.rs` is a composition root; focused runtime modules own ADC polling, display adaptation, diagnostic aggregation, and USB diagnostic transport.
- `xteink-hal` and the firmware-local display, panel-driver, refresh, parsing, and PackBits paths are removed.

## Driver-gate defect and fix

The first Task 7 run produced correct corner and clear-white probes but an almost-black `full-refresh-current` image. Old/new SPI trace comparison found that the migrated `GRAY_LUT` contained two extra zero bytes before its tail. That shifted controller policy to gate `0x22`, source `[0x22, 0x17, 0x41]`, and VCOM `0xA8` instead of gate `0x17`, source `[0x41, 0xA8, 0x32]`, and VCOM `0x30`.

The array is restored byte-for-byte. A firmware integration test now locks the 105-byte LUT tail and all voltage bytes. Independent Python-derived fixture offsets and full-plane hashes also lock parser, decompression, and absolute-plane output.

## Host verification

- `cargo test -p ssd1677-driver`: passes.
- `cargo check -p ssd1677-driver --no-default-features --target riscv32imc-unknown-none-elf` with the pinned nightly `rustc`: passes.
- `cargo clippy -p ssd1677-driver --all-targets -- -D warnings`: passes after the LUT fix.
- `cargo test --workspace`: passes after the LUT fix.
- Pinned RISC-V diagnostic release build with `firmware-bin,diagnostic-console`: passes after the LUT fix.
- Task 9 `cargo test -p binbook-fw`, diagnostic feature tests, and workspace tests pass.
- Task 9 pinned RISC-V release sizes:
  - Default: 1,090,836-byte ELF, up 6,400 bytes (0.59%) from the 1,084,436-byte checkpoint.
  - Diagnostic: 1,109,036-byte ELF, up 7,344 bytes (0.67%) from the 1,101,692-byte checkpoint.
  - The bounded growth corresponds to the reusable display engine, semantic event adapter, and caller-owned streaming state; no whole-page or fixed 8 KiB scratch buffer was introduced.

## Live device evidence

- Final flash: `/tmp/binbook-rust-refactor-driver/flash.txt`.
  - ESP32-C3 revision v0.4, 16 MB flash.
  - Application 1,121,440 bytes.
  - `Flashing has completed!`.
- Final 15-second boot capture: `/tmp/binbook-rust-refactor-driver/boot.txt`.
- HELLO: `/tmp/binbook-rust-refactor-driver/hello.txt` reports protocol 1, frame limit 512, firmware `binbook-fw`, target `xteink-x4`, and required capabilities.
- Baseline STATUS: `/tmp/binbook-rust-refactor-driver/status-before.txt` reports page 0 of 16, grayscale, and zero errors.
- Final STATUS: `/tmp/binbook-rust-refactor-driver/status-after.txt` reports page 0 of 16, grayscale, and zero errors.
- Final logs: `/tmp/binbook-rust-refactor-driver/logs-after.txt`; startup recovery, staged overlay, and base sync complete without error.
- Webcam captures:
  - `/tmp/binbook-rust-refactor-driver/window-corners.jpg`: four physical corner rectangles visible.
  - `/tmp/binbook-rust-refactor-driver/clear-white.jpg`: active panel uniformly white.
  - `/tmp/binbook-rust-refactor-driver/full-refresh.jpg`: PAGE 00 orientation frame restored with distinct black, dark-gray, light-gray, and white swatches.
  - Cropped inspection files use `crop=440:770:770:250` and are stored beside the originals.

## Driver-gate acceptance matrix

| Requirement | Implementation path | Automated test | Observed evidence | State |
|---|---|---|---|---|
| embedded-hal command/reset behavior | `crates/ssd1677-driver` | external command tests | successful init and all probes | Verified |
| window/counter endianness | driver window methods | `tests/windows.rs` | four correct physical corner rectangles | Verified |
| full and partial refresh controls | driver refresh state | `tests/refresh.rs` | clear/corner probes complete | Verified |
| absolute grayscale LUT and voltages | `panel_driver.rs` X4 policy | `grayscale_init_uses_verified_lut_voltage_tail` | PAGE 00 and four swatches restored | Verified |
| parser/decompression/plane stream | core/decompress/render crates | Python-derived offsets and FNV assertions | labeled orientation page matches decoded fixture | Verified |
| protocol state after probes | diagnostic runtime | protocol/engine suites | STATUS: zero protocol/display errors | Verified |

Probe `ok` responses are treated as transport acknowledgements only. Visible outcomes were independently inspected from the fresh webcam files and final state was queried separately.

## Remaining work

1. Task 10: enforce workspace lints, split the rewritten runtime aggregator, and prove independent crate compilation/dependency boundaries.
2. Task 11: run the clean automated regression matrix and CLI/format round trip.
3. Task 12: run the mandatory final serial, navigation, staged-gray, ignored-live-test, and webcam hardware gate.
4. Task 13: update reference documentation, roadmap, stale paths, and the final acceptance matrix.
