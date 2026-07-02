#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import statistics
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any

EVT_PAGE_TURN = 0x0201
EVT_CMD_RECEIPT = 0x0010
EVT_TURN_DEQUEUED = 0x0203
EVT_TURN_DROPPED = 0x0204
EVT_REQUEST_RECEIVE = 0x0208
EVT_GRAY_OVERLAY_START = 0x0306
EVT_BW_REFRESH_START = 0x0314
EVT_BW_BUSY_ASSERTED = 0x0315
EVT_BW_BUSY_READY = 0x0316
EVT_RENDER_FAILURE = 0x0302
EVT_DISPLAY_ERROR = 0x0800
EVT_DISPLAY_RECOVERY = 0x0801


@dataclass(frozen=True, slots=True)
class TurnTiming:
    sequence: int
    target_page: int | None
    cmd_receipt_tick: int | None
    display_dequeue_tick: int | None
    bw_start_tick: int | None
    bw_busy_asserted_tick: int | None
    bw_busy_ready_tick: int | None
    page_turn_tick: int | None
    gray_start_tick: int | None


@dataclass(frozen=True, slots=True)
class TrialReport:
    gap_ms: int
    all_ok: bool
    next_key_after_bw_before_gray: bool
    bw_start_before_next_key: bool
    bw_busy_asserted_before_next_key: bool
    serial_accept_after_bw_busy_asserted: bool
    display_dequeue_after_bw_busy_asserted: bool
    queued_before_bw_busy_asserted: int
    receipt_to_dequeue_ms: list[int]
    dequeue_to_bw_start_ms: list[int]
    bw_start_to_busy_assert_ms: list[int]
    busy_assert_to_busy_ready_ms: list[int]
    busy_ready_to_page_turn_ms: list[int]
    page_turn_to_gray_start_ms: list[int]
    turn_to_turn_ms: list[int]
    response_ms: list[int]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--evidence", required=True)
    args = parser.parse_args()
    reports = build_reports(read_jsonl(Path(args.evidence)))
    if not reports:
        print("no cadence trials found")
        return 1
    print_reports(reports)
    return 0


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    return [
        json.loads(line)
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]


def build_reports(records: list[dict[str, Any]]) -> list[TrialReport]:
    by_trial: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for record in records:
        trial = record.get("trial")
        if isinstance(trial, int):
            by_trial[trial].append(record)
    return [build_trial_report(items) for _, items in sorted(by_trial.items())]


def build_trial_report(records: list[dict[str, Any]]) -> TrialReport:
    gap_ms = int(first_value(records, "gap_ms", 0))
    trial_result = first_kind(records, "trial_result")
    logs = [record for record in records if record.get("kind") == "log"]
    key_sequences = [
        int(record["sequence"])
        for record in sorted(
            (record for record in records if record.get("kind") == "key_sent"),
            key=lambda record: int(record.get("turn_index", 0)),
        )
    ]
    timings = build_turn_timings(logs, key_sequences)
    responses = [
        int(record["response_elapsed_ms"])
        for record in records
        if record.get("kind") == "key_response"
    ]
    page_turns = sorted(
        [record for record in logs if int(record.get("event", -1)) == EVT_PAGE_TURN],
        key=lambda record: int(record["tick_ms"]),
    )
    turn_to_turn_ms = [
        int(later["tick_ms"]) - int(earlier["tick_ms"])
        for earlier, later in zip(page_turns, page_turns[1:])
    ]
    all_ok = bool(trial_result and trial_result.get("all_ok") is True)
    if any(
        int(record.get("event", -1))
        in {EVT_TURN_DROPPED, EVT_RENDER_FAILURE, EVT_DISPLAY_ERROR, EVT_DISPLAY_RECOVERY}
        for record in logs
    ):
        all_ok = False
    return TrialReport(
        gap_ms=gap_ms,
        all_ok=all_ok,
        next_key_after_bw_before_gray=receipts_are_after_bw_before_gray(logs),
        bw_start_before_next_key=bw_starts_are_before_next_key(logs),
        bw_busy_asserted_before_next_key=events_are_before_next_key(
            logs, EVT_BW_BUSY_ASSERTED
        ),
        serial_accept_after_bw_busy_asserted=ticks_are_after_previous_activation(
            timings, "cmd_receipt_tick"
        ),
        display_dequeue_after_bw_busy_asserted=ticks_are_after_previous_activation(
            timings, "display_dequeue_tick"
        ),
        queued_before_bw_busy_asserted=count_ticks_before_previous_activation(
            timings, "cmd_receipt_tick"
        ),
        receipt_to_dequeue_ms=timing_deltas(
            timings, "cmd_receipt_tick", "display_dequeue_tick"
        ),
        dequeue_to_bw_start_ms=timing_deltas(
            timings, "display_dequeue_tick", "bw_start_tick"
        ),
        bw_start_to_busy_assert_ms=timing_deltas(
            timings, "bw_start_tick", "bw_busy_asserted_tick"
        ),
        busy_assert_to_busy_ready_ms=timing_deltas(
            timings, "bw_busy_asserted_tick", "bw_busy_ready_tick"
        ),
        busy_ready_to_page_turn_ms=timing_deltas(
            timings, "bw_busy_ready_tick", "page_turn_tick"
        ),
        page_turn_to_gray_start_ms=timing_deltas(
            timings, "page_turn_tick", "gray_start_tick"
        ),
        turn_to_turn_ms=turn_to_turn_ms,
        response_ms=responses,
    )


