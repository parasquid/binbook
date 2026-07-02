# Handoff: Timing Instrumentation And Non-SD Button Navigation

Date: 2026-07-02
Active plan: `docs/plans/2026-07-02-timing-instrumentation-plan.md`
Current task: Timing instrumentation implementation plus non-SD firmware startup button-navigation fix

## Current Status

Implemented timing instrumentation across the diagnostic protocol, firmware runtime event pipeline, SSD1677 busy-wait seam, Xteink X4 display calls, BinBook CLI log names, and host timing analyzer. Also added page-turn breakdown instrumentation for the non-BUSY portion of `display_request_ms`, splitting page metadata, previous/target fast-base plane fill/decode, previous/target SPI transfer, refresh trigger, non-BUSY, and unattributed time. The analyzer/display timing code was split into focused modules after post-write size review. Non-SD diagnostic firmware startup maps physical Up/Down as page turns immediately instead of empty library-menu navigation.

## Implemented Behavior

| Requirement | Implementation path | Verification evidence |
| --- | --- | --- |
| Stable timing event codes | `firmware/crates/binbook-diagnostic-protocol/src/lib.rs` | `cargo test -p binbook-diagnostic-protocol` passed |
| Readable CLI event names | `crates/binbook/src/diag_response.rs` | `cargo test -p binbook --features serial-device` passed |
| Request receive/start/end events | `firmware/crates/binbook-fw/src/runtime/display_task.rs`, `runtime_engine.rs`, `runtime_aggregator.rs` | `cargo test -p binbook-fw --features diagnostic-console` passed |
| Input enqueue/drop timing | `firmware/crates/binbook-fw/src/runtime/input_task.rs` | `runtime_aggregator_events.rs` tests passed |
| SSD1677 busy-wait observer | `crates/ssd1677-driver/src/wait.rs`, `refresh.rs` | `cargo test -p ssd1677-driver` passed; regression verifies ready-after-delay reports accumulated elapsed poll time |
| Observer plumbing through X4 display | `crates/xteink-x4-display/src/panel.rs`, `native.rs`, `render.rs`, `probes.rs` | `cargo test -p xteink-x4-display` passed |
| Firmware hardware observer bridge | `firmware/crates/binbook-fw/src/runtime/display_backend.rs` | Firmware target and diagnostic builds passed |
| Host timing analyzer | `scripts/analyze_timing.py`, `scripts/timing_breakdown.py`, `scripts/timing_cli.py`, `scripts/timing_report.py`, `tests/test_timing_analysis.py` | `uv run pytest -q tests/test_timing_analysis.py` passed; live capture produced a timeline |
| Page-turn non-BUSY breakdown | `crates/xteink-x4-display/src/native.rs`, `crates/xteink-x4-display/src/plane_write.rs`, `crates/xteink-x4-display/src/render_timing.rs`, `crates/xteink-x4-display/src/render.rs`, `firmware/crates/binbook-fw/src/runtime/display_backend.rs`, `firmware/crates/binbook-fw/src/runtime/render_timing.rs`, `scripts/analyze_timing.py` | Live `diag page next` produced metadata/plane-fill/plane-SPI/refresh-trigger fields and analyzer reconciliation |
| Non-SD startup Up/Down page turns | `firmware/crates/binbook-fw/src/async_refresh.rs`, `firmware/crates/binbook-fw/src/runtime.rs` | RED: `startup_down_maps_to_next_turn` failed with `Some(MenuNext)` vs expected `Some(Turn { turn: Next })`; GREEN: `startup_up_and_down_map_to_page_turns_without_library_menu` passed; live diagnostic key `DOWN` moved `0 -> 1`, `UP` moved `1 -> 0` |

## Verification Evidence

### Host gates that passed

```bash
cargo test -p binbook-diagnostic-protocol
cargo test -p ssd1677-driver
cargo test -p xteink-x4-display
cargo test -p binbook-fw
cargo test -p binbook-fw --features diagnostic-console
cargo test -p binbook --features serial-device
cargo test -p binbook-storage
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
uv run pytest -q
uv run pytest -q tests/test_timing_analysis.py
RUSTC="$(rustup which --toolchain stable rustc)" rustup run stable cargo check -p ssd1677-driver --no-default-features --target riscv32imc-unknown-none-elf
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
```

### Current host gate status

