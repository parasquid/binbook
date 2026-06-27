# Xteink X4 BW Seed Refresh Design

## Summary

The Xteink X4 still shows dirty pages after page turns even after normal
navigation was changed to full-screen BW differential refresh. The observed
pattern is:

- first page after boot is clean grayscale;
- first page turn is dirty;
- after repeated back-and-forth turns, pages can settle cleanly but only as
  black/white output.

The working hypothesis is that the visible grayscale page and the SSD1677 BW
differential RAM state are not the same thing. After a grayscale render, the
firmware records the page as the previous page and immediately allows a partial
BW differential turn. That assumes the controller RAM is already a coherent BW
previous/current frame pair. Hardware behavior contradicts that assumption.

The fix is to make BW differential readiness explicit. A grayscale render
produces a clean visible page but invalidates BW differential readiness. The next
different page must perform a full BW seed refresh before later partial BW
differential refreshes are allowed.

## Refresh State Model

`RefreshState` should track two facts:

- `previous_page`: the page believed to be visible on the panel;
- `bw_differential_ready`: whether SSD1677 red RAM and black RAM both contain a
  coherent BW representation of `previous_page`.

Decision order:

1. No previous page: `FullGrayscale`.
2. Same target page: `Noop`.
3. Cleanup cadence reached: `FullGrayscale`.
4. BW differential RAM not ready: `FullBwSeed`.
5. Clean default policy: `FullScreenDifferential`.
6. Chunk-dirty probe policy: `AdjacentDirtyPartial` only when a transition mask
   exists and BW differential RAM is ready.

Recording a successful `FullGrayscale` sets `previous_page = target_page` and
`bw_differential_ready = false`. Recording a successful `FullBwSeed`,
`FullScreenDifferential`, or `AdjacentDirtyPartial` sets
`previous_page = target_page` and `bw_differential_ready = true`.

## Panel Mode Model

The firmware should stop relying on the boot-time grayscale initialization for
all later refresh modes. Track display mode in `binbook-fw` with:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelMode {
    Unknown,
    Grayscale,
    Bw,
}
```

`binbook-fw`, not `ssd1677-driver`, owns this state because it is tied to page
rendering policy. The reusable driver should remain a command-layer crate.

Display rendering should initialize the panel for the mode it is about to use:

- `FullGrayscale`: call `init_grayscale_with_delay()` if mode is not
  `Grayscale`, stream plane 0 to red RAM, stream plane 1 to black RAM, then
  trigger grayscale refresh.
- `FullBwSeed`: call `init_with_delay()` if mode is not `Bw`, stream target
  page plane 2 to both red RAM and black RAM, then trigger full BW refresh.
- `FullScreenDifferential`: ensure `Bw`, stream previous page plane 2 to red
  RAM, stream target page plane 2 to black RAM, then trigger partial refresh.
- `AdjacentDirtyPartial`: ensure `Bw`, stream only transition-marked previous
  and target BW chunks, then trigger partial refresh. This remains probe-only.

## Logging Requirements

Normal debug builds must make the active path visible over serial before visual
inspection:

- normal firmware logs `[REFRESH] policy=FullScreenDifferentialDefault`;
- every render logs the target page and selected decision;
- mode changes log `[PANEL] init=grayscale` or `[PANEL] init=bw`;
- probe builds still log `[PROBE] chunk_dirty_window`.

The first normal navigation sequence after boot should show:

1. page 0: `FullGrayscale`, panel mode `grayscale`;
2. first page turn: `FullBwSeed`, panel mode `bw`;
3. second page turn: `FullScreenDifferential`, panel mode remains `bw`.

## Acceptance Criteria

Host tests must prove:

- `FullBwSeed` is selected after any successful `FullGrayscale` before partial
  differential is allowed;
- `FullBwSeed` records BW differential readiness;
- cleanup cadence returns to `FullGrayscale` and invalidates BW readiness;
- chunk-dirty probe policy cannot use chunk-dirty partial refresh until BW
  readiness is true;
- normal render path passes panel mode state into display rendering;
- display rendering has explicit BW and grayscale initialization paths.

Hardware verification is the final completion gate. The change is incomplete
until the Xteink X4 is flashed and serial plus visual evidence are recorded in
`HANDOFF.md`.
