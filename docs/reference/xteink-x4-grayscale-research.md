# Xteink X4 Grayscale Research

This note records device-specific research for the `xteink-x4-portrait` BinBook profile. It is not the universal BinBook format specification; use [`../../BINBOOK_FORMAT_SPEC.md`](../../BINBOOK_FORMAT_SPEC.md) for the current BinBook 0.1 candidate format.

## CrossPoint Reader Findings

CrossPoint Reader treats the Xteink X4 display stack as capable of both a 1-bit fast path and a 2-bit high-quality grayscale path.

Repository inspected: `crosspoint-reader` commit `bc9651b`, cloned locally under `tmp/crosspoint-reader`.

Relevant files:

- `lib/Xtc/Xtc/XtcTypes.h`
- `lib/Xtc/README`
- `src/activities/reader/XtcReaderActivity.cpp`
- `lib/GfxRenderer/GfxRenderer.h`
- `lib/GfxRenderer/GfxRenderer.cpp`
- `src/activities/reader/ReaderUtils.h`

## XTC / XTG 1-Bit Path

CrossPoint defines `XTC` containers with `XTG` page data for 1-bit monochrome output.

Observed properties:

- Row-major storage
- 8 pixels per byte
- MSB-first
- `0 = black`
- `1 = white`

This is the fast/simple path.

## XTCH / XTH 2-Bit Path

CrossPoint defines `XTCH` containers with `XTH` page data for 2-bit grayscale output.

Observed properties:

- Two bit planes stored sequentially
- Column-major layout
- Columns are stored right-to-left
- 8 vertical pixels per byte
- Native XTH value mapping:
  - `0 = white`
  - `1 = dark gray`
  - `2 = light gray`
  - `3 = black`

This differs from BinBook's canonical `GRAY2_PACKED` storage, which is row-major and uses:

- `0 = black`
- `1 = dark gray`
- `2 = light gray`
- `3 = white`

## Rendering Flow

CrossPoint renders XTH pages with multiple display passes:

1. Draw all non-white pixels into the black/white framebuffer.
2. Display the black/white base frame.
3. Build the grayscale LSB buffer for dark gray pixels.
4. Build the grayscale MSB buffer for light gray and dark gray pixels.
5. Call `displayGrayBuffer()` to apply the grayscale overlay.
6. Rebuild/cleanup the black/white framebuffer for the next frame.

CrossPoint's text/font rendering also has grayscale paths. `GfxRenderer` exposes `BW`, `GRAYSCALE_LSB`, and `GRAYSCALE_MSB` render modes, and built-in fonts are generated with `--2bit`.

## Rotation

CrossPoint uses logical portrait coordinates of `480x800` and maps them to the physical `800x480` panel by rotating 90 degrees clockwise:

```text
physical_x = logical_y
physical_y = panel_height - 1 - logical_x
```

For BinBook's `xteink-x4-portrait` profile, this corresponds to:

```text
physical_x = logical_y
physical_y = logical_width_px - 1 - logical_x
```

## BinBook Implications

BinBook should keep page storage canonical instead of storing CrossPoint/XTH-native blobs:

- `GRAY2_PACKED` remains row-major, left-to-right, top-to-bottom.
- Canonical `GRAY2_PACKED` values remain `0=black`, `1=dark gray`, `2=light gray`, `3=white`.
- X4 firmware/display code converts canonical BinBook pixels into the native grayscale buffer/update sequence.

Recommended X4 profile behavior:

- Default output: `GRAY2_PACKED`, for the quality path.
- Optional lower format: `GRAY1_PACKED`, for an explicit fast/simple mode.
- Do not require the universal BinBook format to adopt XTH's physical bit-plane layout.