`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p binbook-fw --features diagnostic-console`, `cargo test -p xteink-x4-display`, `cargo test -p binbook --features serial-device`, `cargo test -p binbook-diagnostic-protocol`, `cargo test --workspace`, `uv run pytest -q`, and `uv run pytest -q tests/test_timing_analysis.py` pass on the current workspace. `lsp_diagnostics` is clean for the changed Python files and changed display modules; rust-analyzer reports only pre-existing `unlinked-file` hints for firmware runtime submodules while cargo builds/tests include them.

### Live hardware evidence

Latest page-turn breakdown diagnostic firmware flash:

```bash
FW_FEATURES="firmware-bin,diagnostic-console" firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Relevant output:

```text
Serial port: '/dev/ttyACM0'
Chip type:         esp32c3 (revision v0.4)
Crystal frequency: 40 MHz
Flash size:        16MB
Features:          WiFi, BLE
App/part. size:    1,139,920/16,384,000 bytes, 6.96%
Flashing has completed!
```

Captured 15-second boot serial after flashing. Relevant output:

```text
ESP-IDF v5.5.1-838-gd66ebb86d2e 2nd stage bootloader
SPI Speed      : 40MHz
SPI Mode       : DIO
SPI Flash Size : 16MB
Loaded app from partition at offset 0x10000
Disabling RNG early entropy source...
```

Live diagnostic page-turn sequence:

```bash
cargo run -p binbook --features serial-device -- diag hello --port /dev/ttyACM0
cargo run -p binbook --features serial-device -- diag status --port /dev/ttyACM0
cargo run -p binbook --features serial-device -- diag logs --port /dev/ttyACM0 --clear
cargo run -p binbook --features serial-device -- diag page --port /dev/ttyACM0 next
cargo run -p binbook --features serial-device -- diag status --port /dev/ttyACM0
cargo run -p binbook --features serial-device -- diag logs --port /dev/ttyACM0 --since 27 > /tmp/binbook-page-turn-breakdown-complete.log
cargo run -p binbook --features serial-device -- diag logs --port /dev/ttyACM0 --since 47 >> /tmp/binbook-page-turn-breakdown-complete.log
uv run python scripts/analyze_timing.py --log-text /tmp/binbook-page-turn-breakdown-complete.log
```

Relevant output:

```text
protocol=2 max_frame=4126 capabilities=KEY,PAGE,STATUS,LOG,CRASH,DISPLAY_PROBE,STORAGE firmware=binbook-fw target=xteink-x4
current_page=0 page_count=16 panel_mode=Grayscale dropped_log_count=0 protocol_error_count=0 last_error=0
next_cursor=27 dropped_log_count=0 record_count=0
diag page next: current_page=1
current_page=1 page_count=16 panel_mode=Bw dropped_log_count=0 protocol_error_count=0 last_error=0
turn=1 input_to_enqueue_ms=0 enqueue_to_receive_ms=0 receive_to_display_start_ms=9 display_request_ms=904 busy_wait_ms=473 page_metadata_ms=1 prev_plane_total_ms=203 prev_plane_fill_ms=29 prev_plane_spi_ms=119 target_plane_total_ms=196 target_plane_fill_ms=23 target_plane_spi_ms=119 refresh_trigger_ms=1 non_busy_ms=431 unattributed_ms=30 input_to_page_ms=903 bottleneck=display_request
summary count=1 min=903 max=903 avg=903 p95=903
```

Interpretation for optimization: the observed non-BUSY time was `431 ms`, dominated by plane writes (`203 ms + 196 ms`). Within those writes, SPI/write overhead was `119 ms + 119 ms`, and row fill/decode was `29 ms + 23 ms`; the remaining plane overhead is window setup, commands, GPIO/SPI transaction overhead, async strip yields, and event/log overhead. `busy_wait_ms=473` remains panel/controller waveform time and is not a CPU/SPI optimization target unless refresh mode, LUT, update region, or quality tradeoffs change.

Post-refactor live recapture on the same device used the split analyzer/display modules and produced the same breakdown shape:

```bash
FW_FEATURES="firmware-bin,diagnostic-console" firmware/scripts/flash-xteink-x4-nav-probe.sh
cargo run -p binbook --features serial-device -- diag hello --port /dev/ttyACM0
cargo run -p binbook --features serial-device -- diag status --port /dev/ttyACM0
cargo run -p binbook --features serial-device -- diag page --port /dev/ttyACM0 next
cargo run -p binbook --features serial-device -- diag logs --port /dev/ttyACM0 --since 20 > /tmp/binbook-page-turn-breakdown-refactor-logs.txt
cargo run -p binbook --features serial-device -- diag logs --port /dev/ttyACM0 --since 40 >> /tmp/binbook-page-turn-breakdown-refactor-logs.txt
uv run python scripts/analyze_timing.py --log-text /tmp/binbook-page-turn-breakdown-refactor-logs.txt
```

Relevant output:

```text
Chip type:         esp32c3 (revision v0.4)
Flash size:        16MB
App/part. size:    1,139,968/16,384,000 bytes, 6.96%
Flashing has completed!
protocol=2 max_frame=4126 capabilities=KEY,PAGE,STATUS,LOG,CRASH,DISPLAY_PROBE,STORAGE firmware=binbook-fw target=xteink-x4
current_page=0 page_count=16 panel_mode=Grayscale dropped_log_count=0 protocol_error_count=0 last_error=0
diag page next: current_page=1
current_page=1 page_count=16 panel_mode=Grayscale dropped_log_count=0 protocol_error_count=0 last_error=0
turn=1 input_to_enqueue_ms=0 enqueue_to_receive_ms=0 receive_to_display_start_ms=3 display_request_ms=903 busy_wait_ms=473 page_metadata_ms=0 prev_plane_total_ms=204 prev_plane_fill_ms=21 prev_plane_spi_ms=127 target_plane_total_ms=195 target_plane_fill_ms=26 target_plane_spi_ms=115 refresh_trigger_ms=1 non_busy_ms=430 unattributed_ms=30 input_to_page_ms=903 bottleneck=display_request
summary count=1 min=903 max=903 avg=903 p95=903
```

Fresh webcam evidence from `/dev/video1`:

```bash
ffmpeg -hide_banner -loglevel error -f video4linux2 -i /dev/video1 -frames:v 1 /tmp/binbook-page-turn-breakdown-webcam.jpg
```

Evidence path: `/tmp/binbook-page-turn-breakdown-webcam.jpg`

Observed image content: the Xteink X4 display is visible and shows `PAGE 01` with the orientation/calibration frame (`TL`, `TR`, `BL`, `BR`, edge labels, grid, and checker/stripe page pattern). This matches the independently queried `current_page=1` state after `diag page next`.

Flashed diagnostic firmware for the timing and non-SD button-navigation fixes:

```bash
FW_FEATURES="firmware-bin,diagnostic-console" firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Relevant output:

