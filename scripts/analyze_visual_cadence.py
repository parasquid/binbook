#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
import shutil
import statistics
import subprocess
import tempfile
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

from PIL import Image, ImageChops, ImageStat


@dataclass(frozen=True, slots=True)
class Crop:
    x: int
    y: int
    width: int
    height: int


@dataclass(frozen=True, slots=True)
class FrameSample:
    index: int
    unix_ms: float
    image: Image.Image


@dataclass(frozen=True, slots=True)
class TrialVisualReport:
    trial: int
    gap_ms: int
    key_count: int
    transition_count: int
    visual_ok: bool
    latencies_ms: list[int]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--crop", required=True, help="page-number crop as x,y,w,h")
    parser.add_argument("--video")
    parser.add_argument("--frames-dir")
    parser.add_argument("--video-start-unix-ms", type=float, required=True)
    parser.add_argument("--fps", type=float)
    parser.add_argument("--threshold", type=float, default=6.0)
    parser.add_argument("--min-transition-gap-ms", type=float, default=120.0)
    parser.add_argument("--preview-out")
    args = parser.parse_args()

    if bool(args.video) == bool(args.frames_dir):
        parser.error("provide exactly one of --video or --frames-dir")
    crop = parse_crop(args.crop)
    records = read_jsonl(Path(args.evidence))
    with extracted_frames(args.video) as extracted:
        frames_dir = Path(args.frames_dir) if args.frames_dir else extracted
        fps = args.fps if args.fps else probe_fps(Path(args.video))
        frames = load_frames(frames_dir, crop, args.video_start_unix_ms, fps)
        if not frames:
            print("no frames found")
            return 1
        if args.preview_out:
            frames[0].image.save(args.preview_out)
        transitions = detect_transitions(
            frames,
            threshold=args.threshold,
            min_transition_gap_ms=args.min_transition_gap_ms,
        )
        reports = build_reports(records, transitions)
    print_reports(reports)
    return 0


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    return [
        json.loads(line)
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]


def parse_crop(value: str) -> Crop:
    parts = value.split(",")
    if len(parts) != 4:
        raise SystemExit("--crop must be x,y,w,h")
    x, y, width, height = (int(part) for part in parts)
    if width <= 0 or height <= 0:
        raise SystemExit("--crop width and height must be positive")
    return Crop(x=x, y=y, width=width, height=height)


def probe_fps(video: Path) -> float:
    output = subprocess.check_output(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=avg_frame_rate",
            "-of",
            "default=nw=1:nk=1",
            str(video),
        ],
        text=True,
    ).strip()
    numerator, denominator = output.split("/", 1)
    fps = float(numerator) / float(denominator)
    if not math.isfinite(fps) or fps <= 0:
        raise SystemExit(f"invalid video frame rate: {output}")
    return fps


class extracted_frames:
    def __init__(self, video: str | None):
        self.video = video
        self.tempdir: tempfile.TemporaryDirectory[str] | None = None

    def __enter__(self) -> Path:
        if not self.video:
            return Path()
        if shutil.which("ffmpeg") is None:
            raise SystemExit("ffmpeg is required for --video")
        self.tempdir = tempfile.TemporaryDirectory(prefix="binbook-visual-cadence-")
        pattern = Path(self.tempdir.name) / "frame_%06d.png"
        subprocess.run(
            [
                "ffmpeg",
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
                self.video,
                "-vsync",
                "0",
                str(pattern),
            ],
            check=True,
        )
        return Path(self.tempdir.name)

    def __exit__(self, exc_type: object, exc: object, traceback: object) -> None:
        if self.tempdir:
            self.tempdir.cleanup()


def load_frames(
    frames_dir: Path,
    crop: Crop,
    video_start_unix_ms: float,
    fps: float,
) -> list[FrameSample]:
    frame_paths = sorted(
        path
        for path in frames_dir.iterdir()
        if path.suffix.lower() in {".png", ".jpg", ".jpeg"}
    )
    samples = []
    frame_period_ms = 1000.0 / fps
    for index, path in enumerate(frame_paths):
        image = Image.open(path).convert("L")
        cropped = image.crop((crop.x, crop.y, crop.x + crop.width, crop.y + crop.height))
        samples.append(
            FrameSample(
                index=index,
                unix_ms=video_start_unix_ms + index * frame_period_ms,
                image=cropped,
            )
        )
    return samples


