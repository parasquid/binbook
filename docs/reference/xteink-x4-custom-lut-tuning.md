# Xteink X4 Custom LUT Tuning

This reference captures how BinBook currently thinks about SSD1677 custom LUT
tuning on the Xteink X4. It is intended to be easy for future agents to find
when candidate names, waveform bytes, or tuning policy change.

Search keywords: Xteink X4, SSD1677, custom LUT, waveform tuning, LUT tail,
tail bytes, preview LUT, cleanup LUT, ghosting, BUSY time, page-turn latency.

Current source locations:

- `crates/xteink-x4-display/src/panel.rs` owns firmware waveform byte arrays,
  candidate enum labels, voltage bytes, and waveform revision constants.
- `crates/xteink-x4-display/src/native.rs` chooses which candidate is used for
  preview and cleanup refreshes.
- `crates/xteink-x4-display/tests/streaming.rs` verifies the loaded LUT bytes
  and refresh command sequence.
- `firmware/crates/binbook-fw/src/runtime/display_backend.rs` wires hardware
  BUSY observers and runtime metrics to the display crate.

Primary external reference:

- Solomon Systech SSD1677 Rev 1.0 datasheet:
  <https://cursedhardware.github.io/epd-driver-ic/SSD1677.pdf>
- Cite this datasheet by section and PDF page. Scraped text offsets are tool
  artifacts and are not stable.

## Datasheet Anchors

The SSD1677 datasheet confirms several details that matter when tuning:

- Section 6.6, PDF page 15: the controller supports a programmable waveform
  with 40 phases, organized as 10 groups with 4 phases per group. Each phase
  has a frame count, and each group has a repeat count. This is why changing
  one byte can affect both visible strength and BUSY time.
- The 112-byte waveform setting is split into 105 LUT bytes, gate voltage,
  three source-voltage bytes, VCOM voltage, and two reserved bytes; see section
  6.7, PDF page 17.
- The LUT bytes contain source-voltage selection fields, phase frame-count
  fields, repeat-count fields, and frame-rate fields; see section 6.7, PDF
  page 17.
- `0x21` controls how red RAM and black RAM are interpreted during display
  update. Our preview flow writes previous-page fast-base data into red RAM and
  target-page fast-base data into black RAM, then leaves both RAM options in
  normal mode for staged activation; see the command table, PDF page 28.
- `0x22` selects the display update sequence used by `0x20` master activation.
  During master activation the BUSY pin goes high, and commands should not be
  sent until the operation completes; see the command table, PDF pages 27-28.

The current driver behavior mirrors these anchors: it writes the waveform with
`0x32`, sends `0x21`, sends `0x22`, then sends `0x20` and waits for BUSY to
clear.

## Page Refresh Modes

BinBook uses a high-level page refresh mode for reader behavior. This is
separate from `ssd1677_driver::RefreshMode`, which names low-level SSD1677
update-control commands such as full, partial, grayscale, and staged grayscale.

| User-facing mode | Page-turn policy |
| --- | --- |
| Balanced | Former page-turn behavior: differential BW page turn, then delayed staged grayscale refinement. |
| Fast | Responsive preview-first behavior: accepted fast preview LUT, accepted cleanup LUT, then delayed staged grayscale refinement. |

The firmware boots in Balanced mode. Build with `fast-refresh-mode` to include
the Fast runtime path; `preview-lut-turns` remains as a compatibility alias for
that Fast support. At runtime, diagnostics can query or switch modes:

```bash
binbook diag refresh-mode --port $PORT get
binbook diag refresh-mode --port $PORT set balanced
binbook diag refresh-mode --port $PORT set fast
```

The SSD1677 controller is not powered down after every render. In the current
driver, `RefreshMode::Full` records the controller as powered down after
activation. Partial, grayscale, and staged-grayscale activations leave it
powered, which is what the Balanced and Fast page-turn paths use during normal
navigation.

## Naming Policy

