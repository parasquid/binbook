# Xteink X4 Navigation Probe Design

## Summary

Build a four-page embedded BinBook firmware probe for Xteink X4 device testing.
The first page remains the currently deployed gray-band smoke page. The
remaining pages are deterministic visual probes: checkerboard, stripes, and a
full lorem ipsum text page. Xteink directional buttons navigate between pages.

This milestone remains embedded-fixture based. It proves page indexing, page
blob selection, button input, and repeated GRAY2 rendering without adding
flash-backed book upload or SD-card storage.

## Behavior

- On boot, firmware renders page 1: the existing gray-band GRAY2 smoke page.
- `Right` and `Down` move one page forward.
- `Left` and `Up` move one page backward.
- Navigation clamps at the edges:
  - previous on page 1 does nothing;
  - next on page 4 does nothing.
- `Back`, `Select`, and `Power` are ignored for this milestone.
- Page turns are queued while the deferred-gray refresh coordinator works;
  input handling remains responsive while grayscale and reseed work runs.

## Fixture

Create `firmware/crates/binbook-fw/fixtures/nav_probe.binbook` with exactly four
pages:

1. The current gray-band page from `gray2_probe.binbook`, preserving its
   compressed page payload byte-for-byte.
2. A checkerboard page.
3. A four-tone stripes page.
4. A full page of lorem ipsum text rendered through the existing text renderer.

All pages use the `xteink-x4-portrait` profile and must be stored as current
writer output: `GRAY2_PACKED`, physical `800x480`, RLE PackBits compressed, and
a single plane in plane slot 0.

## Firmware Architecture

Firmware includes `nav_probe.binbook` with `include_bytes!`, opens it once with
the existing Rust `binbook` parser, and renders page 0 at startup. The main loop
polls the GPIO1/GPIO2 ADC ladder buttons and calls a small saturating navigation
helper. When the selected page changes, firmware renders the new page.

The existing `BinBook::page()` API copies the full compressed page payload into
scratch before returning a page reference. A rendered text page can compress to
tens of kilobytes, so the navigation probe must not depend on increasing the
scratch buffer. Instead, firmware should use parser metadata to locate the
single compressed page slice inside the embedded book bytes, then stream that
slice through the existing GRAY2 row renderer.

## Interfaces

- No BinBook file-format change.
- No Python CLI behavior change is required.
- Add or expose firmware helpers for:
  - saturating page navigation from `Button` input;
  - raw ADC polling from channel readings;
  - deriving a single-plane compressed page data slice from an embedded book.

## Tests

- Rust host tests cover navigation direction, edge clamping, ignored buttons,
  raw button polling, and embedded page-slice bounds.
- Python tests or fixture-builder self-checks verify:
  - `nav_probe.binbook` has exactly four pages;
  - every page is `GRAY2_PACKED`, `800x480`, RLE PackBits, single plane;
  - page 1 payload matches the current gray-band probe payload exactly;
  - checkerboard, stripes, and text pages decode to meaningful nonblank content.
- Hardware acceptance:
  - boot shows gray bands;
  - `Right`/`Down` advances through checkerboard, stripes, and lorem ipsum;
  - `Left`/`Up` navigates back;
  - edge button presses clamp.
