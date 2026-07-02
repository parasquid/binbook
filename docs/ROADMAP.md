# BinBook Roadmap

## X4 ADC input refactor

Status: evidence-gated.

Current firmware uses synchronous one-shot ADC reads, a 50 ms Embassy timer, and one global 100 ms cooldown. The candidate architecture uses `Adc::into_async()`, interrupt-completed `read_oneshot().await`, 20 ms periodic sampling, independent stable-candidate state for each ADC ladder, and 30 ms debounce matching the verified SquidScript/X4 reference.

ADC conversion completion can be interrupt-driven, but resistor-ladder button detection still requires periodic sampling; GPIO edges cannot distinguish ladder voltages reliably. Continuous ADC/DMA is not the default because it adds power and RAM complexity without removing debounce.

Do not implement this refactor until serial/camera stress plus physical input logs localize the problem to ADC sampling or debounce. Acceptance requires rapid mixed-direction host sequences, calibrated threshold tests, queue/drop evidence, pinned builds, flash, serial capture, and live physical-button confirmation.

The serial `page-turn-cadence` diagnostic measures the display/request pipeline with protocol KEY frames. It does not exercise ADC sampling, physical contact bounce, release detection, or the 100 ms physical-input cooldown. A physical-button cadence follow-up should capture `INPUT_TRANSITION`, `INPUT_DECISION`, `REQUEST_ENQUEUE`, `REQUEST_RECEIVE`, `PAGE_TURN`, and gray-overlay events while a human or actuator presses Down at controlled intervals, then compare the fastest accepted physical cadence against the serial cadence report.

## X4 page-turn latency optimization targets

Status: evidence-gated.

Current serial cadence diagnostics split queueing from display activation. `serial_accept_after_bw_busy_asserted` is the firmware-only proxy for "the next user key was accepted after the display activation started." `display_dequeue_after_bw_busy_asserted` is only a queue-drain metric and can pass while user keys are still arriving too early.

Baseline before SPI-clock testing used 20 MHz display SPI. A live serial cadence run measured firmware-side activation at `750 ms`, reliable completion/gray-safe cadence at `925 ms`, and `dequeue_to_bw_start` at about `394-396 ms`. That pre-refresh segment is dominated by two full-screen BW plane writes in `crates/xteink-x4-display/src/native.rs::render_bw_differential_timed`: previous page fast-base plane to red RAM, then target page fast-base plane to black RAM. Each plane write in `crates/xteink-x4-display/src/plane_write.rs` took about `196 ms` at 20 MHz, with about `122 ms` attributed to SPI transfer and about `19 ms` to row fill/decode.

40 MHz display SPI was tested by changing `firmware/crates/binbook-fw/src/main.rs::DISPLAY_SPI_FREQUENCY_MHZ` from `20` to `40` and flashing diagnostic firmware. The live run improved activation to `725 ms`, reliable completion to `900 ms`, and `dequeue_to_bw_start` to about `373-376 ms`. Per-plane SPI-write time dropped only from about `122 ms` to about `110 ms`, so future agents should not assume the SPI clock change halves transfer time. Keep 40 MHz only if repeated live runs and webcam verification remain stable.

Partial-window BUSY was tested with diagnostic firmware. The probe measured full screen `800x480`, half height `800x240`, wide stripe `800x64`, one strip `800x16`, centered box `200x100`, and page-number box `240x80`. All cases reported `472 ms` BUSY for all three samples. With the current SSD1677 partial-refresh command sequence, BUSY duration does not scale with the RAM window. Do not pursue sectioned refresh as a BUSY-reduction strategy unless a future custom waveform/update-control experiment proves different behavior.

Full-window plane streaming was implemented for non-cancellable full-plane writes. Absolute grayscale, BW differential page turns, and recovery seeding now set one full-screen RAM window per controller plane and stream all 480 rows in one RAM write command. Cancellable staged overlay and background base sync remain strip-based so they can still stop before activation. A live cadence run measured `dequeue_to_bw_start` at about `255-259 ms`, activation threshold at `525 ms`, reliable completion/gray-safe cadence at `775 ms`, and `BW_BUSY_READY` at about `501 ms`.