def detect_transitions(
    frames: list[FrameSample],
    *,
    threshold: float,
    min_transition_gap_ms: float,
) -> list[float]:
    transitions: list[float] = []
    last_transition_ms = -math.inf
    for previous, current in zip(frames, frames[1:]):
        score = mean_abs_difference(previous.image, current.image)
        if score < threshold:
            continue
        if current.unix_ms - last_transition_ms < min_transition_gap_ms:
            continue
        transitions.append(current.unix_ms)
        last_transition_ms = current.unix_ms
    return transitions


def mean_abs_difference(left: Image.Image, right: Image.Image) -> float:
    diff = ImageChops.difference(left, right)
    return float(ImageStat.Stat(diff).mean[0])


def build_reports(
    records: list[dict[str, Any]],
    transitions: list[float],
) -> list[TrialVisualReport]:
    trial_records: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for record in records:
        trial = record.get("trial")
        if isinstance(trial, int):
            trial_records[trial].append(record)
    starts = {
        trial: float(first_value(items, "host_unix_ms", 0))
        for trial, items in trial_records.items()
        if first_kind(items, "trial_start")
    }
    sorted_trials = sorted(trial_records)
    reports = []
    for index, trial in enumerate(sorted_trials):
        items = trial_records[trial]
        keys = sorted(
            (
                record
                for record in items
                if record.get("kind") == "key_sent" and "host_unix_ms" in record
            ),
            key=lambda record: (int(record.get("turn_index", 0)), float(record["host_unix_ms"])),
        )
        if not keys:
            continue
        next_trial_start = (
            starts.get(sorted_trials[index + 1], math.inf)
            if index + 1 < len(sorted_trials)
            else math.inf
        )
        matched = match_transitions(keys, transitions, next_trial_start)
        reports.append(
            TrialVisualReport(
                trial=trial,
                gap_ms=int(first_value(items, "gap_ms", 0)),
                key_count=len(keys),
                transition_count=len(matched),
                visual_ok=len(matched) == len(keys),
                latencies_ms=[
                    int(round(transition - float(key["host_unix_ms"])))
                    for key, transition in matched
                ],
            )
        )
    return reports


def match_transitions(
    keys: list[dict[str, Any]],
    transitions: list[float],
    next_trial_start: float,
) -> list[tuple[dict[str, Any], float]]:
    matched: list[tuple[dict[str, Any], float]] = []
    transition_index = 0
    for index, key in enumerate(keys):
        key_time = float(key["host_unix_ms"])
        window_end = (
            float(keys[index + 1]["host_unix_ms"])
            if index + 1 < len(keys)
            else next_trial_start
        )
        while transition_index < len(transitions) and transitions[transition_index] < key_time:
            transition_index += 1
        if (
            transition_index < len(transitions)
            and key_time <= transitions[transition_index] < window_end
        ):
            matched.append((key, transitions[transition_index]))
            transition_index += 1
    return matched


def first_kind(records: Iterable[dict[str, Any]], kind: str) -> dict[str, Any] | None:
    for record in records:
        if record.get("kind") == kind:
            return record
    return None


def first_value(records: Iterable[dict[str, Any]], key: str, default: Any) -> Any:
    for record in records:
        if key in record:
            return record[key]
    return default


def print_reports(reports: list[TrialVisualReport]) -> None:
    by_gap: dict[int, list[TrialVisualReport]] = defaultdict(list)
    for report in reports:
        by_gap[report.gap_ms].append(report)
    passing_gaps = []
    for gap_ms, items in sorted(by_gap.items()):
        visual_ok = all(item.visual_ok for item in items)
        if visual_ok:
            passing_gaps.append(gap_ms)
        transitions = sum(item.transition_count for item in items)
        keys = sum(item.key_count for item in items)
        latencies = [latency for item in items for latency in item.latencies_ms]
        print(
            f"gap_ms={gap_ms} visual_ok={format_bool(visual_ok)} "
            f"transitions={transitions}/{keys} "
            f"visual_latency_avg_ms={format_optional_mean(latencies)}"
        )
    if passing_gaps:
        print(f"fastest_visual_gap_ms={min(passing_gaps)}")
    else:
        print("fastest_visual_gap_ms=NA")


def format_bool(value: bool) -> str:
    return "true" if value else "false"


def format_optional_mean(values: list[int]) -> str:
    if not values:
        return "NA"
    return str(int(statistics.mean(values)))


if __name__ == "__main__":
    raise SystemExit(main())
