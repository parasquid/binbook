# Xteink X4 Clean Differential Refresh Design

## Summary

Status: precursor policy record. The current async deferred-gray firmware
design lives in `docs/specs/2026-06-27-x4-async-deferred-grayscale-design.md`.

Xteink X4 page turns currently use compiler-generated BW chunk metadata to write
only changed 16-row chunks before triggering an SSD1677 partial refresh. That is
fast, but hardware testing has shown visible corruption from the previous page
when paging through BinBook pages. The likely failure mode is that the controller
partial waveform is using RAM outside the chunks just written, or the RAM outside
those chunks still contains grayscale seed data rather than a coherent BW
previous/current frame pair.

The clean default must preserve the fast-first goal while making chunk-dirty
partial refresh conditional on hardware proof. Until that proof exists, the
clean fallback is SquidScript-style full-screen BW differential refresh: stream
the previous page BW plane to SSD1677 red RAM (`0x26`), stream the target page BW
plane to SSD1677 black RAM (`0x24`), then trigger partial refresh. This still
uses bounded chunk streaming; it only changes which chunks are written.

This feature is not complete until hardware verification confirms either:

- chunk-dirty refresh is visually clean on the Xteink X4 and can remain default;
  or
- chunk-dirty refresh is disabled by default and full-screen BW differential
  page turns are visually clean.

## Current Context

The existing X4 native chunked refresh implementation has these relevant pieces:

- `binbook/pixels.py` emits SSD1677-native GRAY2 MSB, LSB, and BW planes.
- `binbook/writer.py` emits `PAGE_CHUNK_INDEX` and `PAGE_TRANSITION_INDEX`.
- `firmware/crates/binbook-fw/src/async_refresh.rs` tracks previous page and fast
  refresh cadence.
- `firmware/crates/binbook-fw/src/display.rs` chooses one of:
  - full grayscale seed/cleanup;
  - adjacent dirty partial refresh;
  - full-screen BW differential refresh.

The precursor policy selects `AdjacentDirtyPartial` whenever a transition mask
exists. That makes chunk-dirty refresh the normal adjacent page-turn path
before the hardware behavior has been proven clean.

SquidScript solved the same class of problem by rendering or streaming both
frames for a differential update: the old BW image is supplied to the controller
as the previous image, and the new BW image is supplied as the current image.
For BinBook on X4, those images are plane 2 for the previous and target pages.

## Refresh Requirements

### BW Seed Readiness

A grayscale render must not be treated as a valid BW differential seed. After a
grayscale render or cleanup cadence, firmware must perform a full BW seed refresh
before any partial BW differential refresh. The BW seed writes the same target
page BW plane to red RAM and black RAM, then triggers a full BW refresh.

### Frame Semantics

For any BW differential partial refresh, SSD1677 RAM must contain a coherent pair
for the area affected by the refresh waveform:

- red RAM (`0x26`) contains the previous BW image;
- black RAM (`0x24`) contains the target BW image;
- BW plane polarity remains SSD1677-native: white bits are set, active black
  pigment bits are cleared.

The firmware must not assume that a grayscale refresh leaves red and black RAM in
a usable BW differential state for later partial page turns.

### Default Policy

The firmware must support two fast-refresh policies:

1. `FullScreenDifferentialDefault`
   - Adjacent transitions and non-adjacent jumps both stream all 30 BW chunks
     for previous and target pages before partial refresh.
   - This is the clean fallback and must be available unconditionally.
2. `ChunkDirtyDifferentialDefault`
   - Adjacent transitions stream only the changed chunks from the transition
     mask.
   - This mode may be the default only after a hardware probe proves it is clean
     on the Xteink X4.

If hardware verification is unavailable or inconclusive, the compiled firmware
must default to `FullScreenDifferentialDefault`.

### Hardware Probe

The firmware must include a debug-gated probe that visually tests whether
chunk/window partial refresh is clean. The probe should:

1. Initialize the panel using the normal X4 SSD1677 path.
2. Seed a known full-screen state with full grayscale or full BW refresh.
3. Write previous/current BW data for a small window or one 16-row chunk only.
4. Trigger partial refresh.
5. Make corruption outside the written area visually obvious.

The probe must be behind a compile-time feature or explicit debug path. It must
not run during normal page turns.

Hardware verification evidence must include:

- exact firmware build command;
- exact flash/run command;
- serial/debug output showing selected refresh mode and page/probe step;
- visual result recorded by the agent, including whether stale previous-page
  pixels remain in white areas.

## Implementation Boundaries

`binbook-fw` owns refresh policy and BinBook page/chunk orchestration. The
policy should be testable without hardware.

`ssd1677-driver` remains a reusable command and streaming crate. It may gain a
small helper for writing a chunk/window to a specific RAM plane, but it must not
know BinBook page numbers, transition masks, cleanup cadence, or probe policy.

The `.binbook` format does not need new metadata for this fix. Existing
`PAGE_CHUNK_INDEX`, `PAGE_TRANSITION_INDEX`, and plane 2 BW chunks are enough.

## Acceptance Criteria

Host tests:

- refresh policy defaults to full-screen differential unless chunk-dirty mode is
  explicitly enabled;
- transition masks are ignored in full-screen default mode;
- transition masks select adjacent dirty partial only in chunk-dirty mode;
- `changed_chunk_mask == 0` records a successful no-op page-state update only if
  the target page is known to be visually identical;
- failed streaming or refresh does not advance previous-page state;
- full-screen differential writes previous BW to red RAM and target BW to black
  RAM before triggering partial refresh;
- chunk-dirty differential writes previous and target BW chunks for identical
  windows before triggering partial refresh.

Verification commands:

- `cd firmware && cargo test --workspace`
- `cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release`
- the hardware probe flash/run command chosen by the implementation
- the normal navigation firmware flash/run command chosen by the implementation

Completion gate:

- The feature is incomplete until hardware verification has been run on the
  Xteink X4 with host/device access and the result is documented in `HANDOFF.md`.

## Documentation Updates

Update `BINBOOK_FORMAT_SPEC.md` so its X4 refresh section says chunk-dirty
partial refresh is conditional on proven clean window-scoped behavior. The spec
should describe full-screen BW differential as the clean default fallback.

Update `docs/specs/2026-06-26-x4-native-chunked-refresh-design.md` or supersede
its refresh-policy section with this design so future agents do not treat
chunk-dirty refresh as unconditionally safe.

Update `HANDOFF.md` after implementation with:

- final default policy;
- test commands and outcomes;
- hardware commands and outcomes;
- whether chunk-dirty mode is enabled by default.
