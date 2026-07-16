use std::fs::File;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::time::Duration;

use binbook_diagnostic_protocol::{
    decode_hello_response, Opcode, TransferDirection, TransferTraceState,
    CAP_STORAGE_TRANSFER_METRICS, CAP_STORAGE_UPLOAD_WINDOW_2, TRANSFER_TRACE_FLAG_LIBRARY_PENDING,
};

use crate::{
    bulk_serial::BulkSerial,
    bulk_upload::bulk_upload_stream,
    host_transfer_metrics::MeasuringProgress,
    progress::TextProgress,
    serial_transport::SerialSession,
    transfer_negotiation::{RequestedMode, RequestedWindow, SelectedMode},
    transfer_probe::probe_transfer,
    transfer_report::{ReportOperation, ReportResult, TransferReport},
    transfer_trace_client::{clear_trace, get_trace, start_trace},
    upload::{
        upload_stream, SystemClock, UploadError, UploadPacket, UploadRequest, UploadSummary,
        UploadTransport,
    },
    ProgressMode, RequestedTransferMode, RequestedTransferWindow, UploadArgs,
};

const STORAGE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub fn run_command(args: &UploadArgs) -> Result<(), UploadError> {
    let clock = SystemClock::start();
    let destination = destination_name(&args.file, args.name.as_deref())?;
    let mut source = File::open(&args.file).map_err(|source| UploadError::SourceOpen {
        path: args.file.clone(),
        source,
    })?;
    let mut transport =
        SerialSession::open(&args.port).map_err(|message| UploadError::Transport {
            message,
            cleanup_confirmed: false,
        })?;
    let stderr = io::stderr();
    let is_tty = stderr.is_terminal();
    let mut stderr = stderr.lock();
    let mode = args.progress.unwrap_or(if args.json {
        ProgressMode::Never
    } else {
        ProgressMode::Auto
    });
    let text_progress = TextProgress::new(&mut stderr, mode, is_tty, destination);
    let mut progress = MeasuringProgress::new(text_progress, &clock);
    let selection = probe_transfer(
        &mut transport,
        binbook_diagnostic_protocol::TransferDirection::Upload,
        requested_mode(args.transfer_mode),
        requested_window(args.window),
    )?;
    if args.sd_write_trace.is_some() {
        crate::sd_write_trace_client::clear(&mut transport)?;
    }
    let trace_id = if args.transfer_report.is_some()
        && selection.capabilities & CAP_STORAGE_TRANSFER_METRICS != 0
    {
        Some(start_trace(&mut transport, TransferDirection::Upload)?.trace_id)
    } else {
        None
    };
    let request = UploadRequest::new(destination, args.no_wait_library);
    let (transfer_result, status_queries, retries) = match selection.mode {
        SelectedMode::Legacy => {
            let mut legacy = NegotiatedLegacyTransport {
                session: &mut transport,
                window: selection.window,
            };
            (
                upload_stream(&mut source, request, &mut legacy, &mut progress, &clock),
                0,
                0,
            )
        }
        SelectedMode::Bulk => {
            let mut bulk = BulkSerial::new(
                &mut transport,
                Duration::from_millis(u64::from(selection.idle_timeout_ms)),
            );
            let result = bulk_upload_stream(
                &mut source,
                request,
                selection.window,
                &mut bulk,
                &mut progress,
                &clock,
            );
            let (status_queries, retries) = bulk.recovery_metrics();
            (result, status_queries, retries)
        }
    };
    let progress_result = progress.inner().result().map_err(UploadError::Progress);
    let host = progress.finish(status_queries, retries);
    let summary = match transfer_result {
        Ok(summary) => summary,
        Err(error) => {
            if let Some(path) = args.sd_write_trace.as_deref() {
                let _ = crate::sd_write_trace_client::freeze(&mut transport);
                if let Ok(capture) = crate::sd_write_trace_client::get_all(&mut transport) {
                    let _ = capture.write_atomic(path);
                }
            }
            if let Some(path) = args.transfer_report.as_deref() {
                let firmware = trace_id.and_then(|trace_id| {
                    let snapshot =
                        get_trace(&mut transport, TransferDirection::Upload, trace_id).ok();
                    let _ = clear_trace(&mut transport, TransferDirection::Upload, trace_id);
                    snapshot
                });
                let protocol_error_delta = firmware.map_or(0, |value| {
                    value
                        .protocol_errors_end
                        .saturating_sub(value.protocol_errors_start)
                });
                TransferReport {
                    operation: ReportOperation::Upload,
                    mode_requested: requested_mode_name(args.transfer_mode),
                    mode_selected: selected_mode_name(selection.mode),
                    name: destination,
                    bytes: source.metadata().map_or(0, |value| value.len()),
                    expected_crc32: None,
                    computed_crc32: None,
                    limits: selection.limits,
                    host,
                    firmware: firmware.as_ref(),
                    result: ReportResult {
                        ok: false,
                        library_visible: None,
                        library_refresh_pending: firmware.is_some_and(|value| {
                            value.flags & TRANSFER_TRACE_FLAG_LIBRARY_PENDING != 0
                        }),
                        cleanup_confirmed: cleanup_confirmed(&error),
                        protocol_error_delta,
                    },
                }
                .write_atomic(path)?;
            }
            return Err(error);
        }
    };
    progress_result?;
    let sd_trace_issue = if let Some(path) = args.sd_write_trace.as_deref() {
        crate::sd_write_trace_client::freeze(&mut transport)?;
        let capture = crate::sd_write_trace_client::get_all(&mut transport)?;
        capture.write_atomic(path)?;
        if capture.overflowed || capture.anomalies != 0 {
            Some(format!(
                "SD write trace is incomplete: overflowed={}, anomalies={}",
                capture.overflowed, capture.anomalies
            ))
        } else {
            None
        }
    } else {
        None
    };
    let firmware = if let Some(trace_id) = trace_id {
        let snapshot = get_trace(&mut transport, TransferDirection::Upload, trace_id)?;
        if snapshot.state != TransferTraceState::Succeeded {
            return Err(UploadError::Response(format!(
                "successful upload returned {:?} transfer trace",
                snapshot.state
            )));
        }
        clear_trace(&mut transport, TransferDirection::Upload, trace_id)?;
        Some(snapshot)
    } else {
        None
    };
    if let Some(path) = args.transfer_report.as_deref() {
        let protocol_error_delta = firmware.map_or(0, |value| {
            value
                .protocol_errors_end
                .saturating_sub(value.protocol_errors_start)
        });
        let library_refresh_pending = firmware.map_or(args.no_wait_library, |value| {
            value.flags & TRANSFER_TRACE_FLAG_LIBRARY_PENDING != 0
        });
        TransferReport {
            operation: ReportOperation::Upload,
            mode_requested: requested_mode_name(args.transfer_mode),
            mode_selected: selected_mode_name(selection.mode),
            name: destination,
            bytes: summary.bytes,
            expected_crc32: Some(summary.crc32),
            computed_crc32: Some(summary.crc32),
            limits: selection.limits,
            host,
            firmware: firmware.as_ref(),
            result: ReportResult {
                ok: true,
                library_visible: Some(summary.library_visible),
                library_refresh_pending,
                cleanup_confirmed: None,
                protocol_error_delta,
            },
        }
        .write_atomic(path)?;
    }
    print_summary(destination, &summary, args.json);
    match sd_trace_issue {
        Some(message) => Err(UploadError::Response(message)),
        None => Ok(()),
    }
}

