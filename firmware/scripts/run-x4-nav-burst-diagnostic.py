#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path

from PIL import Image, ImageDraw


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", default="/dev/ttyACM0")
    parser.add_argument("--video-device", default="/dev/video1")
    parser.add_argument("--rounds", type=int, default=10)
    parser.add_argument("--inter-key-ms", type=int, default=0)
    parser.add_argument("--output-dir", type=Path, default=Path("/tmp/x4-nav-burst"))
    parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args()


def commands(args: argparse.Namespace) -> tuple[list[str], list[str]]:
    video = args.output_dir / "nav-burst.mp4"
    evidence = args.output_dir / "evidence.jsonl"
    camera = [
        "ffmpeg",
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "video4linux2",
        "-video_size",
        "1920x1080",
        "-framerate",
        "30",
        "-i",
        args.video_device,
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        str(video),
    ]
    exercise = [
        "cargo",
        "run",
        "--manifest-path",
        "crates/binbook/Cargo.toml",
        "--features",
        "serial-device",
        "--",
        "diag",
        "exercise",
        "nav-burst",
        "--port",
        args.port,
        "--rounds",
        str(args.rounds),
        "--inter-key-ms",
        str(args.inter_key_ms),
        "--output",
        str(evidence),
    ]
    return camera, exercise


def print_dry_run(args: argparse.Namespace) -> None:
    camera, exercise = commands(args)
    print("camera:", " ".join(camera))
    print("exercise:", " ".join(exercise))
    print("crop: ffmpeg -vf crop=440:770:770:250")
    print("frames: round-XX-seq-YYYY-expected-ZZ.jpg")
    print("contact-sheet:", args.output_dir / "round-XX-contact-sheet.jpg")


def load_keys(path: Path) -> list[dict[str, object]]:
    records = []
    for line in path.read_text().splitlines():
        record = json.loads(line)
        if record.get("kind") == "key":
            records.append(record)
    return records


def extract_frame(video: Path, offset: float, output: Path) -> None:
    subprocess.run(
        [
            "ffmpeg",
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            f"{max(0, offset):.3f}",
            "-i",
            str(video),
            "-frames:v",
            "1",
            "-vf",
            "crop=440:770:770:250",
            str(output),
        ],
        check=True,
    )


def video_duration(video: Path) -> float:
    result = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=nw=1:nk=1",
            str(video),
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    return float(result.stdout.strip())


def contact_sheet(
    round_number: int, frames: list[tuple[Path, str]], output_dir: Path
) -> None:
    width, height = 220, 420
    sheet = Image.new("RGB", (width * 4, height * ((len(frames) + 3) // 4)), "white")
    for index, (path, caption) in enumerate(frames):
        image = Image.open(path).convert("RGB")
        image.thumbnail((width, height - 40))
        x = index % 4 * width
        y = index // 4 * height
        sheet.paste(image, (x + (width - image.width) // 2, y + 30))
        ImageDraw.Draw(sheet).text((x + 4, y + 5), caption, fill="black")
    sheet.save(output_dir / f"round-{round_number:02d}-contact-sheet.jpg")


def extract_evidence(output_dir: Path, video_start_unix_ms: int) -> None:
    video = output_dir / "nav-burst.mp4"
    duration = video_duration(video)
    keys = load_keys(output_dir / "evidence.jsonl")
    rounds = sorted({int(record["round"]) for record in keys})
    for round_number in rounds:
        frames = []
        round_keys = [record for record in keys if int(record["round"]) == round_number]
        for record in round_keys:
            sequence = int(record["sequence"])
            expected = int(record["expected_to"])
            offset = (int(record["host_unix_ms"]) - video_start_unix_ms) / 1000
            path = (
                output_dir
                / f"round-{round_number:02d}-seq-{sequence:04d}-expected-{expected:02d}.jpg"
            )
            extract_frame(video, min(offset, duration - 0.05), path)
            frames.append(
                (
                    path,
                    f"seq {sequence} {record['key']} -> {expected} {record['response_elapsed_ms']}ms",
                )
            )
        settled = output_dir / f"round-{round_number:02d}-settled.jpg"
        final_offset = (
            int(round_keys[-1]["host_unix_ms"]) - video_start_unix_ms
        ) / 1000 + 0.7
        extract_frame(video, min(final_offset, duration - 0.05), settled)
        frames.append((settled, f"round {round_number} settled"))
        contact_sheet(round_number, frames, output_dir)


def main() -> int:
    args = parse_args()
    if args.dry_run:
        print_dry_run(args)
        return 0
    args.output_dir.mkdir(parents=True, exist_ok=True)
    camera_command, exercise_command = commands(args)
    video_start_unix_ms = time.time_ns() // 1_000_000
    (args.output_dir / "video-start-unix-ms.txt").write_text(f"{video_start_unix_ms}\n")
    camera = subprocess.Popen(camera_command)
    transcript_path = args.output_dir / "exercise-transcript.txt"
    stderr_path = args.output_dir / "exercise-stderr.txt"
    try:
        with (
            transcript_path.open("w") as transcript,
            stderr_path.open("w") as stderr_transcript,
        ):
            exercise = subprocess.Popen(
                exercise_command,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            assert exercise.stdout is not None
            assert exercise.stderr is not None

            def copy_stream(source, destination, terminal) -> None:
                for line in source:
                    terminal.write(line)
                    terminal.flush()
                    destination.write(line)

            stdout_thread = threading.Thread(
                target=copy_stream, args=(exercise.stdout, transcript, sys.stdout)
            )
            stderr_thread = threading.Thread(
                target=copy_stream,
                args=(exercise.stderr, stderr_transcript, sys.stderr),
            )
            stdout_thread.start()
            stderr_thread.start()
            exercise_code = exercise.wait()
            stdout_thread.join()
            stderr_thread.join()
    finally:
        camera.send_signal(signal.SIGINT)
        camera_code = camera.wait()
    video = args.output_dir / "nav-burst.mp4"
    evidence = args.output_dir / "evidence.jsonl"
    if camera_code not in (0, 255):
        raise SystemExit(f"camera={camera_code}")
    if (
        not video.exists()
        or video.stat().st_size == 0
        or not evidence.exists()
        or evidence.stat().st_size == 0
    ):
        raise SystemExit("video or evidence output is empty")
    extract_evidence(args.output_dir, video_start_unix_ms)
    if exercise_code != 0:
        raise SystemExit(f"exercise={exercise_code}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
