from __future__ import annotations

import subprocess
import sys
from pathlib import Path


EVT_PAGE_TURN = 0x0201
EVT_TURN_DEQUEUED = 0x0203
EVT_REQUEST_RECEIVE = 0x0208
EVT_GRAY_OVERLAY_START = 0x0306
EVT_CMD_RECEIPT = 0x0010
EVT_BW_REFRESH_START = 0x0314
EVT_BW_BUSY_ASSERTED = 0x0315
EVT_BW_BUSY_READY = 0x0316


def write_evidence(path: Path) -> None:
    lines = [
        '{"kind":"run_start","schema_version":1,"rounds":1}',
        '{"kind":"trial_start","schema_version":1,"trial":1,"gap_ms":0}',
        '{"kind":"key_sent","schema_version":1,"trial":1,"gap_ms":0,"turn_index":0,"sequence":3,"host_elapsed_ms":0}',
        '{"kind":"key_response","schema_version":1,"trial":1,"gap_ms":0,"turn_index":0,"sequence":3,"response_elapsed_ms":15}',
        '{"kind":"key_sent","schema_version":1,"trial":1,"gap_ms":0,"turn_index":1,"sequence":4,"host_elapsed_ms":0}',
        '{"kind":"key_response","schema_version":1,"trial":1,"gap_ms":0,"turn_index":1,"sequence":4,"response_elapsed_ms":375}',
        f'{{"kind":"log","schema_version":1,"trial":1,"gap_ms":0,"tick_ms":100,"event":{EVT_CMD_RECEIPT},"arg0":5,"arg1":3,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"trial":1,"gap_ms":0,"tick_ms":105,"event":{EVT_CMD_RECEIPT},"arg0":5,"arg1":4,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"trial":1,"gap_ms":0,"tick_ms":110,"event":{EVT_REQUEST_RECEIVE},"arg0":0,"arg1":3,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"trial":1,"gap_ms":0,"tick_ms":120,"event":{EVT_BW_REFRESH_START},"arg0":3,"arg1":0,"arg2":1}}',
        f'{{"kind":"log","schema_version":1,"trial":1,"gap_ms":0,"tick_ms":130,"event":{EVT_BW_BUSY_ASSERTED},"arg0":3,"arg1":0,"arg2":1}}',
        f'{{"kind":"log","schema_version":1,"trial":1,"gap_ms":0,"tick_ms":340,"event":{EVT_BW_BUSY_READY},"arg0":3,"arg1":1,"arg2":210}}',
        f'{{"kind":"log","schema_version":1,"trial":1,"gap_ms":0,"tick_ms":350,"event":{EVT_PAGE_TURN},"arg0":0,"arg1":1,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"trial":1,"gap_ms":0,"tick_ms":351,"event":{EVT_TURN_DEQUEUED},"arg0":3,"arg1":1,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"trial":1,"gap_ms":0,"tick_ms":360,"event":{EVT_REQUEST_RECEIVE},"arg0":0,"arg1":4,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"trial":1,"gap_ms":0,"tick_ms":610,"event":{EVT_BW_REFRESH_START},"arg0":4,"arg1":1,"arg2":2}}',
        f'{{"kind":"log","schema_version":1,"trial":1,"gap_ms":0,"tick_ms":615,"event":{EVT_BW_BUSY_ASSERTED},"arg0":4,"arg1":1,"arg2":2}}',
        f'{{"kind":"log","schema_version":1,"trial":1,"gap_ms":0,"tick_ms":620,"event":{EVT_PAGE_TURN},"arg0":1,"arg1":2,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"trial":1,"gap_ms":0,"tick_ms":621,"event":{EVT_TURN_DEQUEUED},"arg0":4,"arg1":2,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"trial":1,"gap_ms":0,"tick_ms":980,"event":{EVT_GRAY_OVERLAY_START},"arg0":2,"arg1":0,"arg2":0}}',
        '{"kind":"trial_result","schema_version":1,"trial":1,"gap_ms":0,"all_ok":true,"expected_page":2,"status_page":2,"error_count":0}',
        '{"kind":"trial_start","schema_version":1,"trial":2,"gap_ms":400}',
        '{"kind":"key_sent","schema_version":1,"trial":2,"gap_ms":400,"turn_index":0,"sequence":5,"host_elapsed_ms":0}',
        '{"kind":"key_response","schema_version":1,"trial":2,"gap_ms":400,"turn_index":0,"sequence":5,"response_elapsed_ms":15}',
        '{"kind":"key_sent","schema_version":1,"trial":2,"gap_ms":400,"turn_index":1,"sequence":6,"host_elapsed_ms":400}',
        '{"kind":"key_response","schema_version":1,"trial":2,"gap_ms":400,"turn_index":1,"sequence":6,"response_elapsed_ms":375}',
        f'{{"kind":"log","schema_version":1,"trial":2,"gap_ms":400,"tick_ms":1000,"event":{EVT_CMD_RECEIPT},"arg0":5,"arg1":5,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"trial":2,"gap_ms":400,"tick_ms":1010,"event":{EVT_REQUEST_RECEIVE},"arg0":0,"arg1":5,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"trial":2,"gap_ms":400,"tick_ms":1200,"event":{EVT_BW_REFRESH_START},"arg0":5,"arg1":0,"arg2":1}}',
        f'{{"kind":"log","schema_version":1,"trial":2,"gap_ms":400,"tick_ms":1210,"event":{EVT_BW_BUSY_ASSERTED},"arg0":5,"arg1":0,"arg2":1}}',
        f'{{"kind":"log","schema_version":1,"trial":2,"gap_ms":400,"tick_ms":1350,"event":{EVT_BW_BUSY_READY},"arg0":5,"arg1":1,"arg2":140}}',
        f'{{"kind":"log","schema_version":1,"trial":2,"gap_ms":400,"tick_ms":1360,"event":{EVT_PAGE_TURN},"arg0":0,"arg1":1,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"trial":2,"gap_ms":400,"tick_ms":1361,"event":{EVT_TURN_DEQUEUED},"arg0":5,"arg1":1,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"trial":2,"gap_ms":400,"tick_ms":1500,"event":{EVT_CMD_RECEIPT},"arg0":5,"arg1":6,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"trial":2,"gap_ms":400,"tick_ms":1510,"event":{EVT_REQUEST_RECEIVE},"arg0":0,"arg1":6,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"trial":2,"gap_ms":400,"tick_ms":1600,"event":{EVT_BW_REFRESH_START},"arg0":6,"arg1":1,"arg2":2}}',
        f'{{"kind":"log","schema_version":1,"trial":2,"gap_ms":400,"tick_ms":1610,"event":{EVT_BW_BUSY_ASSERTED},"arg0":6,"arg1":1,"arg2":2}}',
        f'{{"kind":"log","schema_version":1,"trial":2,"gap_ms":400,"tick_ms":1850,"event":{EVT_BW_BUSY_READY},"arg0":6,"arg1":2,"arg2":240}}',
        f'{{"kind":"log","schema_version":1,"trial":2,"gap_ms":400,"tick_ms":1718,"event":{EVT_GRAY_OVERLAY_START},"arg0":1,"arg1":0,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"trial":2,"gap_ms":400,"tick_ms":1860,"event":{EVT_PAGE_TURN},"arg0":1,"arg1":2,"arg2":0}}',
        f'{{"kind":"log","schema_version":1,"trial":2,"gap_ms":400,"tick_ms":1861,"event":{EVT_TURN_DEQUEUED},"arg0":6,"arg1":2,"arg2":0}}',
        '{"kind":"trial_result","schema_version":1,"trial":2,"gap_ms":400,"all_ok":true,"expected_page":2,"status_page":2,"error_count":0}',
        '{"kind":"run_result","schema_version":1,"error_count":0}',
    ]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def test_analyze_cadence_reports_fastest_reliable_gap(tmp_path: Path) -> None:
    evidence = tmp_path / "cadence.jsonl"
    write_evidence(evidence)

    result = subprocess.run(
        [
            sys.executable,
            "scripts/analyze_cadence.py",
            "--evidence",
            str(evidence),
        ],
        check=True,
        text=True,
        capture_output=True,
    )

    assert "gap_ms=0" in result.stdout
    assert "gap_ms=400" in result.stdout
    assert "fastest_reliable_gap_ms=400" in result.stdout
    assert "gap_ms=0 all_ok=true next_key_after_bw_before_gray=false" in result.stdout
    assert "gap_ms=400 all_ok=true next_key_after_bw_before_gray=true" in result.stdout
    assert "gap_ms=0 all_ok=true next_key_after_bw_before_gray=false bw_start_before_next_key=false" in result.stdout
    assert "gap_ms=400 all_ok=true next_key_after_bw_before_gray=true bw_start_before_next_key=true" in result.stdout
    assert "fastest_bw_start_gap_ms=400" in result.stdout
    assert "gap_ms=0 all_ok=true next_key_after_bw_before_gray=false bw_start_before_next_key=false bw_busy_asserted_before_next_key=false" in result.stdout
    assert "gap_ms=400 all_ok=true next_key_after_bw_before_gray=true bw_start_before_next_key=true bw_busy_asserted_before_next_key=true" in result.stdout
    assert "fastest_bw_busy_asserted_gap_ms=400" in result.stdout
    assert "serial_accept_after_bw_busy_asserted=false display_dequeue_after_bw_busy_asserted=true queued_before_bw_busy_asserted=1" in result.stdout
    assert "serial_accept_after_bw_busy_asserted=true display_dequeue_after_bw_busy_asserted=true queued_before_bw_busy_asserted=0" in result.stdout
    assert "fastest_serial_accept_after_bw_busy_asserted_gap_ms=400" in result.stdout
    assert "fastest_display_dequeue_after_bw_busy_asserted_gap_ms=0" in result.stdout
    assert "delta_receipt_to_dequeue_avg_ms=10" in result.stdout
    assert "delta_dequeue_to_bw_start_avg_ms=140" in result.stdout
    assert "delta_bw_start_to_busy_assert_avg_ms=10" in result.stdout
    assert "delta_busy_assert_to_busy_ready_avg_ms=190" in result.stdout
    assert "delta_busy_ready_to_page_turn_avg_ms=10" in result.stdout
    assert "delta_page_turn_to_gray_start_avg_ms=358" in result.stdout
