from __future__ import annotations

import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw


def write_evidence(path: Path) -> None:
    lines = [
        '{"kind":"run_start","schema_version":1,"host_unix_ms":1000000}',
        '{"kind":"trial_start","schema_version":1,"trial":1,"gap_ms":100,"host_unix_ms":1000000}',
        '{"kind":"key_sent","schema_version":1,"trial":1,"gap_ms":100,"turn_index":0,"sequence":1,"host_unix_ms":1000100}',
        '{"kind":"key_sent","schema_version":1,"trial":1,"gap_ms":100,"turn_index":1,"sequence":2,"host_unix_ms":1000200}',
        '{"kind":"trial_result","schema_version":1,"trial":1,"gap_ms":100,"all_ok":true}',
        '{"kind":"trial_start","schema_version":1,"trial":2,"gap_ms":300,"host_unix_ms":1001000}',
        '{"kind":"key_sent","schema_version":1,"trial":2,"gap_ms":300,"turn_index":0,"sequence":3,"host_unix_ms":1001100}',
        '{"kind":"key_sent","schema_version":1,"trial":2,"gap_ms":300,"turn_index":1,"sequence":4,"host_unix_ms":1001400}',
        '{"kind":"trial_result","schema_version":1,"trial":2,"gap_ms":300,"all_ok":true}',
    ]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_frames(path: Path) -> None:
    path.mkdir()
    # 10 fps, video starts at 1_000_000 ms. Trial 1 has one visual transition
    # after two keys, so gap 100 fails. Trial 2 has two distinct transitions.
    pages = [
        "00",
        "00",
        "00",
        "01",
        "01",
        "01",
        "01",
        "01",
        "01",
        "01",
        "01",
        "01",
        "02",
        "02",
        "02",
        "03",
        "03",
        "03",
    ]
    for index, page in enumerate(pages):
        image = Image.new("L", (120, 60), 235)
        draw = ImageDraw.Draw(image)
        draw.rectangle((0, 0, 119, 59), outline=20)
        draw.text((24, 18), f"PAGE {page}", fill=20)
        if page in {"01", "03"}:
            draw.rectangle((86, 10, 108, 48), fill=20)
        if page in {"02", "03"}:
            draw.rectangle((10, 10, 32, 48), fill=20)
        image.save(path / f"frame_{index:06d}.png")


def test_visual_cadence_reports_fastest_gap_with_distinct_transitions(tmp_path: Path) -> None:
    evidence = tmp_path / "evidence.jsonl"
    frames = tmp_path / "frames"
    write_evidence(evidence)
    write_frames(frames)

    result = subprocess.run(
        [
            sys.executable,
            "scripts/analyze_visual_cadence.py",
            "--evidence",
            str(evidence),
            "--frames-dir",
            str(frames),
            "--video-start-unix-ms",
            "1000000",
            "--fps",
            "10",
            "--crop",
            "0,0,120,60",
            "--threshold",
            "2",
            "--min-transition-gap-ms",
            "150",
        ],
        check=True,
        text=True,
        capture_output=True,
    )

    assert "gap_ms=100 visual_ok=false transitions=1/2" in result.stdout
    assert "gap_ms=300 visual_ok=true transitions=2/2" in result.stdout
    assert "fastest_visual_gap_ms=300" in result.stdout
