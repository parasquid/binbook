from __future__ import annotations

from scripts import analyze_timing as timing


SAMPLE_LOG = """
seq=10 tick_ms=1000 level=2 subsystem=2 event=INPUT_DECISION arg0=1 arg1=0 arg2=300
seq=11 tick_ms=1002 level=2 subsystem=2 event=REQUEST_ENQUEUE arg0=0 arg1=7 arg2=0
seq=12 tick_ms=1004 level=2 subsystem=3 event=REQUEST_RECEIVE arg0=0 arg1=7 arg2=2
seq=13 tick_ms=1005 level=2 subsystem=1 event=DISPLAY_REQUEST_START arg0=0 arg1=0 arg2=1
seq=14 tick_ms=1010 level=1 subsystem=1 event=BUSY_WAIT_START arg0=2 arg1=15000 arg2=1
seq=15 tick_ms=1320 level=1 subsystem=1 event=BUSY_WAIT_END arg0=2 arg1=310 arg2=0
seq=16 tick_ms=1900 level=2 subsystem=3 event=PAGE_TURN arg0=0 arg1=1 arg2=0
seq=17 tick_ms=1901 level=2 subsystem=1 event=DISPLAY_REQUEST_END arg0=0 arg1=896 arg2=0
""".strip()

DIAGNOSTIC_PAGE_LOG = """
seq=24 tick_ms=26159 level=2 subsystem=5 event=CMD_RECEIPT arg0=1 arg1=1 arg2=0
seq=25 tick_ms=35032 level=2 subsystem=5 event=CMD_RECEIPT arg0=4 arg1=1 arg2=0
seq=26 tick_ms=42965 level=2 subsystem=5 event=CMD_RECEIPT arg0=3 arg1=1 arg2=0
seq=27 tick_ms=42965 level=2 subsystem=3 event=REQUEST_RECEIVE arg0=1 arg1=1 arg2=-1
seq=28 tick_ms=42965 level=2 subsystem=1 event=DISPLAY_REQUEST_START arg0=1 arg1=0 arg2=1
seq=31 tick_ms=43357 level=1 subsystem=1 event=BUSY_WAIT_START arg0=2 arg1=60000 arg2=1
seq=32 tick_ms=43858 level=1 subsystem=1 event=BUSY_WAIT_END arg0=2 arg1=1 arg2=0
seq=34 tick_ms=43858 level=2 subsystem=3 event=PAGE_TURN arg0=0 arg1=1 arg2=0
seq=37 tick_ms=43859 level=2 subsystem=1 event=DISPLAY_REQUEST_END arg0=1 arg1=894 arg2=0
	""".strip()

BREAKDOWN_PAGE_LOG = """
seq=24 tick_ms=42965 level=2 subsystem=5 event=CMD_RECEIPT arg0=3 arg1=1 arg2=0
seq=25 tick_ms=42965 level=2 subsystem=3 event=REQUEST_RECEIVE arg0=1 arg1=1 arg2=-1
seq=26 tick_ms=42965 level=2 subsystem=1 event=DISPLAY_REQUEST_START arg0=1 arg1=0 arg2=1
seq=27 tick_ms=42968 level=2 subsystem=1 event=PAGE_METADATA_READ arg0=0 arg1=1 arg2=3
seq=28 tick_ms=42969 level=2 subsystem=1 event=PLANE_WRITE_START arg0=0 arg1=1 arg2=48000
seq=29 tick_ms=43029 level=2 subsystem=1 event=PLANE_ROW_FILL_SUMMARY arg0=0 arg1=14 arg2=480
seq=30 tick_ms=43169 level=2 subsystem=1 event=PLANE_SPI_WRITE_SUMMARY arg0=0 arg1=120 arg2=48000
seq=31 tick_ms=43170 level=2 subsystem=1 event=PLANE_WRITE_END arg0=0 arg1=201 arg2=0
seq=32 tick_ms=43171 level=2 subsystem=1 event=PLANE_WRITE_START arg0=1 arg1=0 arg2=48000
seq=33 tick_ms=43229 level=2 subsystem=1 event=PLANE_ROW_FILL_SUMMARY arg0=1 arg1=16 arg2=480
seq=34 tick_ms=43369 level=2 subsystem=1 event=PLANE_SPI_WRITE_SUMMARY arg0=1 arg1=122 arg2=48000
seq=35 tick_ms=43370 level=2 subsystem=1 event=PLANE_WRITE_END arg0=1 arg1=199 arg2=0
seq=36 tick_ms=43390 level=2 subsystem=1 event=REFRESH_TRIGGER arg0=1 arg1=20 arg2=0
seq=37 tick_ms=43391 level=1 subsystem=1 event=BUSY_WAIT_START arg0=2 arg1=60000 arg2=1
seq=38 tick_ms=43865 level=1 subsystem=1 event=BUSY_WAIT_END arg0=2 arg1=474 arg2=0
seq=39 tick_ms=43865 level=2 subsystem=3 event=PAGE_TURN arg0=0 arg1=1 arg2=0
seq=40 tick_ms=43865 level=2 subsystem=1 event=DISPLAY_REQUEST_END arg0=1 arg1=900 arg2=0
""".strip()