fn cleanup_confirmed(error: &UploadError) -> Option<bool> {
    match error {
        UploadError::Transport {
            cleanup_confirmed, ..
        }
        | UploadError::DeviceStatus {
            cleanup_confirmed, ..
        }
        | UploadError::AcceptedLength {
            cleanup_confirmed, ..
        }
        | UploadError::Crc {
            cleanup_confirmed, ..
        } => Some(*cleanup_confirmed),
        _ => None,
    }
}

fn requested_mode_name(value: RequestedTransferMode) -> &'static str {
    match value {
        RequestedTransferMode::Auto => "auto",
        RequestedTransferMode::Legacy => "legacy",
        RequestedTransferMode::Bulk => "bulk",
    }
}

fn selected_mode_name(value: SelectedMode) -> &'static str {
    match value {
        SelectedMode::Legacy => "legacy",
        SelectedMode::Bulk => "bulk",
    }
}

fn requested_mode(value: RequestedTransferMode) -> RequestedMode {
    match value {
        RequestedTransferMode::Auto => RequestedMode::Auto,
        RequestedTransferMode::Legacy => RequestedMode::Legacy,
        RequestedTransferMode::Bulk => RequestedMode::Bulk,
    }
}

fn requested_window(value: RequestedTransferWindow) -> RequestedWindow {
    match value {
        RequestedTransferWindow::Auto => RequestedWindow::Auto,
        RequestedTransferWindow::One => RequestedWindow::One,
        RequestedTransferWindow::Two => RequestedWindow::Two,
        RequestedTransferWindow::Four => RequestedWindow::Four,
        RequestedTransferWindow::Eight => RequestedWindow::Eight,
    }
}

