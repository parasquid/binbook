# Page-Turn Timing Breakdown Implementation Plan

> **For agentic workers:** Execute this plan directly and sequentially. Do not delegate implementation to subagents: `AGENTS.md` requires plans to be executed inline with a current todo tracker. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Break down Xteink X4 page-turn `display_request_ms` into page metadata, per-plane fill/decode, per-plane SPI transfer, refresh trigger, BUSY wait, non-BUSY, and unattributed timing buckets.

**Architecture:** Keep the diagnostic protocol frame shape unchanged and add only stable log events. The reusable display crate emits render-stage events through its existing `EventSink`; firmware maps those events to diagnostic logs; the host analyzer reconciles the new fields inside each existing `DISPLAY_REQUEST_START..DISPLAY_REQUEST_END` timeline.

**Tech Stack:** Rust workspace crates (`xteink-x4-display`, `ssd1677-driver`, `binbook-fw`, `binbook-diagnostic-protocol`, `binbook` CLI), Python 3.13 via `uv`, live Xteink X4 diagnostics on `/dev/ttyACM0` and webcam `/dev/video1` for completion evidence.

---

## Files and responsibilities

- `firmware/crates/binbook-diagnostic-protocol/src/lib.rs`: stable event constants for page metadata, plane writes, row fill summaries, SPI write summaries, and refresh trigger timing.
- `firmware/crates/binbook-diagnostic-protocol/tests/codec.rs`: event-code stability tests.
- `crates/binbook/src/diag_response.rs`: readable names for new events.
- `crates/xteink-x4-display/src/events.rs`: reusable render-stage event variants and integer code helpers for plane role, RAM target, and refresh mode/status.
- `crates/xteink-x4-display/src/native.rs`: timing capture around page metadata reads, plane writes, row fill/decode, and SPI writes.
- `crates/xteink-x4-display/src/panel.rs` and `crates/ssd1677-driver/src/refresh.rs`: observed refresh-trigger timing before the existing BUSY wait.
- `firmware/crates/binbook-fw/src/runtime_engine.rs`: map display render-stage events to firmware runtime events.
- `firmware/crates/binbook-fw/src/runtime_aggregator.rs`: map runtime events to diagnostic log records.
- `firmware/crates/binbook-fw/tests/runtime_aggregator_events.rs`: exact event mapping tests.
- `crates/xteink-x4-display/tests/streaming.rs`: fake-clock/fake-SPI tests for plane fill/SPI summary accounting.
- `scripts/analyze_timing.py`: compute and print new breakdown fields.
- `tests/test_timing_analysis.py`: analyzer tests for new and old logs.
- `HANDOFF.md`: current status and verification evidence.

## Task 1: Stable diagnostic event names

- [ ] Add failing protocol tests for new event constants.
- [ ] Add constants without changing protocol version, frame layout, or existing event numbers.
- [ ] Add CLI readable names.
- [ ] Verify with `cargo test -p binbook-diagnostic-protocol` and `cargo test -p binbook --features serial-device`.

## Task 2: Reusable display render-stage events

- [ ] Add typed display events for `PageMetadataRead`, `PlaneWriteStart`, `PlaneRowFillSummary`, `PlaneSpiWriteSummary`, `PlaneWriteEnd`, and `RefreshTrigger`.
- [ ] Add small code helpers for plane role, RAM target, refresh mode, and status.
- [ ] Keep defaults no-alloc and `no_std` compatible.
- [ ] Verify with `cargo test -p xteink-x4-display`.

## Task 3: Instrument BW differential render

- [ ] Add tests with fake time/SPI proving two plane writes emit total/fill/SPI summaries and bytes/rows match `480` rows and `96_000` total bytes.
- [ ] Time `read_x4_page` calls as `page_metadata_ms`.
- [ ] Time each `decoder.fill(...)` plus row copy as fill/decode time.
- [ ] Time each row `spi.write(&row)` as SPI write time.
- [ ] Emit one summary per plane, not per row.
- [ ] Verify with `cargo test -p xteink-x4-display`.

## Task 4: Instrument refresh trigger separately from BUSY wait

- [ ] Add/adjust SSD1677 refresh tests proving `REFRESH_TRIGGER` covers `trigger_refresh(...)` only and existing `BUSY_WAIT_*` still covers the ready wait.
- [ ] Thread the event through `X4Panel::refresh_observed` without altering refresh command order.
- [ ] Verify with `cargo test -p ssd1677-driver` and `cargo test -p xteink-x4-display`.

## Task 5: Firmware runtime/log mapping

- [ ] Add runtime event variants for the new display-stage events.
- [ ] Map `xteink-x4-display` events into firmware runtime events.
- [ ] Map runtime events into diagnostic log records with the spec payload contract.
- [ ] Verify with `cargo test -p binbook-fw --features diagnostic-console`.

## Task 6: Analyzer output

- [ ] Add analyzer tests for a full new timeline that prints page metadata, per-plane total/fill/SPI, refresh trigger, non-BUSY, and unattributed fields.
- [ ] Add analyzer test for older logs: existing metrics still work and missing new fields are reported as unavailable rather than fake zero.
- [ ] Implement parsing and reconciliation inside `scripts/analyze_timing.py`.
- [ ] Verify with `uv run pytest -q tests/test_timing_analysis.py`.

## Task 7: Full verification and hardware evidence

- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run `cargo test --workspace`.
- [ ] Run `cargo test -p binbook-fw --features diagnostic-console`.
- [ ] Run `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] Run `uv run pytest -q`.
- [ ] Build diagnostic firmware with `cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release`.
- [ ] Flash with `FW_FEATURES="firmware-bin,diagnostic-console" firmware/scripts/flash-xteink-x4-nav-probe.sh`.
- [ ] Capture serial logs, run `scripts/analyze_timing.py --capture --port /dev/ttyACM0 --since <cursor>`, verify breakdown reconciliation, query `diag status`, and capture `/dev/video1` evidence.
- [ ] Update `HANDOFF.md` with verified behavior, exact commands, exact relevant output, webcam path, caveats, and remaining optimization interpretation.
