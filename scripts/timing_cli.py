from __future__ import annotations

import subprocess
import sys
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True, slots=True)
class CliArgs:
    log_text: str | None
    capture: bool
    port: str
    since: int
    allow_incomplete: bool


class UsageError(Exception):
    pass


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