struct NegotiatedLegacyTransport<'a> {
    session: &'a mut SerialSession,
    window: u8,
}

impl UploadTransport for NegotiatedLegacyTransport<'_> {
    fn upload_window(&mut self) -> Result<usize, UploadError> {
        Ok(usize::from(self.window))
    }

    fn exchange(
        &mut self,
        request: &[u8],
        opcode: Opcode,
        sequence: u16,
    ) -> Result<Vec<u8>, UploadError> {
        UploadTransport::exchange(self.session, request, opcode, sequence)
    }

    fn exchange_batch(&mut self, requests: &[UploadPacket]) -> Result<Vec<Vec<u8>>, UploadError> {
        UploadTransport::exchange_batch(self.session, requests)
    }

    fn wait_library_poll(&mut self) {
        UploadTransport::wait_library_poll(self.session);
    }
}

fn destination_name<'a>(
    source: &'a Path,
    explicit: Option<&'a str>,
) -> Result<&'a str, UploadError> {
    explicit
        .or_else(|| source.file_name().and_then(|name| name.to_str()))
        .ok_or(UploadError::DestinationName)
}

fn print_summary(destination: &str, summary: &UploadSummary, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "bytes": summary.bytes,
                "bytes_per_second": summary.bytes_per_second,
                "crc32": format!("0x{:08X}", summary.crc32),
                "destination": destination,
                "elapsed_ms": summary.elapsed_ms,
                "library_visible": summary.library_visible,
            })
        );
    } else {
        println!(
            "Uploaded {destination} ({} bytes, CRC32=0x{:08X}, {} B/s, library_visible={})",
            summary.bytes, summary.crc32, summary.bytes_per_second, summary.library_visible
        );
    }
}

impl UploadTransport for SerialSession {
    fn upload_window(&mut self) -> Result<usize, UploadError> {
        const HELLO_SEQUENCE: u16 = u16::MAX;
        let request = crate::diag_protocol::hello_request(HELLO_SEQUENCE);
        let response = self
            .exchange_frame_batch(&request, &[HELLO_SEQUENCE], STORAGE_REQUEST_TIMEOUT)
            .map_err(transport_error)?
            .into_iter()
            .next()
            .ok_or_else(|| UploadError::Response("HELLO response was absent".into()))?;
        let hello_payload =
            crate::upload_protocol::checked_payload(&response, Opcode::Hello, HELLO_SEQUENCE)?;
        let hello = decode_hello_response(&hello_payload)?;
        Ok(if hello.capabilities & CAP_STORAGE_UPLOAD_WINDOW_2 != 0 {
            2
        } else {
            1
        })
    }

    fn exchange(
        &mut self,
        request: &[u8],
        opcode: Opcode,
        sequence: u16,
    ) -> Result<Vec<u8>, UploadError> {
        self.exchange_frame_batch(request, &[sequence], STORAGE_REQUEST_TIMEOUT)
            .map_err(transport_error)?
            .into_iter()
            .next()
            .ok_or_else(|| {
                UploadError::Response(format!(
                    "response was absent for {opcode:?} sequence {sequence}"
                ))
            })
    }

    fn exchange_batch(&mut self, requests: &[UploadPacket]) -> Result<Vec<Vec<u8>>, UploadError> {
        let bytes = requests
            .iter()
            .flat_map(|request| request.frame.iter().copied())
            .collect::<Vec<_>>();
        let sequences = requests
            .iter()
            .map(|request| request.sequence)
            .collect::<Vec<_>>();
        self.exchange_frame_batch(&bytes, &sequences, STORAGE_REQUEST_TIMEOUT)
            .map_err(transport_error)
    }

    fn wait_library_poll(&mut self) {
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn transport_error(message: String) -> UploadError {
    UploadError::Transport {
        message,
        cleanup_confirmed: false,
    }
}
