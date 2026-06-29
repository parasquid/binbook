# Xteink X4 Fast-Turn Staged-Grayscale Design

## Purpose

Provide fast Xteink X4 page turns followed by four-level grayscale refinement
without a full-screen flash. BinBook compilation performs all per-pixel work.
Firmware selects, decompresses, and streams controller-ready planes without
rotation, dithering, thresholding, grayscale decomposition, or frame
comparison.

This design replaces the current deferred absolute-grayscale path. That path
resets the SSD1677, loads a long grayscale waveform, and activates update
control `0xC7`; its visible full refresh defeats the latency benefit of the
preceding black-and-white turn.

## Reference Behavior

CrossPoint's X4 reader path is the hardware reference:

- SDK commit: `198ad267219c25c8ab84418b806c66f1fb5216a3`
- Driver:
  <https://github.com/crosspoint-reader/community-sdk/blob/198ad267219c25c8ab84418b806c66f1fb5216a3/libs/display/EInkDisplay/src/EInkDisplay.cpp>
- Usage:
  <https://github.com/crosspoint-reader/community-sdk/tree/198ad267219c25c8ab84418b806c66f1fb5216a3/libs/display/EInkDisplay#rendering-greyscale-frames>

CrossPoint first displays every non-white pixel as black. It then writes two
grayscale overlay masks, loads a short 12-frame differential LUT, and activates
the custom LUT through the fast update path. Black and white use overlay state
`00`, which receives no grayscale drive. Only gray pixels visibly lighten.
There is a controller activation, but no whole-panel clear, inversion, or
full-refresh flash.

The LUT and command sequence are MIT-licensed reference material. Preserve an
attribution comment beside the firmware constant.

## Native BinBook Contract

The X4 `DISPLAY_PROFILE.waveform_hint` value is
`SSD1677_STAGED_GRAY2 = 2`. Readers must reject an X4 GRAY2 page when the hint
is absent, unknown, or inconsistent with the required planes. Compatibility
with previously generated candidate files is not required.

An X4 GRAY2 page stores three physical `800x480`, one-bit, SSD1677-native
planes. Each plane remains split into thirty 16-row chunks.

| Slot | RAM role | Meaning |
|---|---|---|
| 0 | RED (`0x26`) | grayscale overlay MSB mask |
| 1 | BW (`0x24`) | grayscale overlay LSB mask |
| 2 | BW/previous | fast black-base plane |

Canonical levels map as follows:

| GRAY2 | Visible target | Fast base | Slot 0 MSB | Slot 1 LSB |
|---:|---|---:|---:|---:|
| 0 | black | 0 | 0 | 0 |
| 1 | dark gray | 0 | 1 | 1 |
| 2 | light gray | 0 | 1 | 0 |
| 3 | white | 1 | 0 | 0 |

The fast base is not dithered. Every non-white pixel is black until refinement.
Bits in the overlay planes mean "apply the differential gray drive," not an
absolute final pixel value.

The compiler applies logical-to-physical rotation, controller X mapping, plane
construction, chunking, PackBits compression, CRC calculation, and adjacent
transition-mask generation. Transition masks compare slot 2 only. Firmware
does not calculate any pixel values.

## Controller Sequence

### Cold start and recovery

A hardware reset and SSD1677 software reset are allowed only for cold start or
safe recovery. Cold start establishes a known BW controller state, displays the
page's slot-2 base with a safe seed, and then uses the staged overlay path.

Runtime mode changes must not reset the controller. Resetting between the BW
base and overlay destroys the differential state that the short waveform
expects.

### Fast page turn

1. Ensure the prior page's slot-2 base is available as previous-frame RAM.
2. Stream the requested page's slot-2 base to BW/current RAM.
3. Trigger the SSD1677 differential partial update.
4. Record page completion only after BUSY clears.
5. Start a 350 ms idle deadline from observed BW completion.

The requested turn is complete at step 4. Grayscale is background refinement,
not part of page-turn latency.

### Grayscale refinement

When the 350 ms deadline expires without a queued turn:

1. Stream slot 1 to BW RAM and slot 0 to RED RAM in 16-row strips.
2. Load CrossPoint's exact `lut_grayscale` bytes: 105 LUT bytes followed by
   VGH, VSH1, VSH2, VSL, and VCOM values.
3. Set normal two-plane interpretation with display update control 1.
4. Activate with custom-LUT fast control `0x0C` when the controller clock and
   analog rails are already on. After a full seed powers them down, power them
   on in the same command with `0xCC`. Never use absolute/full control `0xC7`.
5. Wait for BUSY to clear.
6. Enter `GrayOverlayResident`: the visible page is valid grayscale, but
   controller RAM still contains overlay masks rather than a BW baseline.
7. Begin a cancellable background sync of the current slot-2 base to
   RED/previous RAM without activation.

