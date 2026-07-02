# Visual Page-Turn Cadence Design

## Goal

Measure the fastest serial-injected key cadence where each key produces a distinct visible page-number transition start on the webcam feed.

## Measurement Definition

- Input source: serial-injected diagnostic KEY frames.
- Visual signal: the page-number crop in the webcam video.
- Passing gap: every injected key in a trial maps, in order, to one distinct detected visual transition in the page-number crop.
- Failing gap: any key has no following visual transition, or multiple queued keys collapse into fewer visible page-number transitions.

## Timing Model

The CLI evidence must include host-wall-clock timestamps for trial start, key send, and key response records. The webcam capture records its own start host timestamp. Video frame timestamps are computed as `video_start_unix_ms + frame_index * frame_period_ms`.

Firmware logs remain useful for diagnostics, but they are not the pass/fail source for visual cadence because queued keys can still produce successful firmware completions after the intended visual press window has passed.

## Crop Calibration

The operator supplies or recalibrates a page-number crop as `x,y,w,h`. The analyzer saves a crop preview so the crop can be checked before trusting the result.

## Output

The report prints per-gap visual pass/fail, detected transition counts, per-key visual latency where available, and the fastest passing gap.
