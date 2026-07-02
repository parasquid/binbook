from __future__ import annotations

import statistics
from collections.abc import Sequence
from typing import Protocol


class TimingSummary(Protocol):
    @property
    def input_to_enqueue_ms(self) -> int: ...

    @property
    def enqueue_to_receive_ms(self) -> int: ...

    @property
    def receive_to_display_start_ms(self) -> int: ...

    @property
    def display_request_ms(self) -> int: ...

    @property
    def busy_wait_ms(self) -> int: ...

    @property
    def page_metadata_ms(self) -> int | None: ...

    @property
    def prev_plane_total_ms(self) -> int | None: ...

    @property
    def prev_plane_fill_ms(self) -> int | None: ...

    @property
    def prev_plane_spi_ms(self) -> int | None: ...

    @property
    def target_plane_total_ms(self) -> int | None: ...

    @property
    def target_plane_fill_ms(self) -> int | None: ...

    @property
    def target_plane_spi_ms(self) -> int | None: ...

    @property
    def refresh_trigger_ms(self) -> int | None: ...

    @property
    def non_busy_ms(self) -> int: ...

    @property
    def unattributed_ms(self) -> int | None: ...

    @property
    def input_to_page_ms(self) -> int: ...

    @property
    def bottleneck_stage(self) -> str: ...


def print_timelines(timelines: Sequence[TimingSummary]) -> None:
    for index, timeline in enumerate(timelines, start=1):
        line = (
            f"turn={index} input_to_enqueue_ms={timeline.input_to_enqueue_ms} "
            + f"enqueue_to_receive_ms={timeline.enqueue_to_receive_ms} "
            + f"receive_to_display_start_ms={timeline.receive_to_display_start_ms} "
            + f"display_request_ms={timeline.display_request_ms} "
            + f"busy_wait_ms={timeline.busy_wait_ms} "
            + f"page_metadata_ms={format_optional(timeline.page_metadata_ms)} "
            + f"prev_plane_total_ms={format_optional(timeline.prev_plane_total_ms)} "
            + f"prev_plane_fill_ms={format_optional(timeline.prev_plane_fill_ms)} "
            + f"prev_plane_spi_ms={format_optional(timeline.prev_plane_spi_ms)} "
            + f"target_plane_total_ms={format_optional(timeline.target_plane_total_ms)} "
            + f"target_plane_fill_ms={format_optional(timeline.target_plane_fill_ms)} "
            + f"target_plane_spi_ms={format_optional(timeline.target_plane_spi_ms)} "
            + f"refresh_trigger_ms={format_optional(timeline.refresh_trigger_ms)} "
            + f"non_busy_ms={timeline.non_busy_ms} "
            + f"unattributed_ms={format_optional(timeline.unattributed_ms)} "
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


def format_optional(value: int | None) -> str:
    if value is None:
        return "NA"
    return str(value)


def percentile95(values: Sequence[int]) -> int:
    if len(values) == 1:
        return values[0]
    return int(statistics.quantiles(values, n=20, method="inclusive")[18])
