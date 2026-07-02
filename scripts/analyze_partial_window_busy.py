#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import statistics
from collections import defaultdict
from pathlib import Path
from typing import Any


EVT_PARTIAL_WINDOW_BUSY = 0x031B
USEFUL_THRESHOLD_MS = 300

CASE_NAMES = {
    0: "full",
    1: "half-height",
    2: "stripe-64",
    3: "strip-16",
    4: "center-box",
    5: "page-number-box",
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--evidence", required=True)
    args = parser.parse_args()
    records = read_jsonl(Path(args.evidence))
    reports = build_reports(records)
    if not reports:
        print("no partial-window busy records found")
        return 1
    print_reports(reports)
    return 0


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    return [
        json.loads(line)
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]


def build_reports(records: list[dict[str, Any]]) -> dict[int, list[int]]:
    busy_by_case: dict[int, list[int]] = defaultdict(list)
    for record in records:
        if record.get("kind") != "log":
            continue
        if int(record.get("event", -1)) != EVT_PARTIAL_WINDOW_BUSY:
            continue
        case = int(record["arg0"])
        busy_ms = int(record["arg1"])
        status = int(record.get("arg2", -1))
        if status == 0:
            busy_by_case[case].append(busy_ms)
    return dict(sorted(busy_by_case.items()))


def print_reports(reports: dict[int, list[int]]) -> None:
    full_avg = average(reports.get(0, []))
    promising = False
    for case, values in reports.items():
        name = CASE_NAMES.get(case, f"case-{case}")
        avg = average(values)
        reduction = full_avg - avg if full_avg is not None else 0
        useful = case != 0 and avg < USEFUL_THRESHOLD_MS
        promising = promising or useful
        print(
            f"case={case} name={name} "
            f"busy_avg_ms={avg} reduction_vs_full_ms={reduction} useful={str(useful).lower()} "
            f"busy_min_ms={min(values)} busy_max_ms={max(values)} samples={len(values)}"
        )
    print(
        "sectioned_refresh_recommendation="
        + ("promising" if promising else "not_promising")
    )


def average(values: list[int]) -> int:
    return round(statistics.mean(values))


if __name__ == "__main__":
    raise SystemExit(main())