Remaining targets, in recommended order:

Before starting any target below, reproduce the current baseline with `binbook diag exercise page-turn-cadence`, `scripts/analyze_cadence.py`, and webcam verification where the target affects visible output. Keep accepted-key metrics separate from queued/dequeued metrics: `serial_accept_after_bw_busy_asserted` is the closest firmware-only proxy for "next key accepted after display activation started"; `display_dequeue_after_bw_busy_asserted` is only a queue-drain metric.

1. One-plane update investigation. Current BW turns write previous fast-base to one controller RAM plane and target fast-base to the other. Investigate whether the previous plane can be preserved or pre-seeded across normal sequential turns so only the target plane needs to be written. This could save about one full plane write, but it is high risk because stale controller RAM can produce wrong transitions or ghosting. Acceptance: explicit controller RAM state logs, alternating forward/backward turn tests, boundary turns, recovery after failed/cancelled renders, and webcam verification across at least 20 consecutive turns.

2. Dirty-region writes for SPI/decode reduction only. Compare the previous and target fast-base data and skip unchanged regions where both controller RAM targets would remain equivalent. The partial-window BUSY matrix shows this should not be expected to reduce the post-refresh BUSY section, and full-window streaming removed most strip command overhead, so this is only worthwhile if real books have large unchanged regions. Acceptance: fixture tests with unchanged, partially changed, and fully changed pages; hardware logs showing fewer `PLANE_SPI_WRITE_SUMMARY` bytes; visual verification for page sequences with large white margins and dense text.

3. Timestamp precision cleanup. `REQUEST_RECEIVE` currently samples `now_ms` before the select branch completes, so receipt-to-dequeue deltas can be slightly negative at high gaps. Move timestamp sampling closer to event emission or add a monotonic event-order correction in the analyzer. Acceptance: synthetic analyzer tests for signed deltas and live logs showing no unexplained negative dequeue deltas.

4. Custom LUT/update-control investigation. If reducing the BUSY section remains necessary, investigate shorter SSD1677 partial waveforms or update-control variants. Treat this as high risk: acceptance requires repeated hardware runs across temperature-relevant conditions, ghosting inspection by webcam, full cleanup recovery, and a documented rollback path.

Supporting work: row fill/decode optimization is not currently a primary target. Profile `PlaneDecoder::fill` and PackBits/LZ4 paths with realistic pages before changing buffer sizes or compression strategy. Acceptance requires before/after row-fill summaries from live logs and no increase in RAM footprint beyond documented constrained-device limits.

Do not optimize against `display_dequeue_after_bw_busy_asserted`; it measures queued work being processed later. Optimize against `serial_accept_after_bw_busy_asserted`, `dequeue_to_bw_start`, `PLANE_SPI_WRITE_SUMMARY`, `PLANE_ROW_FILL_SUMMARY`, `BW_BUSY_READY`, and webcam verification.

## Python authoring package modularization

Split binary format models/writing, EPUB ingestion, raster rendering, viewer, and kerning-proof server into independently testable modules. Define an explicit public package API, add basedpyright strict checking and Ruff gates, and preserve existing CLI output and BinBook bytes.

## SquidScript Rust-native BinBook/display adoption

After SquidScript chooses its post-Zephyr firmware architecture, consume `binbook-core`, `binbook-decompress`, `gray2-render`, `ssd1677-driver`, and `xteink-x4-display` directly. Do not add a C ABI or compatibility facade before that architecture is selected.

## Internal Flash LittleFS

After the SD card path is working, evaluate `littlefs2` on the existing 192 KB internal flash region for storing a small number of binbooks (tens of KB each). FAT is unsuitable for internal flash due to sector-erase wear, so this would use a wear-leveling filesystem (LittleFS) as a separate path.

- **Depends on:** completion of A (storage abstraction) — LittleFS would implement the same `Filesystem` trait from `binbook-storage`.
- **Hardware:** same internal flash already backing `FlashStorage` (0x40E90000, 192 KB).
- **Gate:** requires LittleFS on-device writes verified before adoption; read-only use of LittleFS is strictly out of scope.
