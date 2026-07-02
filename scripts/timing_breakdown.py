from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass
from typing import Protocol


class TimingLogRecord(Protocol):
    @property
    def tick_ms(self) -> int: ...

    @property
    def event(self) -> str: ...

    @property
    def arg0(self) -> int: ...

    @property
    def arg1(self) -> int: ...

    @property
    def arg2(self) -> int: ...


@dataclass(frozen=True, slots=True)
class DisplayBreakdown:
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


def build_display_breakdown(
    records: Sequence[TimingLogRecord],
    start_ms: int,
    end_ms: int,
    display_request_ms: int,
    busy_wait_ms: int,
) -> DisplayBreakdown:
    window = records_in_window(records, start_ms, end_ms)
    page_metadata_ms = first_arg2(window, "PAGE_METADATA_READ")
    prev_plane_total_ms = first_arg1_for_role(window, "PLANE_WRITE_END", 0)
    prev_plane_fill_ms = first_arg1_for_role(window, "PLANE_ROW_FILL_SUMMARY", 0)
    prev_plane_spi_ms = first_arg1_for_role(window, "PLANE_SPI_WRITE_SUMMARY", 0)
    target_plane_total_ms = first_arg1_for_role(window, "PLANE_WRITE_END", 1)
    target_plane_fill_ms = first_arg1_for_role(window, "PLANE_ROW_FILL_SUMMARY", 1)
    target_plane_spi_ms = first_arg1_for_role(window, "PLANE_SPI_WRITE_SUMMARY", 1)
    refresh_trigger_ms = first_arg1(window, "REFRESH_TRIGGER")
    non_busy_ms = max(0, display_request_ms - busy_wait_ms)
    attributed_ms = optional_sum(
        page_metadata_ms,
        prev_plane_total_ms,
        target_plane_total_ms,
        refresh_trigger_ms,
    )
    unattributed_ms = (
        max(0, non_busy_ms - attributed_ms) if attributed_ms is not None else None
    )
    return DisplayBreakdown(
        page_metadata_ms=page_metadata_ms,
        prev_plane_total_ms=prev_plane_total_ms,
        prev_plane_fill_ms=prev_plane_fill_ms,
        prev_plane_spi_ms=prev_plane_spi_ms,
        target_plane_total_ms=target_plane_total_ms,
        target_plane_fill_ms=target_plane_fill_ms,
        target_plane_spi_ms=target_plane_spi_ms,
        refresh_trigger_ms=refresh_trigger_ms,
        non_busy_ms=non_busy_ms,
        unattributed_ms=unattributed_ms,
    )


def records_in_window(
    records: Sequence[TimingLogRecord], start_ms: int, end_ms: int
) -> list[TimingLogRecord]:
    return [record for record in records if start_ms <= record.tick_ms <= end_ms]


def first_arg1(records: Sequence[TimingLogRecord], event: str) -> int | None:
    record = first_event(records, event)
    if record is None:
        return None
    return record.arg1


def first_arg2(records: Sequence[TimingLogRecord], event: str) -> int | None:
    record = first_event(records, event)
    if record is None:
        return None
    return record.arg2


def first_arg1_for_role(
    records: Sequence[TimingLogRecord], event: str, role: int
) -> int | None:
    for record in records:
        if record.event == event and record.arg0 == role:
            return record.arg1
    return None


def optional_sum(*values: int | None) -> int | None:
    total = 0
    for value in values:
        if value is None:
            return None
        total += value
    return total


def first_event(
    records: Sequence[TimingLogRecord], event: str
) -> TimingLogRecord | None:
    for record in records:
        if record.event == event:
            return record
    return None