def build_turn_timings(
    logs: list[dict[str, Any]], key_sequences: list[int]
) -> list[TurnTiming]:
    by_event = records_by_event(logs)
    completions = records_by_arg0(by_event.get(EVT_TURN_DEQUEUED, []))
    page_turns_by_page = records_by_arg1(by_event.get(EVT_PAGE_TURN, []))
    gray_starts = sorted(
        by_event.get(EVT_GRAY_OVERLAY_START, []), key=lambda record: int(record["tick_ms"])
    )
    timings = []
    for sequence in key_sequences:
        completion = completions.get(sequence)
        target_page = int(completion["arg1"]) if completion is not None else None
        page_turn = (
            page_turns_by_page.get(target_page) if target_page is not None else None
        )
        page_turn_tick = int(page_turn["tick_ms"]) if page_turn is not None else None
        gray_start_tick = None
        if target_page is not None and page_turn_tick is not None:
            gray_start_tick = first_tick(
                record
                for record in gray_starts
                if int(record["arg0"]) == target_page
                and int(record["tick_ms"]) > page_turn_tick
            )
        timings.append(
            TurnTiming(
                sequence=sequence,
                target_page=target_page,
                cmd_receipt_tick=event_tick_for_sequence(
                    by_event.get(EVT_CMD_RECEIPT, []), sequence, "arg1"
                ),
                display_dequeue_tick=event_tick_for_sequence(
                    by_event.get(EVT_REQUEST_RECEIVE, []), sequence, "arg1"
                ),
                bw_start_tick=event_tick_for_sequence(
                    by_event.get(EVT_BW_REFRESH_START, []), sequence, "arg0"
                ),
                bw_busy_asserted_tick=event_tick_for_sequence(
                    by_event.get(EVT_BW_BUSY_ASSERTED, []), sequence, "arg0"
                ),
                bw_busy_ready_tick=event_tick_for_sequence(
                    by_event.get(EVT_BW_BUSY_READY, []), sequence, "arg0"
                ),
                page_turn_tick=page_turn_tick,
                gray_start_tick=gray_start_tick,
            )
        )
    return timings


def records_by_event(logs: list[dict[str, Any]]) -> dict[int, list[dict[str, Any]]]:
    grouped: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for record in logs:
        grouped[int(record.get("event", -1))].append(record)
    return grouped


def records_by_arg0(records: list[dict[str, Any]]) -> dict[int, dict[str, Any]]:
    return {int(record["arg0"]): record for record in records}


def records_by_arg1(records: list[dict[str, Any]]) -> dict[int, dict[str, Any]]:
    return {int(record["arg1"]): record for record in records}


def event_tick_for_sequence(
    records: list[dict[str, Any]], sequence: int, field: str
) -> int | None:
    return first_tick(record for record in records if int(record[field]) == sequence)


def first_tick(records: Any) -> int | None:
    for record in sorted(records, key=lambda item: int(item["tick_ms"])):
        return int(record["tick_ms"])
    return None


def ticks_are_after_previous_activation(timings: list[TurnTiming], field: str) -> bool:
    if len(timings) < 2:
        return False
    for previous, current in zip(timings, timings[1:]):
        previous_tick = previous.bw_busy_asserted_tick
        current_tick = getattr(current, field)
        if previous_tick is None or current_tick is None:
            return False
        if current_tick <= previous_tick:
            return False
    return True


