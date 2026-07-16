#![cfg(feature = "serial-device")]

use std::fs;

use binbook::transfer_report::{
    HostTransferMetrics, ReportOperation, ReportResult, TransferReport,
    TRANSFER_REPORT_SCHEMA_VERSION,
};
use binbook_diagnostic_protocol::TransferLimits;

fn limits() -> TransferLimits {
    TransferLimits {
        flags: 3,
        max_data_bytes: 3_584,
        legacy_upload_window: 2,
        legacy_read_window: 2,
        bulk_slots: 4,
        rx_capacity_bytes: 16_504,
        max_frame_bytes: 4_126,
        idle_timeout_ms: 60_000,
    }
}

#[test]
fn report_uses_locked_schema_and_numeric_crc_fields() {
    let report = TransferReport {
        operation: ReportOperation::Upload,
        mode_requested: "auto",
        mode_selected: "bulk",
        name: "book.binbook",
        bytes: 1_022_165,
        expected_crc32: Some(0x89ab_cdef),
        computed_crc32: Some(0x89ab_cdef),
        limits: limits(),
        host: HostTransferMetrics {
            prepare_ms: 10,
            data_ms: 20,
            finish_ms: 30,
            library_ms: 40,
            end_to_end_ms: 100,
            bytes_per_second: 51_108,
            status_queries: 2,
            retries: 1,
        },
        firmware: None,
        result: ReportResult {
            ok: true,
            library_visible: Some(true),
            library_refresh_pending: false,
            cleanup_confirmed: None,
            protocol_error_delta: 0,
        },
    };
    let json = report.to_json();
    assert_eq!(json["schema_version"], TRANSFER_REPORT_SCHEMA_VERSION);
    assert_eq!(json["operation"], "upload");
    assert_eq!(json["expected_crc32"], 0x89ab_cdefu32);
    assert_eq!(json["computed_crc32"], 0x89ab_cdefu32);
    assert_eq!(json["limits"]["bulk_slots"], 4);
    assert_eq!(json["host"]["status_queries"], 2);
    assert!(json["firmware"].is_null());
    assert_eq!(json["result"]["status"], "ok");
}

#[test]
fn report_atomically_replaces_an_existing_file_without_temp_leaks() {
    let directory = std::env::temp_dir().join(format!(
        "binbook-transfer-report-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("report.json");
    fs::write(&path, "stale").unwrap();
    let report = TransferReport {
        operation: ReportOperation::Readback,
        mode_requested: "legacy",
        mode_selected: "legacy",
        name: "book.binbook",
        bytes: 7,
        expected_crc32: None,
        computed_crc32: None,
        limits: limits(),
        host: HostTransferMetrics::default(),
        firmware: None,
        result: ReportResult {
            ok: false,
            library_visible: None,
            library_refresh_pending: false,
            cleanup_confirmed: Some(true),
            protocol_error_delta: 0,
        },
    };
    report.write_atomic(&path).unwrap();
    let decoded: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(decoded["result"]["status"], "error");
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    fs::remove_dir_all(directory).unwrap();
}