```text
Serial port: '/dev/ttyACM0'
Chip type:         esp32c3 (revision v0.4)
Crystal frequency: 40 MHz
Flash size:        16MB
Features:          WiFi, BLE
App/part. size:    1,136,752/16,384,000 bytes, 6.94%
Flashing has completed!
```

Latest non-SD button-navigation fix flash output:

```text
Serial port: '/dev/ttyACM0'
Chip type:         esp32c3 (revision v0.4)
Crystal frequency: 40 MHz
Flash size:        16MB
Features:          WiFi, BLE
App/part. size:    1,136,704/16,384,000 bytes, 6.94%
Flashing has completed!
```

The first immediate `diag status` attempt after that flash timed out while the
device was still rebooting. A fresh 15-second boot capture then showed the app
loaded, and the following HELLO/STATUS/DOWN/UP sequence succeeded.

Captured 15-second boot serial with the pyserial command from `AGENTS.md`. Relevant output:

```text
ESP-IDF v5.5.1-838-gd66ebb86d2e 2nd stage bootloader
SPI Speed      : 40MHz
SPI Mode       : DIO
SPI Flash Size : 16MB
Loaded app from partition at offset 0x10000
Disabling RNG early entropy source...
```

Verified diagnostic HELLO:

```bash
cargo run -p binbook --features serial-device -- diag hello --port /dev/ttyACM0
```

Output from the pre-fix hardware capture:

```text
protocol=2 max_frame=4126 capabilities=KEY,PAGE,STATUS,LOG,CRASH,DISPLAY_PROBE,STORAGE firmware=binbook-fw target=xteink-x4
```

Verified baseline STATUS:

```bash
cargo run -p binbook --features serial-device -- diag status --port /dev/ttyACM0
```

Output:

```text
current_page=0 page_count=16 panel_mode=Grayscale dropped_log_count=0 protocol_error_count=0 last_error=0
```

Generated a diagnostic page turn:

```bash
cargo run -p binbook --features serial-device -- diag page --port /dev/ttyACM0 next
```

Output:

```text
current_page=1
```

Follow-up STATUS independently confirmed state:

```text
current_page=1 page_count=16 panel_mode=Grayscale dropped_log_count=0 protocol_error_count=0 last_error=0
```

Captured timing logs from cursor 20:

```text
seq=27 tick_ms=42965 level=2 subsystem=3 event=REQUEST_RECEIVE arg0=1 arg1=1 arg2=-1
seq=28 tick_ms=42965 level=2 subsystem=1 event=DISPLAY_REQUEST_START arg0=1 arg1=0 arg2=1
seq=31 tick_ms=43357 level=1 subsystem=1 event=BUSY_WAIT_START arg0=2 arg1=60000 arg2=1
seq=32 tick_ms=43858 level=1 subsystem=1 event=BUSY_WAIT_END arg0=2 arg1=1 arg2=0
seq=34 tick_ms=43858 level=2 subsystem=3 event=PAGE_TURN arg0=0 arg1=1 arg2=0
seq=37 tick_ms=43859 level=2 subsystem=1 event=DISPLAY_REQUEST_END arg0=1 arg1=894 arg2=0
```

Ran timing analyzer on live captured logs:

```bash
uv run python scripts/analyze_timing.py --capture --port /dev/ttyACM0 --since 20
```

Output:

```text
turn=1 input_to_enqueue_ms=0 enqueue_to_receive_ms=0 receive_to_display_start_ms=0 display_request_ms=894 busy_wait_ms=1 input_to_page_ms=893 bottleneck=display_request
summary count=1 min=893 max=893 avg=893 p95=893
```

The `busy_wait_ms=1` value came from the original SSD1677 observer reporting the final poll interval on the ready path rather than accumulated elapsed polling time.

Post-fix live recapture on the same device produced:

```text
seq=31 tick_ms=55649 level=1 subsystem=1 event=BUSY_WAIT_START arg0=2 arg1=60000 arg2=1
seq=32 tick_ms=56150 level=1 subsystem=1 event=BUSY_WAIT_END arg0=2 arg1=474 arg2=0
```

Post-fix analyzer output:

```text
turn=1 input_to_enqueue_ms=0 enqueue_to_receive_ms=0 receive_to_display_start_ms=3 display_request_ms=893 busy_wait_ms=474 input_to_page_ms=893 bottleneck=display_request
summary count=1 min=893 max=893 avg=893 p95=893
```

After the non-SD startup-mode fix, live serial diagnostics verified the shared Up/Down navigation path on the flashed device:

```bash
cargo run -p binbook --features serial-device -- diag hello --port /dev/ttyACM0
cargo run -p binbook --features serial-device -- diag status --port /dev/ttyACM0
cargo run -p binbook --features serial-device -- diag logs --port /dev/ttyACM0 --clear
cargo run -p binbook --features serial-device -- diag key --port /dev/ttyACM0 DOWN
cargo run -p binbook --features serial-device -- diag status --port /dev/ttyACM0
cargo run -p binbook --features serial-device -- diag key --port /dev/ttyACM0 UP
cargo run -p binbook --features serial-device -- diag status --port /dev/ttyACM0
cargo run -p binbook --features serial-device -- diag page --port /dev/ttyACM0 next
cargo run -p binbook --features serial-device -- diag status --port /dev/ttyACM0
cargo run -p binbook --features serial-device -- diag logs --port /dev/ttyACM0 --since 27
```

Relevant output:

```text
protocol=2 max_frame=4126 capabilities=KEY,PAGE,STATUS,LOG,CRASH,DISPLAY_PROBE,STORAGE firmware=binbook-fw target=xteink-x4
current_page=0 page_count=16 panel_mode=Grayscale dropped_log_count=0 protocol_error_count=0 last_error=0
next_cursor=27 dropped_log_count=0 record_count=0
diag key DOWN: ok
current_page=1 page_count=16 panel_mode=Bw dropped_log_count=0 protocol_error_count=0 last_error=0
diag key UP: ok
current_page=0 page_count=16 panel_mode=Bw dropped_log_count=0 protocol_error_count=0 last_error=0
diag page next: current_page=1
current_page=1 page_count=16 panel_mode=Bw dropped_log_count=0 protocol_error_count=0 last_error=0
seq=29 tick_ms=74436 level=2 subsystem=2 event=TURN_QUEUED arg0=1 arg1=1 arg2=0
seq=36 tick_ms=75335 level=2 subsystem=3 event=PAGE_TURN arg0=0 arg1=1 arg2=0
seq=38 tick_ms=75335 level=2 subsystem=2 event=TURN_DEQUEUED arg0=1 arg1=1 arg2=0
```

Captured webcam evidence from `/dev/video1`:

```bash
mkdir -p "/tmp/binbook-x4-evidence" && ffmpeg -y -f v4l2 -i /dev/video1 -frames:v 1 "/tmp/binbook-x4-evidence/x4-display-page1.jpg"
```