Diagnostic candidate names such as `FastA` and `FastB` are working labels.
They are not product vocabulary. Accepted regular-use recipes are named by
role and version, such as `PreviewFastV1` and `CleanupStrongV1`, and are used
by the Fast page refresh mode.

When updating names, keep these findability strings somewhere in the new text:
`custom LUT`, `LUT tail`, `preview`, `cleanup`, `ghosting`, and `BUSY`.

## LUT Layout

The display crate stores each firmware-owned SSD1677 waveform as a 112-byte
array:

- bytes `0..105`: waveform LUT bytes written with SSD1677 command `0x32`;
- byte `105`: gate voltage;
- bytes `106`, `107`, and `108`: source voltage bytes;
- byte `109`: VCOM voltage;
- bytes `110..112`: unused padding in the firmware array.

In current tuning notes, "tail bytes" means LUT offsets `100..104`, the last
five bytes of the 105-byte waveform payload. This is different from the voltage
tail at bytes `105..109`. The voltage bytes are treated as panel-risky and are
kept unchanged unless a tuning plan explicitly scopes voltage experiments.

The 105 LUT bytes are not a single linear table of brightness values. They are
packed waveform settings. The datasheet describes these fields at a high level:

| Field type | Practical meaning for tuning |
| --- | --- |
| `VS` source-voltage selection | Chooses which source/VCOM voltage state a phase applies. This affects particle movement direction and strength. |
| `TP` phase frame count | Controls how many frames a phase lasts. Longer phases usually increase drive time and can increase BUSY. |
| `RP` repeat count | Repeats a four-phase group. More repeats generally mean stronger/more complete movement and longer BUSY. |
| Frame-rate setting | Changes waveform timing granularity. This can alter perceived speed and strength even when phase counts look similar. |

Because those fields are packed, changing a byte can alter more than one
conceptual parameter. Treat offsets `100..104` as a narrow empirical tuning
handle, not as a decoded "strength" register.

## RAM And Update Mapping

For black/white mode, SSD1677 maps the current red-RAM and black-RAM bits into
one of four LUT transitions. The datasheet names the inputs as previous and new
pixel states. In the BinBook preview path, we intentionally create that pair by
writing:

- previous page fast-base plane to secondary/red RAM;
- target page fast-base plane to black RAM.

Then the custom preview LUT drives the panel from the previous state toward the
target state. Cleanup reuses those same RAM planes with a stronger cleanup LUT.
This is why preview tuning and cleanup tuning can be independent even though
they use the same RAM contents.

The staged activation sequence currently uses:

- `0x21 = [0x00]` for normal RAM interpretation in staged mode;
- `0x22 = [0x0c]` when the controller is already powered;
- `0x22 = [0xcc]` when the sequence also needs clock/analog enable bits;
- `0x20` master activation, followed by a BUSY wait.

Full and partial refresh paths use different `0x22` values. Do not compare BUSY
numbers across preview, cleanup, staged gray, partial BW, and full refresh
without also noting the update-control path.

## Current Working Model

Custom LUT tuning changes the electrical waveform that moves black and white
particles during a refresh. We do not have a complete SSD1677 waveform
compiler, so the current approach is empirical and intentionally narrow:

1. Start from the verified staged grayscale LUT.
2. Change only LUT offsets `100..104`.
3. Keep gate, source, and VCOM voltage bytes unchanged.
4. Measure live BUSY timing and inspect the panel visually.
5. Keep the change only if the page-turn behavior is measurably better and can
   recover to a healthy final state.

This model is a heuristic. The meaning of a numeric byte change is not simply
"higher is always darker" or "lower is always faster." The same byte can affect
drive duration, polarity pattern, and residual particle state. Treat every
candidate as hardware data, not as a proof from byte inspection.

## Preview Versus Cleanup

Tune preview and cleanup separately.

Preview LUT:

- Goal: make the next page visible quickly enough that button presses feel
  responsive.
