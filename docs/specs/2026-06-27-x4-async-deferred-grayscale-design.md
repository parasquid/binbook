# Xteink X4 Async Deferred-Grayscale Refresh Design

## Summary

This document records the current BinBook Xteink X4 refresh design before the
final hardware verdict. The firmware turns page requests into a bounded FIFO of
`PageTurn` values, renders the next BW page immediately, waits 350 ms of idle
time, then renders grayscale. Display work is split across async tasks so input
and diagnostics remain responsive while the SSD1677 is busy.

The current host-verified default keeps the conservative visible BW reseed as
the normal build behavior. A temporary `deferred-gray-probe` build switches the
post-gray strategy to silent reseed for the hardware experiment. Silent reseed
remains an unverified hardware hypothesis until Task 9 records the webcam
verdict.

## Fixed Constants

```rust
pub const PAGE_TURN_QUEUE_CAPACITY: usize = 16;
pub const DISPLAY_COMPLETION_CAPACITY: usize = 16;
pub const INPUT_POLL_INTERVAL_MS: u64 = 50;
pub const INPUT_COOLDOWN_MS: u32 = 100;
pub const GRAY_SETTLE_DELAY_MS: u64 = 350;
pub const DISPLAY_BUSY_TIMEOUT_MS: u64 = 60_000;
pub const DISPLAY_STREAM_STRIP_ROWS: u16 = 16;
pub const DISPLAY_SPI_FREQUENCY_MHZ: u32 = 20;
```

The display SPI clock uses the SSD1677 write limit of 20 MHz, not the older
4 MHz shared-bus ceiling.

## Architecture

- `input_task` owns the ADC ladder and GPIO button sampling.
- `display_task` owns SPI2, SSD1677 control pins, the refresh coordinator, and
  page streaming.
- `diagnostic_task` owns USB Serial/JTAG only when `diagnostic-console` is
  enabled.
- Input, display, and diagnostic work are concurrent. Button presses queue while
  grayscale work is pending; they are not dropped unless the bounded queue is
  full.

The reusable `ssd1677-driver` crate stays Embassy-independent. It exposes
command-layer primitives such as refresh triggering, BUSY checking, and async
wait wrappers. Firmware owns the async orchestration and policy.

## Refresh Phases

The refresh coordinator models these phases:

- `BwReady`
- `BwRefreshing`
- `GrayDelay`
- `GrayRefreshing`
- `BwReseeding`
- `Recovering`
- `Fault`

The state flow is:

1. Accept a queued turn and start BW rendering from the last completed page.
2. Record BW completion only when the rendered target matches the active turn.
3. Enter `GrayDelay` for 350 ms after BW completion.
4. If another turn arrives before the deadline, cancel the gray phase and start
   the next BW render.
5. If the queue stays empty, render grayscale.
6. After grayscale, reseed BW using the selected compile-time strategy.

Recovery is one safe retry after a display failure. A second failure while in the
recovery path enters `Fault`.

## Post-Gray Strategies

- `VisibleReseed` is the conservative build default.
- `SilentReseed` writes the current page's BW plane into both SSD1677 RAM planes
  without a visible refresh activation.

The silent strategy is the current experiment target, not a confirmed permanent
behavior. The user or webcam verdict in Task 9 chooses the final build strategy.
There is no runtime auto-detection and no persistent strategy setting.

## Deferred Gray Exercise

The CLI exposes `diag exercise deferred-gray` as the host-controlled exercise
for this flow. The exercise keeps one serial session open, records elapsed
timings for webcam correlation, and validates the following sequence:

1. page 0 baseline;
2. page 1 appears through a completed BW turn;
3. grayscale begins only after the idle deadline;
4. queued `RIGHT`, `RIGHT`, `LEFT` requests render pages `2`, `3`, `2` in FIFO
   order;
5. the final `RIGHT` renders page `3`;
6. `STATUS` and `LOG` confirm zero dropped turns, zero protocol errors, and
   zero nonrecoverable display errors.

This exercise is evidence gathering, not proof that the silent reseed strategy
is permanently safe.

## Hardware Gate

The Task 9 experiment build flashes:

```bash
FW_FEATURES="firmware-bin,diagnostic-console,deferred-gray-probe" \
  firmware/scripts/flash-xteink-x4-nav-probe.sh
```

The permanent strategy is selected only after:

- the 15-second boot capture is recorded;
- the autonomous deferred-gray exercise is visually verified by webcam;
- STATUS and LOG confirm the expected queue and reseed events;
- the final diagnostic image is reflashed and revalidated.

If the webcam verdict accepts silent reseed, the permanent default becomes
silent reseed and the probe feature is removed. If the verdict rejects the
silent path, the permanent default remains visible reseed and the experiment
feature is removed.

