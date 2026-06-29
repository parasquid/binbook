# Handoff: X4 Staged-Grayscale Implementation

Date: 2026-06-29

## Current State

All implementation and agent-verifiable acceptance tasks in
[`docs/plans/2026-06-29-x4-staged-grayscale.md`](docs/plans/2026-06-29-x4-staged-grayscale.md)
have passed. The attached X4 is running the permanent
`firmware-bin,diagnostic-console` image on page 0 in grayscale mode. The only
outstanding acceptance input is the user's explicit verdict on the final webcam
artifacts.

The runtime uses BinBook waveform hint `SSD1677_STAGED_GRAY2 = 2`: slot 0 is
overlay MSB, slot 1 overlay LSB, and slot 2 the non-dithered BW base. Page turns
are differential BW, refinement starts after 350 ms, request epochs cancel
pre-activation streaming, and completed overlays perform cancellable background
base sync.

SSD1677 staged activation is power-state aware: `0x0C` when clock/analog power
is already on and `0xCC` after a full seed powers down. Explicit
`full-refresh-current` reconstructs absolute LUT planes without a framebuffer:
`red = !(base | (msb & !lsb))`, `black = !(base | lsb)`. The CLI permits 70
seconds for probes, covering firmware's 60-second BUSY bound plus streaming.

## Host Verification

Final clean matrix:

- `cargo clean`: removed 2,768 files / 526.0 MiB;
- firmware diagnostic workspace: 209 passed;
- firmware default workspace: 127 passed;
- pinned-nightly RISC-V release builds passed for `firmware-bin`,
  `firmware-bin,diagnostic-console`, and
  `firmware-bin,diagnostic-console,debug-log`;
- CLI default: 32 passed;
- CLI `serial-device`: 51 passed, 4 live tests ignored;
- Python: 93 passed, 26 skipped in 9.72 seconds;
- `git diff --check`: required after this documentation update.

Key regressions include power-state-safe `0xCC/0x0C` activation, absolute-plane
reconstruction from staged slots, canonical four-gray RAM polarity, true
black/white checker generation, deterministic overlay cancellation, and the
70-second display-probe timeout.

## Firmware And Boot Evidence

Final flash command:

```bash
FW_FEATURES="firmware-bin,diagnostic-console" \
  firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Flash succeeded on ESP32-C3 revision v0.4, 40 MHz crystal, 16 MB flash, MAC
`38:44:be:98:72:dc`; final application size is 263,328 bytes (1.61%). The
15-second boot record at `/tmp/x4-staged-gray-boot.txt` contains ESP-IDF v5.5.1,
the factory partition at `0x10000`, segment loads, and application load.

HELLO decodes as protocol 1, maximum frame 512, firmware `binbook-fw`, target
`xteink-x4`, and capabilities `KEY,PAGE,STATUS,LOG,CRASH,DISPLAY_PROBE`.
Immediate post-flash HELLO sometimes times out during USB re-enumeration; the
single runbook-authorized retry succeeds. Combined
`diagnostic-console,debug-log` also returns HELLO, proving packet transport owns
USB. The normal diagnostic image was reflashed afterward and HELLO reconfirmed.

## Staged Exercise Evidence

Final post-probe-fix exercise:
`/tmp/x4-staged-gray-post-probe-fix-exercise.txt`. Every validator phase passed
in 5.604 seconds. Earlier independently paginated logs established:

- page-1 BW completion→overlay start: 131604→131956 ms (352 ms);
- waveform hint 2 / LUT revision 1;
- page-2 overlay cancellation: 133049→133083 ms;
- FIFO completions exactly page `2,3,2`;
- page-2 and final page-3 overlay/base-sync completion;
- no dropped turns, display errors, or protocol errors.

After the final restore:

```text
current_page=0 page_count=4 panel_mode=Grayscale dropped_log_count=0 protocol_error_count=0 last_error=0
```

## General Device Runbook

- KEY shared navigation: independently verified `0→1→0`.
- PAGE: `goto 3`, `goto 0`, `next`, `previous`, `last`, `first`, and `current`
  returned `3,0,1,0,3,0,0`; STATUS confirmed discriminating states.
- LOG: a nonempty ring cleared at cursor 269; only the new retrieval receipt
  remained. A subsequent render produced fresh origin-timestamped phase,
  panel, controller-state, page-turn, and completion events from cursor 270.
- CRASH: clear returned `ok`; independent get returned `crash=empty`.
- Fragmented status, two-frame batch, and malformed-frame recovery tests passed
  individually with one test thread. Malformed tests intentionally raised
  `protocol_error_count` to 10 while valid STATUS still completed; reflashing
  reset it to zero.
- Full-refresh probe returned `ok`; isolated logs contained `RENDER_START`,
  `RENDER_SUCCESS`, sequence-matched completion, and zero-error STATUS.

## Webcam Evidence

Confirmed crop is `crop=440:770:770:250`; it includes the black device bezel,
which is not rendered content.

- staged exercise video: `/tmp/x4-staged-gray-final-panel.mp4`;
- four-corner probe: `/tmp/x4-probe-window-corners-panel.jpg`;
- clear-white probe: `/tmp/x4-probe-clear-white-panel.jpg`;
- corrected absolute full-refresh page 0:
  `/tmp/x4-probe-full-refresh-page0-polarity-panel.jpg`;
- final staged page 0: `/tmp/x4-final-page0-panel.jpg`.

Agent inspection confirms full active-panel coverage, correct orientation,
four corner rectangles, uniform white, four distinct grayscale swatches,
stable black/white regions, no full navigation refresh, pre-activation
cancellation, and artifact-free final content. The user still needs to provide
an explicit verdict on these final artifacts if required for sign-off.

## Acceptance Matrix

| Requirement | Automated evidence | Device / visual evidence | State |
|---|---|---|---|
| Staged planes and hint 2 | Python truth-table, parser, fixture tests | Waveform event `2/1` | Verified |
| Power-state-safe short LUT | Driver `0xCC/0x0C` tests | 143 ms boot overlay, no timeout | Verified |
| BW turn and ≥350 ms delay | Coordinator/engine tests | 352 ms measured | Verified |
| Pre-activation cancellation | Strip/epoch and CLI tests | 34 ms start→cancel | Verified |
| FIFO and base sync | Engine/CLI tests | Pages `2,3,2`; sync events | Verified |
| Stable full-panel base | True checker fixture test | Checker webcam sequence | Verified by agent |
| Four gray levels, no navigation full refresh | Reconstruction tests | Staged video and final still | Verified by agent |
| Explicit full-refresh probe | Absolute-plane and timeout tests | `ok`, `RENDER_SUCCESS`, corrected still | Verified |
| KEY/PAGE/LOG/CRASH/stream handling | Host and live console tests | Independent state queries | Verified |
| Combined-feature USB ownership | Feature builds | Combined-image HELLO | Verified |
| Permanent final image | Release build | Normal image, page 0, clean STATUS | Verified |
| User visual verdict | N/A | Final artifacts listed above | Pending user input |
