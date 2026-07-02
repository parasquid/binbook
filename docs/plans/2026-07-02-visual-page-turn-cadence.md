# Visual Page-Turn Cadence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add webcam-based page-number transition measurement for serial-injected page-turn cadence.

**Architecture:** Keep the existing serial cadence runner for key injection and JSONL evidence. Add host timestamps to evidence and a Python visual analyzer that maps key send times to page-number crop transitions detected from video frames.

**Tech Stack:** Rust CLI diagnostics, Python 3, Pillow, ffmpeg/ffprobe, pytest.

---

### Task 1: Timestamp Evidence

**Files:**
- Modify: `crates/binbook/src/page_turn_cadence_runtime.rs`
- Modify: `crates/binbook/tests/hardware_diagnostic.rs`

- [ ] Add `host_unix_ms` to `trial_start`, `key_sent`, and `key_response` evidence records.
- [ ] Update the existing cadence evidence test to assert `host_unix_ms` is present on key events.
- [ ] Run `cargo test -p binbook --features serial-device page_turn_cadence -- --nocapture`.

### Task 2: Visual Analyzer

**Files:**
- Create: `scripts/analyze_visual_cadence.py`
- Create: `tests/test_visual_cadence_analysis.py`

- [ ] Write a failing pytest that creates synthetic page-number crop frames and cadence JSONL evidence.
- [ ] Implement crop parsing, frame loading, frame-difference transition detection, key-to-transition matching, and fastest-gap reporting.
- [ ] Run `uv run pytest -q tests/test_visual_cadence_analysis.py`.

### Task 3: Live Capture

**Files:**
- Modify: `HANDOFF.md`

- [ ] Capture a recalibrated webcam video for at least one failing and one passing gap.
- [ ] Run `scripts/analyze_visual_cadence.py` on both captures.
- [ ] Record commands, crop, evidence paths, video paths, and results in `HANDOFF.md`.

### Task 4: Verification

- [ ] Run `cargo test -p binbook --features serial-device`.
- [ ] Run `cargo clippy -p binbook --all-targets --features serial-device -- -D warnings`.
- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run `uv run pytest -q`.
- [ ] Run `git diff --check`.