def test_parse_cli_log_records_extracts_timing_events() -> None:
    records = timing.parse_log_text(SAMPLE_LOG)

    assert [record.event for record in records] == [
        "INPUT_DECISION",
        "REQUEST_ENQUEUE",
        "REQUEST_RECEIVE",
        "DISPLAY_REQUEST_START",
        "BUSY_WAIT_START",
        "BUSY_WAIT_END",
        "PAGE_TURN",
        "DISPLAY_REQUEST_END",
    ]


def test_parse_cli_log_skips_non_record_key_value_lines() -> None:
    text = "Finished profile=dev\n" + SAMPLE_LOG

    records = timing.parse_log_text(text)

    assert records[0].event == "INPUT_DECISION"


def test_page_turn_summary_computes_stage_durations() -> None:
    [summary] = timing.build_timelines(timing.parse_log_text(SAMPLE_LOG))

    assert summary.input_to_enqueue_ms == 2
    assert summary.enqueue_to_receive_ms == 2
    assert summary.receive_to_display_start_ms == 1
    assert summary.display_request_ms == 896
    assert summary.busy_wait_ms == 310
    assert summary.input_to_page_ms == 900
    assert summary.bottleneck_stage == "display_request"


def test_missing_required_event_reports_incomplete_timeline() -> None:
    incomplete = SAMPLE_LOG.replace("event=PAGE_TURN", "event=REFRESH_PHASE")

    assert timing.build_timelines(timing.parse_log_text(incomplete)) == []


def test_diagnostic_command_page_turn_uses_command_receipt_as_origin() -> None:
    [summary] = timing.build_timelines(timing.parse_log_text(DIAGNOSTIC_PAGE_LOG))

    assert summary.input_to_enqueue_ms == 0
    assert summary.enqueue_to_receive_ms == 0
    assert summary.receive_to_display_start_ms == 0
    assert summary.display_request_ms == 894
    assert summary.busy_wait_ms == 1
    assert summary.input_to_page_ms == 893
    assert summary.bottleneck_stage == "display_request"


def test_diagnostic_command_timing_clamps_out_of_order_stage_ticks() -> None:
    text = DIAGNOSTIC_PAGE_LOG.replace(
        "seq=27 tick_ms=42965 level=2 subsystem=3 event=REQUEST_RECEIVE",
        "seq=27 tick_ms=42962 level=2 subsystem=3 event=REQUEST_RECEIVE",
    )

    [summary] = timing.build_timelines(timing.parse_log_text(text))

    assert summary.enqueue_to_receive_ms == 0


def test_page_turn_summary_splits_non_busy_display_time() -> None:
    [summary] = timing.build_timelines(timing.parse_log_text(BREAKDOWN_PAGE_LOG))

    assert summary.display_request_ms == 900
    assert summary.busy_wait_ms == 474
    assert summary.page_metadata_ms == 3
    assert summary.prev_plane_total_ms == 201
    assert summary.prev_plane_fill_ms == 14
    assert summary.prev_plane_spi_ms == 120
    assert summary.target_plane_total_ms == 199
    assert summary.target_plane_fill_ms == 16
    assert summary.target_plane_spi_ms == 122
    assert summary.refresh_trigger_ms == 20
    assert summary.non_busy_ms == 426
    assert summary.unattributed_ms == 3
