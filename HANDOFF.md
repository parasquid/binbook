# Handoff: X4 Boundary Burst FIFO Fix

Date: 2026-06-30

## Current state

The boundary and mixed-direction protocol KEY fix is implemented and verified on the attached Xteink X4. The device is running `firmware-bin,diagnostic-console` and is visibly settled on page 1 in grayscale mode.

All 165 live KEY requests from the ten-round navigation diagnostic plus the five-key boundary burst produced unique sequence-matched engine completions. The boundary burst `[Up, Down, Up, Up, Down]` from page 0 completed as `0,1,0,0,1`; sequences 283 and 286 emitted `TURN_BOUNDARY_NOOP`, and independent STATUS reported page 1 with zero dropped logs, protocol errors, or display errors.

## Implementation

Directional protocol KEY presses no longer complete as dispatch-time no-ops. Every accepted press becomes a typed relative `PageTurn` and enters the existing 16-request FIFO. The display engine applies turns in FIFO order, so dequeue order is the queue-tail logical page model. Boundary turns reach the existing no-render observation/completion path and retain their protocol sequence.

This does not change ADC thresholds, 50 ms polling, 100 ms cooldown, physical button mapping, queue capacity, display coordination, SSD1677 behavior, protocol version, STATUS layout, or framebuffer/RAM usage.

## Host verification

- Regression red phase: `cargo test -p binbook-fw --features diagnostic-console --test boundary_fifo -- --nocapture` failed because sequences 100 and 77 returned immediate success instead of `RuntimeCommand::Hardware`.
- Regression green phase: the same command passes 2 tests.
- Focused protocol/engine/aggregator/transport/nav-burst suites pass.
- Clean diagnostic firmware workspace: 218 tests pass (`27 + 22 + 2 + 128 + 9 + 11 + 19` across nonempty suites).
- Default firmware workspace: 133 tests pass (`27 + 21 + 55 + 11 + 19` across nonempty suites).
- CLI default: 35 tests pass.
- CLI `serial-device`: 56 tests pass and 4 live tests remain explicitly ignored.
- Python: 98 tests pass and 26 are skipped.
- Pinned release binaries: default 1,084,436 bytes; diagnostic 1,101,692 bytes; diagnostic/debug 1,101,692 bytes.
- `git diff --check` passes. Protocol version remains `1`; STATUS layout and input/queue constants are unchanged; no serde dependency was added to firmware crates.

## Live device evidence

- Flash command: `FW_FEATURES="firmware-bin,diagnostic-console" firmware/scripts/flash-xteink-x4-nav-probe.sh`.
- Flash result: ESP32-C3 rev v0.4, 40 MHz, 16 MB flash, application 1,116,352 bytes, completed successfully.
- Boot capture command: the 15-second pyserial reset/capture from `AGENTS.md`.
- Boot record: `/tmp/x4-nav-burst-fifo-fix-boot.txt`.
- Baseline HELLO: protocol 1, frame limit 512, firmware `binbook-fw`, target `xteink-x4`, and all required capabilities.
- Baseline STATUS: page 0, page count 16, grayscale, all counters zero.
- Diagnostic command: `UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline python firmware/scripts/run-x4-nav-burst-diagnostic.py --port /dev/ttyACM0 --video-device /dev/video1 --rounds 10 --inter-key-ms 0 --output-dir /tmp/x4-nav-burst-fifo-fix`.
- JSONL: `/tmp/x4-nav-burst-fifo-fix/evidence.jsonl` contains 165 KEY records, 165 unique `TURN_DEQUEUED` sequence values, 11 successful round results, and `error_count=0`.
- Boundary no-ops: sequence 283 at page 0 and sequence 286 at page 0.
- Boundary completions: sequences 283 through 287 completed on pages `0,1,0,0,1`.
- Independent final STATUS: `/tmp/x4-nav-burst-fifo-fix/final-status.txt` reports `current_page=1 page_count=16 panel_mode=Grayscale dropped_log_count=0 protocol_error_count=0 last_error=0`.
- Independent boundary logs: `/tmp/x4-nav-burst-fifo-fix/final-boundary-log-page-1.txt` and `/tmp/x4-nav-burst-fifo-fix/final-boundary-log-page-2.txt`.
- Video: `/tmp/x4-nav-burst-fifo-fix/nav-burst.mp4`.
- Boundary contact sheet: `/tmp/x4-nav-burst-fifo-fix/round-11-contact-sheet.jpg`.
- Settled boundary frame: `/tmp/x4-nav-burst-fifo-fix/round-11-settled.jpg` visibly reads `PAGE 01` with the orientation frame and grayscale swatches intact.

The extracted per-key frames are host-timestamp samples from a zero-delay queued burst, not proof of exact transition timing. Some samples capture a later visible page while queued requests are still completing. Serial completion sequences and independent STATUS prove per-key logical outcomes; the settled webcam frame proves the required final visible page.

## Acceptance matrix

| Requirement | Implementation path | Automated evidence | Live serial evidence | Webcam evidence | State |
|---|---|---|---|---|---|
| Boundary burst models `0,1,0,0,1` | KEY dispatch to typed FIFO `PageTurn` | `boundary_fifo` regression | seq 283–287 complete `0,1,0,0,1` | settled `PAGE 01` | Verified |
| One completion per accepted KEY | runtime aggregator and display completion channel | boundary, aggregator, transport, scripted burst tests | 165 KEY records and 165 unique `TURN_DEQUEUED` sequences | final visible state agrees | Verified |
| Boundary no-ops use observation/completion path | display engine `TurnBoundaryNoop` path | engine no-op test plus dispatch regression | seq 283 and 286 emit `TURN_BOUNDARY_NOOP` and `TURN_DEQUEUED` | no extra visible page transition required | Verified |
| Boundary no-op performs no display operation | display engine returns before BW render | backend operation-count assertion | no `PAGE_TURN` for seq 283 or 286 | page remains 0 at those logical outcomes | Verified |
| FIFO-relative intent preserved | bounded request FIFO | modeled burst and queue-capacity tests | all 165 sequences match host model | settled pages 10 and 1 visible | Verified |
| No queue/counter regression | unchanged capacity and aggregator errors | full firmware/CLI/Python matrices | zero dropped logs, protocol errors, last error | no corruption in inspected contact sheets | Verified |
| Physical ADC semantics unchanged | existing input mapping/timing | default and diagnostic input tests | no physical ADC exercise in this run | not exercised | Source/test verified; live physical path pending |
| Display/SSD1677 behavior unchanged | no display/driver production changes | engine and SSD1677 suites | normal page events and grayscale settle | orientation frame and swatches intact | Verified |

Transport acknowledgements are not counted as completion evidence. Completion claims above require matching engine events, resulting state, independent STATUS/log queries, and the required settled webcam observation.

## Adversarial completion review

- Dispatch-bypass hypothesis: a boundary KEY could still return immediate `Ok`. Refuted by the red/green dispatch regression and live sequence 283 reaching both `TURN_BOUNDARY_NOOP` and `TURN_DEQUEUED`.
- FIFO-drift hypothesis: later relative requests could still resolve from stale committed state. Refuted by live sequences 283–287 completing exactly `0,1,0,0,1` and all 165 KEY sequences matching the host model.
- Misleading-success hypothesis: the runner could exit zero while device state remained wrong. Refuted by independent STATUS/log pagination after the runner exited and direct inspection of the fresh settled webcam frame showing `PAGE 01`.