def count_ticks_before_previous_activation(timings: list[TurnTiming], field: str) -> int:
    count = 0
    for previous, current in zip(timings, timings[1:]):
        previous_tick = previous.bw_busy_asserted_tick
        current_tick = getattr(current, field)
        if previous_tick is not None and current_tick is not None and current_tick <= previous_tick:
            count += 1
    return count


def timing_deltas(timings: list[TurnTiming], start: str, end: str) -> list[int]:
    deltas = []
    for timing in timings:
        start_tick = getattr(timing, start)
        end_tick = getattr(timing, end)
        if start_tick is not None and end_tick is not None:
            deltas.append(end_tick - start_tick)
    return deltas


def receipts_are_after_bw_before_gray(logs: list[dict[str, Any]]) -> bool:
    completions = sorted(
        [
            record
            for record in logs
            if int(record.get("event", -1)) == EVT_TURN_DEQUEUED
        ],
        key=lambda record: int(record["tick_ms"]),
    )
    if len(completions) < 2:
        return True
    page_turns = [record for record in logs if int(record.get("event", -1)) == EVT_PAGE_TURN]
    receipts = [record for record in logs if int(record.get("event", -1)) == EVT_CMD_RECEIPT]
    gray_starts = [
        int(record["tick_ms"])
        for record in logs
        if int(record.get("event", -1)) == EVT_GRAY_OVERLAY_START
    ]
    for previous, next_completion in zip(completions, completions[1:]):
        previous_page = int(previous["arg1"])
        previous_turn_tick = first_page_turn_tick(page_turns, previous_page)
        next_sequence = int(next_completion["arg0"])
        next_receipt_tick = first_receipt_tick(receipts, next_sequence)
        if previous_turn_tick is None or next_receipt_tick is None:
            return False
        if next_receipt_tick <= previous_turn_tick:
            return False
        next_gray_tick = min(
            (tick for tick in gray_starts if tick > previous_turn_tick),
            default=None,
        )
        if next_gray_tick is not None and next_receipt_tick >= next_gray_tick:
            return False
    return True


def bw_starts_are_before_next_key(logs: list[dict[str, Any]]) -> bool:
    return events_are_before_next_key(logs, EVT_BW_REFRESH_START)


def events_are_before_next_key(logs: list[dict[str, Any]], event: int) -> bool:
    starts = sorted(
        [
            record
            for record in logs
            if int(record.get("event", -1)) == event
        ],
        key=lambda record: int(record["tick_ms"]),
    )
    if len(starts) < 2:
        return False
    receipts = [record for record in logs if int(record.get("event", -1)) == EVT_CMD_RECEIPT]
    for current, next_start in zip(starts, starts[1:]):
        sequence = int(current["arg0"])
        next_sequence = int(next_start["arg0"])
        if sequence >= 0 and next_sequence >= 0 and next_sequence <= sequence:
            return False
        next_receipt_tick = first_receipt_tick(receipts, next_sequence)
        if next_receipt_tick is None:
            return False
        if int(current["tick_ms"]) >= next_receipt_tick:
            return False
    return True


def first_page_turn_tick(records: list[dict[str, Any]], page: int) -> int | None:
    for record in sorted(records, key=lambda item: int(item["tick_ms"])):
        if int(record["arg1"]) == page:
            return int(record["tick_ms"])
    return None


def first_receipt_tick(records: list[dict[str, Any]], sequence: int) -> int | None:
    for record in sorted(records, key=lambda item: int(item["tick_ms"])):
        if int(record["arg1"]) == sequence:
            return int(record["tick_ms"])
    return None


def first_kind(records: list[dict[str, Any]], kind: str) -> dict[str, Any] | None:
    for record in records:
        if record.get("kind") == kind:
            return record
    return None


def first_value(records: list[dict[str, Any]], key: str, default: Any) -> Any:
    for record in records:
        if key in record:
            return record[key]
    return default


