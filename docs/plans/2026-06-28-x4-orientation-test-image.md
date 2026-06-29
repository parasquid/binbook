# Xteink X4 Orientation Test Image Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put a full-panel diagnostic orientation target with webcam-readable 64-pixel labels on page 0 of the attached Xteink X4.

**Architecture:** Generate the target as a logical 480x800 Pillow image in the existing navigation-fixture builder, then pass it through the normal rotation, packing, native-plane, compression, and firmware embedding pipeline. Tests reconstruct page 0 from the stored native planes and verify full-canvas geometry before any hardware flash.

**Tech Stack:** Python 3.13, Pillow, pytest, BinBook native X4 planes, Rust firmware, espflash, diagnostic serial CLI, ffmpeg.

---

### Task 1: Add RED full-panel fixture checks

**Files:**
- Modify: `tests/test_nav_probe_fixture.py`

- [x] **Step 1: Add native-plane logical decoding for tests**

Read plane slots 0 and 1 from `BinBookReader`, decompress them with
`decode_packbits`, map native bits back to gray values `0..3`, construct the
800x480 storage image, and call `storage_image_to_logical(..., 480, 800, 270)`.

- [x] **Step 2: Add one focused geometry test**

Require page 0 to have dark pixels along all four 10-pixel borders, a dark
center crosshair at `(240, 400)`, distinct swatch samples `[0, 85, 170, 255]`,
and non-white pixels in all four logical quadrants. Require the right half to
contain non-white pixels so the current gray-band fixture fails.

- [x] **Step 3: Verify RED**

Run:

```bash
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline pytest -q tests/test_nav_probe_fixture.py
```

Expected: the new page-0 geometry test fails against the current fixture.

### Task 2: Generate the orientation target

**Files:**
- Modify: `firmware/scripts/build-nav-probe-fixture.py`
- Modify: `firmware/crates/binbook-fw/fixtures/nav_probe.binbook`

- [x] **Step 1: Add `_make_orientation_target(profile)`**

Create a logical grayscale image with a 10-pixel black border, 50/100-pixel
ticks, faint full-page grid, center rulers, four 64-pixel bold corner labels,
64-pixel edge labels, unique corner shapes, four grayscale swatches, `PAGE 0`,
and `PORTRAIT 480x800`. Keep every element inside the active-area border.

- [x] **Step 2: Use the target as page 0**

Replace the source-fixture gray-band page construction with
`pil_image_to_packed(_make_orientation_target(profile), profile, dither=False)`
and retain pages 1–3 unchanged.

- [x] **Step 3: Rebuild the binary fixture**

Run:

```bash
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline python firmware/scripts/build-nav-probe-fixture.py
```

Expected: four pages, 360 chunks, and six transitions.

- [x] **Step 4: Verify GREEN**

Run:

```bash
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline pytest -q tests/test_nav_probe_fixture.py tests/test_x4_native_planes.py
cd firmware && cargo test -p binbook-fw --features diagnostic-console
```

Expected: all focused Python and firmware tests pass.

### Task 3: Put the target on the attached display

**Files:**
- Modify: `HANDOFF.md`

- [x] **Step 1: Build and flash the diagnostic image**

```bash
FW_FEATURES="firmware-bin,diagnostic-console" firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Run with host escalation and wait for USB re-enumeration.

- [x] **Step 2: Verify identity and request page 0**

```bash
cd cli
cargo run --features serial-device -- diag hello --port /dev/ttyACM0
cargo run --features serial-device -- diag page --port /dev/ttyACM0 goto 0
cargo run --features serial-device -- diag status --port /dev/ttyACM0
```

Expected: protocol 1, `current_page=0`, and zero runtime error counters.

- [x] **Step 3: Capture and crop a fresh webcam still**

```bash
ffmpeg -hide_banner -loglevel error -f video4linux2 -video_size 1920x1080 -framerate 30 -i /dev/video1 -frames:v 1 /tmp/x4_orientation_target_full.jpg
ffmpeg -hide_banner -loglevel error -i /tmp/x4_orientation_target_full.jpg -vf "crop=440:770:770:250" -frames:v 1 /tmp/x4_orientation_target_panel.jpg
```

Inspect the actual cropped webcam file. The crop intentionally includes the
black device bezel; only the lighter active panel is rendered content.

- [x] **Step 4: Record current-state evidence**

Rewrite `HANDOFF.md` with the implemented target, exact host results, flash and
serial output, both image paths, the actual visible result, and the unresolved
half-screen diagnosis if any marker is missing.

- [x] **Step 5: Run final focused checks**

```bash
UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline pytest -q tests/test_nav_probe_fixture.py
git diff --check
```

Expected: tests pass and no whitespace errors.
