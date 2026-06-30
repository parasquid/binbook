# BinBook Roadmap

## X4 ADC input refactor

Status: evidence-gated.

Current firmware uses synchronous one-shot ADC reads, a 50 ms Embassy timer, and one global 100 ms cooldown. The candidate architecture uses `Adc::into_async()`, interrupt-completed `read_oneshot().await`, 20 ms periodic sampling, independent stable-candidate state for each ADC ladder, and 30 ms debounce matching the verified SquidScript/X4 reference.

ADC conversion completion can be interrupt-driven, but resistor-ladder button detection still requires periodic sampling; GPIO edges cannot distinguish ladder voltages reliably. Continuous ADC/DMA is not the default because it adds power and RAM complexity without removing debounce.

Do not implement this refactor until serial/camera stress plus physical input logs localize the problem to ADC sampling or debounce. Acceptance requires rapid mixed-direction host sequences, calibrated threshold tests, queue/drop evidence, pinned builds, flash, serial capture, and live physical-button confirmation.

## Python authoring package modularization

Split binary format models/writing, EPUB ingestion, raster rendering, viewer, and kerning-proof server into independently testable modules. Define an explicit public package API, add basedpyright strict checking and Ruff gates, and preserve existing CLI output and BinBook bytes.

## Rust CLI and diagnostic protocol modularization

Split command models, serial transport, response formatting, exercise evidence, and protocol codecs into focused modules. Replace oversized test files and source-shape checks with public behavior and wire-format tests without changing protocol version 1.

## SquidScript Rust-native BinBook/display adoption

After SquidScript chooses its post-Zephyr firmware architecture, consume `binbook-core`, `binbook-decompress`, `gray2-render`, `ssd1677-driver`, and `xteink-x4-display` directly. Do not add a C ABI or compatibility facade before that architecture is selected.