def print_reports(reports: list[TrialReport]) -> None:
    for gap_ms, items in group_by_gap(reports).items():
        all_ok = all(item.all_ok for item in items)
        after_bw_before_gray = all(item.next_key_after_bw_before_gray for item in items)
        turn_spacing = [
            value for item in items for value in item.turn_to_turn_ms
        ]
        responses = [value for item in items for value in item.response_ms]
        receipt_to_dequeue = [
            value for item in items for value in item.receipt_to_dequeue_ms
        ]
        dequeue_to_bw_start = [
            value for item in items for value in item.dequeue_to_bw_start_ms
        ]
        bw_start_to_busy_assert = [
            value for item in items for value in item.bw_start_to_busy_assert_ms
        ]
        busy_assert_to_busy_ready = [
            value for item in items for value in item.busy_assert_to_busy_ready_ms
        ]
        busy_ready_to_page_turn = [
            value for item in items for value in item.busy_ready_to_page_turn_ms
        ]
        page_turn_to_gray_start = [
            value for item in items for value in item.page_turn_to_gray_start_ms
        ]
        print(
            f"gap_ms={gap_ms} all_ok={format_bool(all_ok)} "
            f"next_key_after_bw_before_gray={format_bool(after_bw_before_gray)} "
            f"bw_start_before_next_key={format_bool(all(item.bw_start_before_next_key for item in items))} "
            f"bw_busy_asserted_before_next_key={format_bool(all(item.bw_busy_asserted_before_next_key for item in items))} "
            f"serial_accept_after_bw_busy_asserted={format_bool(all(item.serial_accept_after_bw_busy_asserted for item in items))} "
            f"display_dequeue_after_bw_busy_asserted={format_bool(all(item.display_dequeue_after_bw_busy_asserted for item in items))} "
            f"queued_before_bw_busy_asserted={sum(item.queued_before_bw_busy_asserted for item in items)} "
            f"turn_to_turn_avg_ms={format_optional_mean(turn_spacing)} "
            f"response_avg_ms={format_optional_mean(responses)} "
            f"delta_receipt_to_dequeue_avg_ms={format_optional_mean(receipt_to_dequeue)} "
            f"delta_dequeue_to_bw_start_avg_ms={format_optional_mean(dequeue_to_bw_start)} "
            f"delta_bw_start_to_busy_assert_avg_ms={format_optional_mean(bw_start_to_busy_assert)} "
            f"delta_busy_assert_to_busy_ready_avg_ms={format_optional_mean(busy_assert_to_busy_ready)} "
            f"delta_busy_ready_to_page_turn_avg_ms={format_optional_mean(busy_ready_to_page_turn)} "
            f"delta_page_turn_to_gray_start_avg_ms={format_optional_mean(page_turn_to_gray_start)}"
        )
    reliable = [
        report.gap_ms
        for report in reports
        if report.all_ok and report.next_key_after_bw_before_gray
    ]
    if reliable:
        print(f"fastest_reliable_gap_ms={min(reliable)}")
    else:
        print("fastest_reliable_gap_ms=NA")
    bw_reliable = [
        report.gap_ms
        for report in reports
        if report.bw_start_before_next_key
    ]
    if bw_reliable:
        print(f"fastest_bw_start_gap_ms={min(bw_reliable)}")
    else:
        print("fastest_bw_start_gap_ms=NA")
    busy_reliable = [
        report.gap_ms
        for report in reports
        if report.bw_busy_asserted_before_next_key
    ]
    if busy_reliable:
        print(f"fastest_bw_busy_asserted_gap_ms={min(busy_reliable)}")
    else:
        print("fastest_bw_busy_asserted_gap_ms=NA")
    serial_accept_reliable = [
        report.gap_ms
        for report in reports
        if report.serial_accept_after_bw_busy_asserted
    ]
    if serial_accept_reliable:
        print(
            "fastest_serial_accept_after_bw_busy_asserted_gap_ms="
            f"{min(serial_accept_reliable)}"
        )
    else:
        print("fastest_serial_accept_after_bw_busy_asserted_gap_ms=NA")
    display_dequeue_reliable = [
        report.gap_ms
        for report in reports
        if report.display_dequeue_after_bw_busy_asserted
    ]
    if display_dequeue_reliable:
        print(
            "fastest_display_dequeue_after_bw_busy_asserted_gap_ms="
            f"{min(display_dequeue_reliable)}"
        )
    else:
        print("fastest_display_dequeue_after_bw_busy_asserted_gap_ms=NA")


def group_by_gap(reports: list[TrialReport]) -> dict[int, list[TrialReport]]:
    grouped: dict[int, list[TrialReport]] = defaultdict(list)
    for report in reports:
        grouped[report.gap_ms].append(report)
    return dict(sorted(grouped.items()))


def format_bool(value: bool) -> str:
    return "true" if value else "false"


def format_optional_mean(values: list[int]) -> str:
    if not values:
        return "NA"
    return str(int(statistics.mean(values)))


if __name__ == "__main__":
    raise SystemExit(main())
