# Xteink X4 Orientation Test Image Design

## Purpose

Replace page 0 of the four-page navigation fixture with a diagnostic target
that makes rotation, mirroring, clipping, half-screen writes, stale pixels, and
grayscale errors unambiguous in a calibrated webcam image. Pages 1–3 remain
available for navigation testing.

## Image Layout

The source image uses the `xteink-x4-portrait` logical canvas of 480 by 800
pixels and is encoded through the normal BinBook image pipeline.

- A 10-pixel black border follows the complete active-area perimeter.
- Bold 64-pixel labels identify `TL`, `TR`, `BL`, and `BR`.
- Each corner has a distinct adjacent symbol: triangle, circle, square, or
  diamond.
- Bold edge labels identify `TOP`, `RIGHT`, `BOTTOM`, and `LEFT`. Side labels
  are rotated to follow their edges.
- A center crosshair and horizontal and vertical rulers intersect at the exact
  logical center.
- Alternating major edge ticks occur every 100 logical pixels; shorter minor
  ticks occur every 50 pixels.
- Four labeled swatches show black, dark gray, light gray, and white.
- A light-gray grid covers otherwise white regions so an unwritten white area
  cannot resemble intended background.
- `PAGE 0` and `PORTRAIT 480x800` identify the page and expected orientation.
- A solid black triangle appears only at the top-left as the primary asymmetric
  orientation marker.

Labels and markers must remain inside the 10-pixel border and must not overlap
the swatches, rulers, or one another.

## Integration

`firmware/scripts/build-nav-probe-fixture.py` generates the image and uses it as
page 0 of `firmware/crates/binbook-fw/fixtures/nav_probe.binbook`. The remaining
fixture pages retain their distinct checkerboard, stripe, and text patterns.
No protocol, runtime, or display-driver behavior changes for this diagnostic.

## Automated Checks

Tests decode page 0 back to logical orientation and verify:

- all four active-area borders are dark over their required spans;
- each corner marker occupies its assigned quadrant;
- the grid reaches both halves of the page;
- all four grayscale swatches contain distinct expected levels;
- the center crosshair intersects the logical center;
- dark or gray diagnostic pixels exist on both sides of the vertical center and
  both sides of the horizontal center.

The tests must fail against the current gray-band page before fixture
generation changes are made.

## Device Verification

Rebuild the fixture, run the focused Python and firmware fixture tests, build
and flash `firmware-bin,diagnostic-console`, and request page 0. Capture a fresh
native 1920x1080 webcam still from `/dev/video1` and crop it with the confirmed
`crop=440:770:770:250` calibration.

The hardware result is acceptable only if the complete border, every 64-pixel
label, all unique corner symbols, both rulers, the grid, and all grayscale
swatches are visible across the entire active e-ink area. The black device
bezel included by the webcam crop is not rendered content.