- Primary metric: preview BUSY time.
- Secondary metric: whether text and page identity are recognizable.
- Risk: ghosting, weak contrast, wrong black/white settling, and visible
  intermediate artifacts.
- Current target: stay below the user-visible BUSY budget for immediate
  response. Recent experiments used a hard ceiling of `100 ms`.

Cleanup LUT:

- Goal: reduce ghosting left by the preview without a full refresh.
- Primary metric: visible ghost reduction after preview.
- Secondary metrics: cleanup BUSY time and whether cleanup introduces flashes
  or new artifacts.
- Risk: too weak leaves ghosting; too strong may behave like an intrusive
  flash, cause dark/light overshoot, or erase the responsiveness benefit.
- Cleanup may take longer than preview because the user has already seen the
  page transition. It still must be measured and bounded.

The normal delayed gray refinement is separate from both. It should still run
after preview and cleanup when the device is idle.

## Visual Effects To Watch

Darkness and lightness are visual outcomes, not just byte values.

When preview is too weak:

- target page is visible but washed out;
- old page remains readable under the new page;
- black text looks gray;
- white areas keep a gray haze.

When preview is too strong:

- the first transition may flash or overshoot;
- black regions can look too dark or leave dark residue;
- the page may look less stable before cleanup;
- BUSY time may rise beyond the responsiveness target.

When cleanup is too weak:

- old page structure remains visible after cleanup;
- gray refinement improves quality but does not erase ghosting;
- repeated turns accumulate residual patterns.

When cleanup is too strong:

- the cleanup becomes visually distracting;
- light areas may over-clear or flash;
- dark areas may temporarily overshoot;
- cleanup BUSY may dominate the total interaction.

## Metrics To Capture

Every candidate change needs live hardware evidence. Host tests prove command
sequence and byte selection; they do not prove display quality.

Capture at minimum:

- preview BUSY site and elapsed milliseconds;
- cleanup BUSY site and elapsed milliseconds;
- gray refresh BUSY site and elapsed milliseconds;
- page command response page;
- final STATUS with page, panel mode, dropped logs, protocol errors, and last
  error;
- structured logs showing order: preview, page commit, cleanup, gray refinement,
  base sync;
- visual judgment of ghosting, contrast, and distracting flashes.

Use discriminating navigation, such as `goto 0`, clear logs, then `next`, so
the observed page transition and logs are unambiguous.

## Tuning Rules

- Change one thing per run.
- Keep preview and cleanup candidate changes separate.
- Prefer small byte steps, such as `0x10` increments, when moving from a known
  usable point.
- Do not infer success from `ok` transport responses; verify logs, final state,
  and visual output.
- If a log query times out while background display work is still active, query
  STATUS first before retrying the log cursor.
- Keep voltage bytes unchanged unless a separate plan explicitly treats voltage
  tuning as higher-risk hardware work.
- If candidate names change, update source labels, tests, this reference, and
  any user-facing diagnostic docs in the same change.

## Current Candidate Snapshot

The names below are current working labels and may change.

| Role | Working label | LUT bytes `100..104` | Known behavior |
| --- | --- | ---: | --- |
| Baseline staged gray | `STAGED_GRAY_LUT` | `0x8f` | Verified normal staged-gray refinement baseline. |
| Diagnostic fast probe | `FastA` | `0x6f` | Measured about `70 ms` in prior probe/preview runs; visually ghosted but better than `FastB`. |
| Diagnostic fast probe | `FastB` | `0x4f` | Measured about `95 ms`; visually worse ghosting in prior runs. |
| Accepted Fast preview | `PreviewFastV1` | `0x7f` | Current Fast-mode preview recipe; latest live run measured preview BUSY about `63 ms`. |
| Accepted Fast cleanup | `CleanupStrongV1` | `0xbf` | Current Fast-mode cleanup recipe after preview; latest live run measured cleanup BUSY about `106 ms`. |

Always check `crates/xteink-x4-display/src/panel.rs` for the authoritative
current bytes before quoting a candidate value.
