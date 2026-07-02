#!/usr/bin/env python3
# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///
# ─── How to run ───
# uv run python scripts/analyze_timing.py --log-text /tmp/binbook-timing-log.txt

from __future__ import annotations

import statistics
import subprocess
import sys
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path


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
    input_to_page_ms: int
    bottleneck_stage: str


@dataclass(frozen=True, slots=True)
class CliArgs:
    log_text: str | None
    capture: bool
    port: str
    since: int
    allow_incomplete: bool


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


def percentile95(values: list[int]) -> int:
    if len(values) == 1:
        return values[0]
    return int(statistics.quantiles(values, n=20, method="inclusive")[18])


def read_input(args: CliArgs) -> str:
    if args.log_text is not None:
        return Path(args.log_text).read_text(encoding="utf-8")
    if args.capture:
        result = subprocess.run(
            [
                "cargo",
                "run",
                "-p",
                "binbook",
                "--features",
                "serial-device",
                "--",
                "diag",
                "logs",
                "--port",
                args.port,
                "--since",
                str(args.since),
            ],
            check=True,
            text=True,
            capture_output=True,
        )
        return result.stdout
    return sys.stdin.read()


def print_timelines(timelines: list[Timeline]) -> None:
    for index, timeline in enumerate(timelines, start=1):
        line = (
            f"turn={index} input_to_enqueue_ms={timeline.input_to_enqueue_ms} "
            + f"enqueue_to_receive_ms={timeline.enqueue_to_receive_ms} "
            + f"receive_to_display_start_ms={timeline.receive_to_display_start_ms} "
            + f"display_request_ms={timeline.display_request_ms} "
            + f"busy_wait_ms={timeline.busy_wait_ms} "
            + f"input_to_page_ms={timeline.input_to_page_ms} "
            + f"bottleneck={timeline.bottleneck_stage}"
        )
        print(line)
    totals = [timeline.input_to_page_ms for timeline in timelines]
    summary = (
        f"summary count={len(timelines)} min={min(totals)} max={max(totals)} "
        + f"avg={int(statistics.mean(totals))} p95={percentile95(totals)}"
    )
    print(summary)


def parse_args(argv: Sequence[str]) -> CliArgs | None:
    log_text: str | None = None
    capture = False
    port = "/dev/ttyACM0"
    since = 0
    allow_incomplete = False
    index = 0
    while index < len(argv):
        arg = argv[index]
        if arg in {"-h", "--help"}:
            print_help()
            return None
        if arg == "--capture":
            capture = True
            index += 1
            continue
        if arg == "--allow-incomplete":
            allow_incomplete = True
            index += 1
            continue
        if arg in {"--log-text", "--port", "--since"}:
            if index + 1 >= len(argv):
                raise UsageError(f"missing value for {arg}")
            value = argv[index + 1]
            match arg:
                case "--log-text":
                    log_text = value
                case "--port":
                    port = value
                case "--since":
                    since = int(value)
                case unreachable:
                    raise AssertionError(unreachable)
            index += 2
            continue
        raise UsageError(f"unknown argument {arg}")
    return CliArgs(
        log_text=log_text,
        capture=capture,
        port=port,
        since=since,
        allow_incomplete=allow_incomplete,
    )


class UsageError(Exception):
    pass


def print_help() -> None:
    print(
        "usage: analyze_timing.py [--log-text PATH] [--capture] [--port PORT] [--since CURSOR] [--allow-incomplete]"
    )
    print("Analyze BinBook diagnostic timing logs")
    print("--log-text PATH      Path to saved `binbook diag logs` text output")
    print("--capture            Capture logs through the Rust CLI")
    print("--port PORT          Serial port for --capture")
    print("--since CURSOR       Log cursor for --capture")
    print("--allow-incomplete   Return zero without complete timelines")


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
