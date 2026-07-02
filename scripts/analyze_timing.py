#!/usr/bin/env python3
# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///
# ─── How to run ───
# uv run python scripts/analyze_timing.py --log-text /tmp/binbook-timing-log.txt

from __future__ import annotations

import sys
from dataclasses import dataclass
from pathlib import Path

if __package__ in {None, ""}:
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from scripts.timing_breakdown import build_display_breakdown
from scripts.timing_cli import UsageError, parse_args, read_input
from scripts.timing_report import print_timelines


@dataclass(frozen=True, slots=True)
class LogRecord:
    sequence: int
    tick_ms: int
    level: int
    subsystem: int
    event: str
    arg0: int
    arg1: int
    arg2: int


@dataclass(frozen=True, slots=True)
class Timeline:
    input_to_enqueue_ms: int
    enqueue_to_receive_ms: int
    receive_to_display_start_ms: int
    display_request_ms: int
    busy_wait_ms: int
    page_metadata_ms: int | None
    prev_plane_total_ms: int | None
    prev_plane_fill_ms: int | None
    prev_plane_spi_ms: int | None
    target_plane_total_ms: int | None
    target_plane_fill_ms: int | None
    target_plane_spi_ms: int | None
    refresh_trigger_ms: int | None
    non_busy_ms: int
    unattributed_ms: int | None
    input_to_page_ms: int
    bottleneck_stage: str


def parse_log_text(text: str) -> list[LogRecord]:
    records: list[LogRecord] = []
    for line in text.splitlines():
        record = parse_record_line(line)
        if record is None:
            continue
        records.append(record)
    return records


def parse_record_line(line: str) -> LogRecord | None:
    fields: dict[str, str] = {}
    for token in line.split():
        if "=" not in token:
            continue
        key, value = token.split("=", 1)
        fields[key] = value
    if not required_log_fields_present(fields):
        return None
    return LogRecord(
        sequence=int(fields["seq"]),
        tick_ms=int(fields["tick_ms"]),
        level=int(fields["level"]),
        subsystem=int(fields["subsystem"]),
        event=fields["event"],
        arg0=int(fields["arg0"]),
        arg1=int(fields["arg1"]),
        arg2=int(fields["arg2"]),
    )


def required_log_fields_present(fields: dict[str, str]) -> bool:
    return all(
        key in fields
        for key in (
            "seq",
            "tick_ms",
            "level",
            "subsystem",
            "event",
            "arg0",
            "arg1",
            "arg2",
        )
    )


def build_timelines(records: list[LogRecord]) -> list[Timeline]:
    timelines: list[Timeline] = []
    for page_index, page_turn in enumerate(records):
        if page_turn.event != "PAGE_TURN":
            continue
        before_page = records[:page_index]
        input_decision = last_event(before_page, "INPUT_DECISION")
        enqueue = last_event(before_page, "REQUEST_ENQUEUE")
        receive = last_event(before_page, "REQUEST_RECEIVE")
        display_start = last_event(before_page, "DISPLAY_REQUEST_START")
        display_end = first_event(records[page_index + 1 :], "DISPLAY_REQUEST_END")
        if input_decision is None or enqueue is None:
            command_receipt = last_event(before_page, "CMD_RECEIPT")
            if command_receipt is None:
                continue
            input_decision = command_receipt
            enqueue = command_receipt
        if receive is None or display_start is None or display_end is None:
            continue
        busy_wait_ms = sum(
            record.arg1
            for record in records
            if record.event == "BUSY_WAIT_END"
            and display_start.tick_ms <= record.tick_ms <= display_end.tick_ms
        )
        breakdown = build_display_breakdown(
            records,
            display_start.tick_ms,
            display_end.tick_ms,
            display_end.arg1,
            busy_wait_ms,
        )
        input_to_enqueue_ms = elapsed_ms(input_decision.tick_ms, enqueue.tick_ms)
        enqueue_to_receive_ms = elapsed_ms(enqueue.tick_ms, receive.tick_ms)
        receive_to_display_start_ms = elapsed_ms(receive.tick_ms, display_start.tick_ms)
        input_to_page_ms = elapsed_ms(input_decision.tick_ms, page_turn.tick_ms)
        timelines.append(
            Timeline(
                input_to_enqueue_ms=input_to_enqueue_ms,
                enqueue_to_receive_ms=enqueue_to_receive_ms,
                receive_to_display_start_ms=receive_to_display_start_ms,
                display_request_ms=display_end.arg1,
                busy_wait_ms=busy_wait_ms,
                page_metadata_ms=breakdown.page_metadata_ms,
                prev_plane_total_ms=breakdown.prev_plane_total_ms,
                prev_plane_fill_ms=breakdown.prev_plane_fill_ms,
                prev_plane_spi_ms=breakdown.prev_plane_spi_ms,
                target_plane_total_ms=breakdown.target_plane_total_ms,
                target_plane_fill_ms=breakdown.target_plane_fill_ms,
                target_plane_spi_ms=breakdown.target_plane_spi_ms,
                refresh_trigger_ms=breakdown.refresh_trigger_ms,
                non_busy_ms=breakdown.non_busy_ms,
                unattributed_ms=breakdown.unattributed_ms,
                input_to_page_ms=input_to_page_ms,
                bottleneck_stage=bottleneck_stage(
                    [
                        ("input_to_enqueue", input_to_enqueue_ms),
                        ("enqueue_to_receive", enqueue_to_receive_ms),
                        ("receive_to_display_start", receive_to_display_start_ms),
                        ("display_request", display_end.arg1),
                        ("busy_wait", busy_wait_ms),
                    ]
                ),
            )
        )
    return timelines


def elapsed_ms(start_ms: int, end_ms: int) -> int:
    return max(0, end_ms - start_ms)


def last_event(records: list[LogRecord], event: str) -> LogRecord | None:
    for record in reversed(records):
        if record.event == event:
            return record
    return None


def first_event(records: list[LogRecord], event: str) -> LogRecord | None:
    for record in records:
        if record.event == event:
            return record
    return None


def bottleneck_stage(stages: list[tuple[str, int]]) -> str:
    return max(stages, key=lambda item: item[1])[0]


def main() -> int:
    try:
        args = parse_args(sys.argv[1:])
    except UsageError as error:
        print(str(error), file=sys.stderr)
        return 2
    if args is None:
        return 0
    timelines = build_timelines(parse_log_text(read_input(args)))
    if not timelines:
        print("no complete timing timelines found", file=sys.stderr)
        return 0 if args.allow_incomplete else 1
    print_timelines(timelines)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
