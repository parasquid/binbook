use std::io::{self, Write};
use std::path::Path;

use binbook_diagnostic_protocol::{TransferLimits, TransferTraceSnapshot};
use serde_json::{json, Value};

use crate::atomic_output::write_atomic_io;

pub const TRANSFER_REPORT_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportOperation {
    Upload,
    Readback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HostTransferMetrics {
    pub prepare_ms: u64,
    pub data_ms: u64,
    pub finish_ms: u64,
    pub library_ms: u64,
    pub end_to_end_ms: u64,
    pub bytes_per_second: u64,
    pub status_queries: u32,
    pub retries: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportResult {
    pub ok: bool,
    pub library_visible: Option<bool>,
    pub library_refresh_pending: bool,
    pub cleanup_confirmed: Option<bool>,
    pub protocol_error_delta: u32,
}

pub struct TransferReport<'a> {
    pub operation: ReportOperation,
    pub mode_requested: &'a str,
    pub mode_selected: &'a str,
    pub name: &'a str,
    pub bytes: u64,
    pub expected_crc32: Option<u32>,
    pub computed_crc32: Option<u32>,
    pub limits: TransferLimits,
    pub host: HostTransferMetrics,
    pub firmware: Option<&'a TransferTraceSnapshot>,
    pub result: ReportResult,
}

impl TransferReport<'_> {
    pub fn to_json(&self) -> Value {
        json!({
            "schema_version": TRANSFER_REPORT_SCHEMA_VERSION,
            "operation": operation_name(self.operation),
            "mode_requested": self.mode_requested,
            "mode_selected": self.mode_selected,
            "name": self.name,
            "bytes": self.bytes,
            "expected_crc32": self.expected_crc32,
            "computed_crc32": self.computed_crc32,
            "limits": limits_json(self.limits),
            "host": host_json(self.host),
            "firmware": self.firmware.map(firmware_json),
            "result": {
                "status": if self.result.ok { "ok" } else { "error" },
                "library_visible": self.result.library_visible,
                "library_refresh_pending": self.result.library_refresh_pending,
                "cleanup_confirmed": self.result.cleanup_confirmed,
                "protocol_error_delta": self.result.protocol_error_delta,
            },
        })
    }

    pub fn write_atomic(&self, path: &Path) -> io::Result<()> {
        write_atomic_io(path, |file| {
            serde_json::to_writer_pretty(&mut *file, &self.to_json())?;
            file.write_all(b"\n")
        })
    }
}

fn operation_name(value: ReportOperation) -> &'static str {
    match value {
        ReportOperation::Upload => "upload",
        ReportOperation::Readback => "readback",
    }
}

fn limits_json(value: TransferLimits) -> Value {
    json!({
        "max_data_bytes": value.max_data_bytes,
        "legacy_upload_window": value.legacy_upload_window,
        "legacy_read_window": value.legacy_read_window,
        "bulk_slots": value.bulk_slots,
        "rx_capacity_bytes": value.rx_capacity_bytes,
        "max_frame_bytes": value.max_frame_bytes,
        "idle_timeout_ms": value.idle_timeout_ms,
    })
}

fn host_json(value: HostTransferMetrics) -> Value {
    json!({
        "prepare_ms": value.prepare_ms,
        "data_ms": value.data_ms,
        "finish_ms": value.finish_ms,
        "library_ms": value.library_ms,
        "end_to_end_ms": value.end_to_end_ms,
        "bytes_per_second": value.bytes_per_second,
        "status_queries": value.status_queries,
        "retries": value.retries,
    })
}

fn firmware_json(value: &TransferTraceSnapshot) -> Value {
    json!({
        "direction": format!("{:?}", value.direction).to_ascii_lowercase(),
        "trace_id": value.trace_id,
        "state": format!("{:?}", value.state).to_ascii_lowercase(),
        "selected_window": value.selected_window,
        "chunk_size": value.chunk_size,
        "peak_rx_bytes": value.peak_rx_bytes,
        "logical_bytes": value.logical_bytes,
        "frames": value.frames,
        "usb_rx_bytes": value.usb_rx_bytes,
        "usb_tx_bytes": value.usb_tx_bytes,
        "usb_rx_us": value.usb_rx_us,
        "usb_tx_us": value.usb_tx_us,
        "framing_us": value.framing_us,
        "frame_crc_us": value.frame_crc_us,
        "owner_queue_us": value.owner_queue_us,
        "owner_execute_us": value.owner_execute_us,
        "sd_read_calls": value.sd_read_calls,
        "sd_write_calls": value.sd_write_calls,
        "sd_read_blocks": value.sd_read_blocks,
        "sd_write_blocks": value.sd_write_blocks,
        "sd_read_logical_bytes": value.sd_read_logical_bytes,
        "sd_write_logical_bytes": value.sd_write_logical_bytes,
        "sd_read_us": value.sd_read_us,
        "sd_write_us": value.sd_write_us,
        "fat_allocation_us": value.fat_allocation_us,
        "upload_prepare_us": value.upload_prepare_us,
        "data_io_us": value.data_io_us,
        "crc_us": value.crc_us,
        "commit_us": value.commit_us,
        "library_refresh_us": value.library_refresh_us,
        "data_phase_us": value.data_phase_us,
        "end_to_end_us": value.end_to_end_us,
        "usb_sd_overlap_us": value.usb_sd_overlap_us,
        "peak_slots": value.peak_slots,
        "flags": value.flags,
        "queue_stalls": value.queue_stalls,
        "protocol_errors_start": value.protocol_errors_start,
        "protocol_errors_end": value.protocol_errors_end,
    })
}