Evidence path: `/tmp/binbook-x4-evidence/x4-display-page1.jpg`

Observed image content: the Xteink X4 display is visible and shows `PAGE 01` with the orientation/calibration frame (`TL`, `TR`, `BL`, `BR`, edge labels, and checker/stripe pattern). This matches the independently queried device state after `diag page next`.

## Caveats

- The live analyzer evidence used a diagnostic `page next` command, so `REQUEST_ENQUEUE` and `INPUT_DECISION` are not present. The analyzer uses the matching `CMD_RECEIPT` as the origin for diagnostic-command timelines. Physical button timing remains covered by host tests and should be captured separately if physical-input latency is required.
- The non-SD button-navigation fix is verified through host ADC/button mapping tests, the flashed firmware image, and live diagnostic key/status/log page-turn evidence. Direct physical Up/Down button actuation was not observed by this agent because no actuator/user press was available during the run; to close that last hardware-surface gap, press physical Down then Up and query `diag status`/`diag logs --since <cursor>`.
- `REQUEST_RECEIVE.arg2` is `-1` for queue age in the live diagnostic path because no enqueue timestamp is carried through the request object. This is intentional; the implementation does not fabricate timing data.
- The first listed live timing capture predates the busy-wait elapsed-time fix. Use the post-fix recapture above for hardware `busy_wait_ms` decisions.
- Visual webcam evidence was captured for the `PAGE 01` state at `/tmp/binbook-x4-evidence/x4-display-page1.jpg`. Full probe-specific visual verification (`window-corners`, `clear-white`, `full-refresh-current`) was not run because the timing instrumentation acceptance was verified through serial/state/log timing evidence and the page-turn display image.

## Files Modified For Timing Work

- `firmware/crates/binbook-diagnostic-protocol/src/lib.rs`
- `firmware/crates/binbook-diagnostic-protocol/tests/codec.rs`
- `crates/binbook/src/diag_response.rs`
- `firmware/crates/binbook-fw/Cargo.toml`
- `firmware/crates/binbook-fw/src/diag_log.rs`
- `firmware/crates/binbook-fw/src/runtime_engine.rs`
- `firmware/crates/binbook-fw/src/runtime_aggregator.rs`
- `firmware/crates/binbook-fw/src/runtime/input_task.rs`
- `firmware/crates/binbook-fw/src/runtime/display_task.rs`
- `firmware/crates/binbook-fw/src/runtime/display_backend.rs`
- `firmware/crates/binbook-fw/tests/runtime_aggregator_events.rs`
- `crates/ssd1677-driver/src/wait.rs`
- `crates/ssd1677-driver/src/refresh.rs`
- `crates/ssd1677-driver/src/lib.rs`
- `crates/ssd1677-driver/tests/async_wait.rs`
- `crates/xteink-x4-display/src/panel.rs`
- `crates/xteink-x4-display/src/native.rs`
- `crates/xteink-x4-display/src/plane_write.rs`
- `crates/xteink-x4-display/src/render_timing.rs`
- `crates/xteink-x4-display/src/render.rs`
- `crates/xteink-x4-display/src/probes.rs`
- `scripts/analyze_timing.py`
- `scripts/timing_breakdown.py`
- `scripts/timing_cli.py`
- `scripts/timing_report.py`
- `scripts/__init__.py`
- `tests/test_timing_analysis.py`
- `docs/specs/2026-07-02-page-turn-timing-breakdown-design.md`
- `docs/plans/2026-07-02-page-turn-timing-breakdown-plan.md`
- `firmware/crates/binbook-fw/src/async_refresh.rs`
- `firmware/crates/binbook-fw/src/async_refresh_tests.rs`
- `firmware/crates/binbook-fw/src/runtime.rs`
- `firmware/crates/binbook-fw/src/runtime/render_timing.rs`

## Opportunistic Lint Fixes Made While Running Required Gates

- `crates/binbook-storage/src/read_at.rs`: replaced redundant closures with `FsReadError::Backend` to satisfy workspace clippy.
- `firmware/crates/binbook-fw/src/menu.rs`: replaced lint-equivalent expressions with `saturating_sub`, removed an unnecessary cast, and simplified an identity multiplication.
- `crates/ssd1677-driver/src/wait.rs`: replaced the explicit busy-wait elapsed counter with an iterator-derived elapsed value to satisfy `clippy::explicit-counter-loop` without changing observer output.

## Recommended Next Step

If more hardware evidence is required, run physical-button page turns or the full navigation burst diagnostic and feed the resulting logs to `scripts/analyze_timing.py`.
