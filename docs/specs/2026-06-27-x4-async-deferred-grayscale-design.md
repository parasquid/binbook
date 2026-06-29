# Xteink X4 Async Runtime Architecture

This document describes the shared async runtime architecture. The active
refresh design is
[`2026-06-29-x4-staged-grayscale-design.md`](2026-06-29-x4-staged-grayscale-design.md).

The firmware uses three concurrent Embassy tasks:

- `input_task` samples the ADC ladder and enqueues physical turns;
- `display_task` exclusively owns the SSD1677 and runs the host-testable
  `DisplayEngine`;
- `runtime_event_aggregator_task` exclusively owns diagnostic state, logs, and
  pending protocol reservations when `diagnostic-console` is enabled.

The request channel holds 16 entries. Runtime events use a separate 32-entry
channel. A physical or protocol request increments the request epoch only after
its channel enqueue succeeds. Duplicate sequences, pending-capacity failures,
and full request queues do not change the epoch.

The current refresh sequence is:

1. cold-reset once and safely seed page 0's non-dithered slot-2 BW base;
2. perform each requested page turn as a fast differential BW update;
3. wait 350 ms from observed BUSY completion;
4. stream the two staged overlay masks and activate firmware LUT revision 1
   with update control `0x0C`;
5. synchronize the current slot-2 base to previous-frame RAM in the background
   without activation.

The controller state is explicit: `BwBaseReady`, `GrayOverlayResident`,
`BaseSyncInProgress`, or `NeedsFullBwInputs`. A queued request cancels overlay
streaming before activation or cancels background base sync at a 16-row
boundary. A request received after activation remains queued until BUSY clears.
Cancellation is not an error and never invokes recovery.

Normal staged refinement does not reset the controller, use absolute grayscale
control `0xC7`, or perform a visible reseed. Hardware reset and full refresh are
reserved for cold start, explicit probes, and one safe recovery attempt.

The diagnostic CLI command is:

```bash
cargo run --features serial-device -- diag exercise staged-gray --port /dev/ttyACM0
```

It validates the 350 ms delay, waveform hint `2`, LUT revision `1`, ordered
overlay events, pre-activation cancellation, background base sync, FIFO page
completions, final page 3, and zero drop/protocol/unrecovered-display errors.
Serial evidence does not replace the calibrated webcam gate.
