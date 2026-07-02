from __future__ import annotations

import subprocess
import sys
from pathlib import Path


EVT_PARTIAL_WINDOW_CASE_START = 0x0317
EVT_PARTIAL_WINDOW_GEOMETRY = 0x0318
EVT_PARTIAL_WINDOW_SIZE = 0x0319
EVT_PARTIAL_WINDOW_RAM_WRITE = 0x031A
EVT_PARTIAL_WINDOW_BUSY = 0x031B
EVT_PARTIAL_WINDOW_CASE_END = 0x031C


def write_evidence(path: Path) -> None:
    lines = [
        '{"kind":"run_start","schema_version":1,"rounds":2}',
        '{"kind":"probe_request","schema_version":1,"round":1,"sequence":3}',
        f'{{"kind":"log","schema_version":1,"round":1,"tick_ms":100,"event":{EVT_PARTIAL_WINDOW_CASE_START},"arg0":0,"arg1":1,"arg2":1}}',
        f'{{"kind":"log","schema_version":1,"round":1,"tick_ms":101,"event":{EVT_PARTIAL_WINDOW_GEOMETRY},"arg0":0,"arg1":0,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"round":1,"tick_ms":102,"event":{EVT_PARTIAL_WINDOW_SIZE},"arg0":0,"arg1":48000,"arg2":96000}}',
        f'{{"kind":"log","schema_version":1,"round":1,"tick_ms":103,"event":{EVT_PARTIAL_WINDOW_RAM_WRITE},"arg0":0,"arg1":110,"arg2":111}}',
        f'{{"kind":"log","schema_version":1,"round":1,"tick_ms":600,"event":{EVT_PARTIAL_WINDOW_BUSY},"arg0":0,"arg1":500,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"round":1,"tick_ms":610,"event":{EVT_PARTIAL_WINDOW_CASE_END},"arg0":0,"arg1":510,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"round":1,"tick_ms":700,"event":{EVT_PARTIAL_WINDOW_CASE_START},"arg0":2,"arg1":1,"arg2":1}}',
        f'{{"kind":"log","schema_version":1,"round":1,"tick_ms":701,"event":{EVT_PARTIAL_WINDOW_GEOMETRY},"arg0":2,"arg1":0,"arg2":208}}',
        f'{{"kind":"log","schema_version":1,"round":1,"tick_ms":702,"event":{EVT_PARTIAL_WINDOW_SIZE},"arg0":2,"arg1":6400,"arg2":12800}}',
        f'{{"kind":"log","schema_version":1,"round":1,"tick_ms":703,"event":{EVT_PARTIAL_WINDOW_RAM_WRITE},"arg0":2,"arg1":14,"arg2":15}}',
        f'{{"kind":"log","schema_version":1,"round":1,"tick_ms":940,"event":{EVT_PARTIAL_WINDOW_BUSY},"arg0":2,"arg1":240,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"round":1,"tick_ms":950,"event":{EVT_PARTIAL_WINDOW_CASE_END},"arg0":2,"arg1":250,"arg2":0}}',
        '{"kind":"probe_request","schema_version":1,"round":2,"sequence":4}',
        f'{{"kind":"log","schema_version":1,"round":2,"tick_ms":1000,"event":{EVT_PARTIAL_WINDOW_CASE_START},"arg0":0,"arg1":2,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"round":2,"tick_ms":1500,"event":{EVT_PARTIAL_WINDOW_BUSY},"arg0":0,"arg1":520,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"round":2,"tick_ms":1600,"event":{EVT_PARTIAL_WINDOW_CASE_START},"arg0":2,"arg1":2,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"round":2,"tick_ms":1840,"event":{EVT_PARTIAL_WINDOW_BUSY},"arg0":2,"arg1":260,"arg2":0}}',
        '{"kind":"run_result","schema_version":1,"error_count":0}',
    ]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def test_analyze_partial_window_busy_reports_threshold_decision(tmp_path: Path) -> None:
    evidence = tmp_path / "partial-window-busy.jsonl"
    write_evidence(evidence)

    result = subprocess.run(
        [
            sys.executable,
            "scripts/analyze_partial_window_busy.py",
            "--evidence",
            str(evidence),
        ],
        check=True,
        text=True,
        capture_output=True,
    )

    assert "case=0 name=full busy_avg_ms=510" in result.stdout
    assert "case=2 name=stripe-64 busy_avg_ms=250" in result.stdout
    assert "case=2 name=stripe-64 busy_avg_ms=250 reduction_vs_full_ms=260 useful=true" in result.stdout
    assert "sectioned_refresh_recommendation=promising" in result.stdout