The overlay operation must not issue hardware reset, software reset, a full
refresh command, or a grayscale-revert activation.

The explicit diagnostic `full-refresh-current` probe is separate from normal
navigation. Because slots 0 and 1 are differential masks, it reconstructs the
legacy absolute four-gray RAM planes while streaming, without a framebuffer:
`red = !(base | (msb & !lsb))` and `black = !(base | lsb)`. Directly feeding
staged masks to the absolute LUT is invalid.

### Controller RAM state

The runtime tracks controller state explicitly instead of reducing it to a
boolean readiness flag:

- `BwBaseReady`: RED/previous RAM contains the visible page's complete slot-2
  base and a normal full-window differential turn may proceed.
- `GrayOverlayResident`: staged activation completed and the visible page is
  valid grayscale, but overlay masks remain in controller RAM.
- `BaseSyncInProgress`: a no-activation slot-2 sync is being streamed.
- `NeedsFullBwInputs`: a sync was canceled or RAM contents are otherwise not a
  complete BW baseline. The next turn must stream both the current page's full
  slot-2 base to RED/previous RAM and the target page's full slot-2 base to
  BW/current RAM before partial activation. This is a normal path, not recovery.

Background base sync is an optimization, not a correctness requirement. A turn
must never wait for it. If sync finishes first, state becomes `BwBaseReady`. If
a turn arrives, cancel sync at the next 16-row boundary, enter
`NeedsFullBwInputs`, and let the turn overwrite both complete inputs.

## Cancellation and Queuing

Every successfully enqueued physical or protocol turn increments a request
epoch. The overlay captures the epoch at start and checks it between each
16-row strip and immediately before activation.

- A changed epoch before activation cancels refinement. Partially written
  controller RAM is not visible because no activation occurred. The next
  full-window BW differential operation overwrites both required inputs.
- Cancellation is a normal state transition, not a display error and not a
  dropped turn.
- Once activation is issued, cancellation is unsafe. Presses received while
  SSD1677 BUSY is asserted remain in the bounded FIFO and execute afterward.
- A request rejected because the queue is full does not increment the epoch.
- Background base sync uses the same epoch. Cancellation during sync does not
  change the visible grayscale page, emit a display error, or perform recovery.

## Failure and Recovery

Plane decode, SPI, command, and BUSY timeout errors propagate to the display
engine. The first failure performs one cold BW recovery seed for the requested
page. A failure during that recovery enters `Fault`. No page or differential
readiness state advances before its corresponding hardware operation succeeds.

Cancellation never invokes recovery. A canceled overlay leaves the visible BW
page valid and the next turn uses a full-window old/new BW write. A canceled
base sync leaves the visible grayscale page valid and enters
`NeedsFullBwInputs`.

## Waveform ownership and revision

BinBook stores the waveform family hint and pixel masks only. It never stores
LUT bytes or panel voltages. Firmware owns a named
`STAGED_GRAY_LUT_REVISION = 1` corresponding to the pinned CrossPoint LUT.
Structured diagnostics emit waveform hint `2` and LUT revision `1` before the
first staged activation. Temperature-specific LUT selection is outside this
implementation until device measurements demonstrate a need; the fixed
revision must not be described as temperature-calibrated.

## Diagnostics

Protocol version 1 and STATUS layout remain unchanged. Structured events must
distinguish:

- BW start and completion;
- gray-delay start and cancellation;
- overlay plane streaming start and cancellation;
- differential grayscale activation and completion;
- controller-state changes and cancellable background base sync;
- waveform hint and firmware LUT revision;
- recovery and unrecovered failure.

The event origin records device monotonic time. The aggregator commits each
event before forwarding any matching protocol completion.

## Hardware Acceptance

Hardware evidence uses the diagnostic serial console and a native
`1920x1080` webcam recording from `/dev/video1`. Crop the panel with
`crop=440:770:770:250`. The crop includes the black device bezel; it is not
rendered content.

Acceptance requires all of the following:

1. The fast base covers the complete active panel and initially maps black,
   dark gray, and light gray to black while preserving white.
2. Differential refinement begins at least 350 ms after BW completion.
3. Dark and light swatches separate into the intended four levels.
4. Black and white areas remain visually stable during refinement.
5. No full-panel clear, inversion, white flash, or black flash occurs.
6. A turn queued before activation cancels the pending refinement and begins
   the next BW turn.
7. A turn received after activation begins remains queued and executes after
   BUSY clears.
8. A turn received during background base sync cancels the sync and begins
   without waiting for the remaining rows.
9. STATUS and logs confirm the final page, FIFO order, waveform hint `2`, LUT
   revision `1`, zero dropped turns,
   zero protocol errors, and zero unrecovered display errors.

Serial acknowledgements alone are transport evidence. Webcam evidence is
mandatory for the visible criteria.
