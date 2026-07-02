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
