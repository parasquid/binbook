use std::io::{IsTerminal, Write};
use std::time::Duration;

use binbook_diagnostic_protocol::{KeyCode, Opcode, MAX_PAYLOAD_BYTES};

use crate::{
    diag_protocol, serial_transport::SerialSession, DiagCommand, ExerciseCommand, PageAction,
    ProbeCommand, ProgressMode, RefreshModeAction, RefreshModeChoice, RequestedTransferMode,
    RequestedTransferWindow, StorageCommand,
};

const STORAGE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub fn run_diag(cmd: DiagCommand) {
    if let DiagCommand::Exercise { exercise } = &cmd {
        match exercise {
            ExerciseCommand::StagedGray { port } => {
                if let Err(error) = crate::exercise::run_staged_gray(port) {
                    eprintln!("Communication error: {error}");
                    std::process::exit(1);
                }
                return;
            }
            ExerciseCommand::NavBurst {
                port,
                rounds,
                inter_key_ms,
                output,
            } => {
                if let Err(error) =
                    crate::nav_burst::run_nav_burst(port, *rounds, *inter_key_ms, output.as_deref())
                {
                    eprintln!("Diagnostic error: {error}");
                    std::process::exit(1);
                }
                return;
            }
            ExerciseCommand::PageTurnCadence {
                port,
                key,
                start_page,
                turns,
                rounds,
                min_gap_ms,
                max_gap_ms,
                coarse_step_ms,
                refine_step_ms,
                output,
            } => {
                let Some(key) = crate::page_turn_cadence::key_name_to_code(key) else {
                    eprintln!("Diagnostic error: invalid key {key}");
                    std::process::exit(1);
                };
                if min_gap_ms > max_gap_ms {
                    eprintln!("Diagnostic error: min gap must not exceed max gap");
                    std::process::exit(1);
                }
                if let Err(error) = crate::page_turn_cadence::run_page_turn_cadence(
                    port,
                    crate::page_turn_cadence::PageTurnCadenceOptions {
                        port,
                        key,
                        start_page: *start_page,
                        turns: *turns,
                        rounds: *rounds,
                        min_gap_ms: *min_gap_ms,
                        max_gap_ms: *max_gap_ms,
                        coarse_step_ms: *coarse_step_ms,
                        refine_step_ms: *refine_step_ms,
                    },
                    output.as_deref(),
                ) {
                    eprintln!("Diagnostic error: {error}");
                    std::process::exit(1);
                }
                return;
            }
            ExerciseCommand::PartialWindowBusy {
                port,
                rounds,
                output,
            } => {
                if let Err(error) = crate::partial_window_busy::run_partial_window_busy(
                    port,
                    crate::partial_window_busy::PartialWindowBusyOptions {
                        port,
                        rounds: *rounds,
                    },
                    output.as_deref(),
                ) {
                    eprintln!("Diagnostic error: {error}");
                    std::process::exit(1);
                }
                return;
            }
        }
    }

    let (port, frame, expected_opcode, timeout) = match &cmd {
        DiagCommand::Hello { port } => (
            port.clone(),
            diag_protocol::hello_request(1),
            Opcode::Hello,
            Duration::from_secs(2),
        ),
        DiagCommand::Key { port, key } => {
            let kc = match key.as_str() {
                "LEFT" => KeyCode::Left,
                "RIGHT" => KeyCode::Right,
                "UP" => KeyCode::Up,
                "DOWN" => KeyCode::Down,
                "SELECT" => KeyCode::Select,
                "BACK" => KeyCode::Back,
                "POWER" => KeyCode::Power,
                _ => unreachable!(),
            };
            (
                port.clone(),
                diag_protocol::key_request(1, kc),
                Opcode::Key,
                Duration::from_secs(30),
            )
        }
        DiagCommand::Page { port, action } => {
            let frame = match action {
                PageAction::Next => diag_protocol::page_action_request(
                    1,
                    binbook_diagnostic_protocol::PageAction::Next,
                ),
                PageAction::Previous => diag_protocol::page_action_request(
                    1,
                    binbook_diagnostic_protocol::PageAction::Previous,
                ),
                PageAction::First => diag_protocol::page_action_request(
                    1,
                    binbook_diagnostic_protocol::PageAction::First,
                ),
                PageAction::Last => diag_protocol::page_action_request(
                    1,
                    binbook_diagnostic_protocol::PageAction::Last,
                ),
                PageAction::Current => diag_protocol::page_action_request(
                    1,
                    binbook_diagnostic_protocol::PageAction::Current,
                ),
                PageAction::Goto { page } => diag_protocol::page_goto_request(1, *page),
            };
            (port.clone(), frame, Opcode::Page, Duration::from_secs(30))
        }
        DiagCommand::Status { port } => (
            port.clone(),
            diag_protocol::status_request(1),
            Opcode::Status,
            Duration::from_secs(2),
        ),
        DiagCommand::RefreshMode { port, action } => {
            let frame = match action {
                RefreshModeAction::Get => diag_protocol::refresh_mode_get_request(1),
                RefreshModeAction::Set { mode } => {
                    let code = match mode {
                        RefreshModeChoice::Balanced => {
                            binbook_diagnostic_protocol::RefreshModeCode::Balanced
                        }
                        RefreshModeChoice::Fast => {
                            binbook_diagnostic_protocol::RefreshModeCode::Fast
                        }
                    };
                    diag_protocol::refresh_mode_set_request(1, code)
                }
            };
            (
                port.clone(),
                frame,
                Opcode::RefreshMode,
                Duration::from_secs(2),
            )
        }
        DiagCommand::Logs { port, since, clear } => {
            if *clear {
                (
                    port.clone(),
                    diag_protocol::log_clear_request(1),
                    Opcode::LogClear,
                    Duration::from_secs(2),
                )
            } else {
                let cursor = since.unwrap_or(0);
                (
                    port.clone(),
                    diag_protocol::log_get_request(1, cursor, 496),
                    Opcode::LogGet,
                    Duration::from_secs(2),
                )
            }
        }
        DiagCommand::Crash { port, clear } => {
            if *clear {
                (
                    port.clone(),
                    diag_protocol::crash_clear_request(1),
                    Opcode::CrashClear,
                    Duration::from_secs(2),
                )
            } else {
                (
                    port.clone(),
                    diag_protocol::crash_get_request(1),
                    Opcode::CrashGet,
                    Duration::from_secs(2),
                )
            }
        }
        DiagCommand::Probe { port, probe } => {
            let choice = match probe {
                ProbeCommand::WindowCorners => diag_protocol::ProbeChoice::WindowCorners,
                ProbeCommand::ClearWhite => diag_protocol::ProbeChoice::ClearWhite,
                ProbeCommand::FullRefreshCurrent => diag_protocol::ProbeChoice::FullRefreshCurrent,
                ProbeCommand::PartialWindowBusy => {
                    diag_protocol::ProbeChoice::PartialWindowBusyMatrix
                }
                ProbeCommand::CustomLutFastA => diag_protocol::ProbeChoice::CustomLutFastA,
                ProbeCommand::CustomLutFastB => diag_protocol::ProbeChoice::CustomLutFastB,
            };
            (
                port.clone(),
                diag_protocol::display_probe_request(1, choice),
                Opcode::DisplayProbe,
                crate::DISPLAY_PROBE_TIMEOUT,
            )
        }
        DiagCommand::Storage(storage_cmd) => match storage_cmd {
            StorageCommand::List { port, path } => {
                let _ = path;
                return list_storage(port);
            }
            StorageCommand::Read {
                port,
                path,
                output,
                offset,
                max_bytes,
                progress,
                transfer_mode,
                window,
                transfer_report,
            } => {
                if let Some(output) = output {
                    if offset.is_some() || max_bytes.is_some() {
                        return read_file_range(
                            port,
                            path,
                            output,
                            offset.unwrap_or(0),
                            max_bytes.unwrap_or((MAX_PAYLOAD_BYTES - 5) as u32),
                        );
                    }
                    return read_file(
                        port,
                        path,
                        output,
                        *progress,
                        *transfer_mode,
                        *window,
                        transfer_report.as_deref(),
                    );
                }
                let frame = diag_protocol::store_read_request_range(
                    1,
                    path,
                    offset.unwrap_or(0),
                    max_bytes.unwrap_or((MAX_PAYLOAD_BYTES - 5) as u32),
                );
                (
                    port.clone(),
                    frame,
                    Opcode::StoreRead,
                    STORAGE_REQUEST_TIMEOUT,
                )
            }
            StorageCommand::Delete { port, path } => {
                let frame = diag_protocol::store_delete_request(1, path);
                (
                    port.clone(),
                    frame,
                    Opcode::StoreDelete,
                    STORAGE_REQUEST_TIMEOUT,
                )
            }
            StorageCommand::Begin {
                port,
                path,
                size,
                crc32,
            } => {
                let frame = diag_protocol::store_upload_begin_request(1, path, *size, *crc32);
                (
                    port.clone(),
                    frame,
                    Opcode::StoreUploadBegin,
                    STORAGE_REQUEST_TIMEOUT,
                )
            }
            StorageCommand::Upload(args) => {
                if let Err(error) = crate::upload_command::run_command(args) {
                    eprintln!("Upload failed: {error}");
                    std::process::exit(1);
                }
                return;
            }
            StorageCommand::Abort { port, upload_id } => {
                let frame = diag_protocol::store_abort_request(1, *upload_id);
                (
                    port.clone(),
                    frame,
                    Opcode::StoreAbort,
                    STORAGE_REQUEST_TIMEOUT,
                )
            }
        },
        DiagCommand::Exercise { .. } => unreachable!("exercise handled above"),
    };

    let mut session = match SerialSession::open(&port) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    match session.send_and_receive(&frame, expected_opcode, 1, timeout) {
        Ok(response) => match diag_protocol::format_response(&response, expected_opcode, 1) {
            Ok(output) => println!("{output}"),
            Err(error) => {
                eprintln!("Response error: {error}");
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("Communication error: {e}");
            std::process::exit(1);
        }
    }
}

fn list_storage(port: &str) {
    use binbook_diagnostic_protocol::{
        decode_frame, decode_store_list_count, decode_store_list_entry, Opcode, MAX_PAYLOAD_BYTES,
        STORE_LIST_ENTRY_HEADER_BYTES,
    };

    let mut session = SerialSession::open(port).unwrap_or_else(|error| {
        eprintln!("Error: {error}");
        std::process::exit(1);
    });
    let mut entries = Vec::new();
    let mut start_index = 0u16;
    let mut sequence = 1u16;
    loop {
        let request = diag_protocol::store_list_request_from(sequence, start_index);
        let response = session
            .send_and_receive(
                &request,
                Opcode::StoreList,
                sequence,
                STORAGE_REQUEST_TIMEOUT,
            )
            .unwrap_or_else(|error| {
                eprintln!("List failed: {error}");
                std::process::exit(1);
            });
        let mut payload = [0u8; MAX_PAYLOAD_BYTES];
        let (_, payload_len) = decode_frame(&response, &mut payload).unwrap_or_else(|error| {
            eprintln!("Invalid list response frame: {error:?}");
            std::process::exit(1);
        });
        let count = decode_store_list_count(&payload[..payload_len]).unwrap_or_else(|error| {
            eprintln!("Invalid list response count: {error:?}");
            std::process::exit(1);
        });
        if count == 0 {
            break;
        }
        let mut pos = 2usize;
        for _ in 0..count {
            let entry =
                decode_store_list_entry(&payload[pos..payload_len]).unwrap_or_else(|error| {
                    eprintln!("Invalid list entry: {error:?}");
                    std::process::exit(1);
                });
            pos += STORE_LIST_ENTRY_HEADER_BYTES + entry.name.len();
            entries.push((entry.name.to_owned(), entry.file_size, entry.page_count));
        }
        start_index = start_index.checked_add(count).unwrap_or_else(|| {
            eprintln!("List cursor overflow");
            std::process::exit(1);
        });
        sequence = sequence.wrapping_add(1).max(1);
    }

    println!("entry_count={}", entries.len());
    for (name, size, page_count) in entries {
        println!("  {name} size={size} pages={page_count}");
    }
}

fn read_file_range(port: &str, path: &str, output: &std::path::Path, offset: u32, max: u32) {
    use binbook_diagnostic_protocol::{
        decode_frame, decode_store_read_response, Opcode, MAX_PAYLOAD_BYTES,
    };

    let mut session = SerialSession::open(port).unwrap_or_else(|error| {
        eprintln!("Error: {error}");
        std::process::exit(1);
    });
    let request = diag_protocol::store_read_request_range(1, path, offset, max);
    let response = session
        .send_and_receive(&request, Opcode::StoreRead, 1, STORAGE_REQUEST_TIMEOUT)
        .unwrap_or_else(|error| {
            eprintln!("Read failed: {error}");
            std::process::exit(1);
        });
    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    let (_, payload_len) = decode_frame(&response, &mut payload).unwrap();
    let decoded = decode_store_read_response(&payload[..payload_len]).unwrap_or_else(|error| {
        eprintln!("Invalid read response: {error:?}");
        std::process::exit(1);
    });
    std::fs::write(output, decoded.data).unwrap_or_else(|error| {
        eprintln!("Error writing {}: {error}", output.display());
        std::process::exit(1);
    });
    println!(
        "Read {} bytes from {} at offset {} into {} (eof={})",
        decoded.data.len(),
        path,
        offset,
        output.display(),
        decoded.eof
    );
}

fn read_file(
    port: &str,
    path: &str,
    output: &std::path::Path,
    progress_mode: Option<ProgressMode>,
    requested_mode: RequestedTransferMode,
    requested_window: RequestedTransferWindow,
    transfer_report: Option<&std::path::Path>,
) {
    let clock = crate::upload::SystemClock::start();
    let file = std::fs::File::create(output).unwrap_or_else(|error| {
        eprintln!("Error creating {}: {error}", output.display());
        std::process::exit(1);
    });
    let mut output_file = std::io::BufWriter::new(file);
    let mut session = SerialSession::open(port).unwrap_or_else(|error| {
        let _ = std::fs::remove_file(output);
        eprintln!("Read failed: {error}");
        std::process::exit(1);
    });
    let selection = crate::transfer_probe::probe_transfer(
        &mut session,
        binbook_diagnostic_protocol::TransferDirection::Readback,
        read_requested_mode(requested_mode),
        read_requested_window(requested_window),
    )
    .unwrap_or_else(|error| {
        let _ = std::fs::remove_file(output);
        eprintln!("Read negotiation failed: {error}");
        std::process::exit(1);
    });
    let prepare_ms = elapsed_ms(&clock);
    let trace_id = if transfer_report.is_some()
        && selection.capabilities & binbook_diagnostic_protocol::CAP_STORAGE_TRANSFER_METRICS != 0
    {
        Some(
            crate::transfer_trace_client::start_trace(
                &mut session,
                binbook_diagnostic_protocol::TransferDirection::Readback,
            )
            .unwrap_or_else(|error| {
                let _ = std::fs::remove_file(output);
                eprintln!("Read trace start failed: {error}");
                std::process::exit(1);
            })
            .trace_id,
        )
    } else {
        None
    };
    let stderr = std::io::stderr();
    let is_tty = stderr.is_terminal();
    let mut stderr = stderr.lock();
    let text_progress = crate::progress::TextProgress::for_readback(
        &mut stderr,
        progress_mode.unwrap_or(ProgressMode::Auto),
        is_tty,
        path,
    );
    let mut progress = crate::host_transfer_metrics::MeasuringProgress::new(text_progress, &clock);
    let storage_name = match crate::storage_readback::sd_storage_name(path) {
        Ok(name) => name,
        Err(error) => {
            drop(output_file);
            let _ = std::fs::remove_file(output);
            eprintln!("Read failed: {error}");
            return;
        }
    };
    let (result, status_queries, retries) = match selection.mode {
        crate::transfer_negotiation::SelectedMode::Legacy => (
            crate::storage_readback::read_stream_with_window(
                storage_name,
                usize::from(selection.window),
                &mut session,
                &mut output_file,
            )
            .map(|bytes| (bytes, None)),
            0,
            0,
        ),
        crate::transfer_negotiation::SelectedMode::Bulk => {
            let mut bulk = crate::bulk_serial::BulkSerial::new(
                &mut session,
                Duration::from_millis(u64::from(selection.idle_timeout_ms)),
            );
            let result = crate::bulk_readback_stream::bulk_readback_stream(
                storage_name,
                selection.window,
                &mut bulk,
                &mut output_file,
                &mut progress,
                &clock,
            );
            let (status_queries, retries) = bulk.recovery_metrics();
            (
                result.map(|summary| (summary.bytes, Some(summary))),
                status_queries,
                retries,
            )
        }
    };
    let (bytes, summary) = match result {
        Ok(value) => value,
        Err(error) => {
            let partial_bytes = output_file
                .get_ref()
                .metadata()
                .map_or(0, |value| value.len());
            drop(output_file);
            let host = progress.finish(status_queries, retries);
            let firmware = trace_id.and_then(|trace_id| {
                let snapshot = crate::transfer_trace_client::get_trace(
                    &mut session,
                    binbook_diagnostic_protocol::TransferDirection::Readback,
                    trace_id,
                )
                .ok();
                let _ = crate::transfer_trace_client::clear_trace(
                    &mut session,
                    binbook_diagnostic_protocol::TransferDirection::Readback,
                    trace_id,
                );
                snapshot
            });
            if let Some(report_path) = transfer_report {
                let protocol_error_delta = firmware.map_or(0, |value| {
                    value
                        .protocol_errors_end
                        .saturating_sub(value.protocol_errors_start)
                });
                let report = crate::transfer_report::TransferReport {
                    operation: crate::transfer_report::ReportOperation::Readback,
                    mode_requested: read_requested_mode_name(requested_mode),
                    mode_selected: read_selected_mode_name(selection.mode),
                    name: path,
                    bytes: partial_bytes,
                    expected_crc32: None,
                    computed_crc32: None,
                    limits: selection.limits,
                    host,
                    firmware: firmware.as_ref(),
                    result: crate::transfer_report::ReportResult {
                        ok: false,
                        library_visible: None,
                        library_refresh_pending: false,
                        cleanup_confirmed: read_cleanup_confirmed(&error),
                        protocol_error_delta,
                    },
                };
                if let Err(report_error) = report.write_atomic(report_path) {
                    eprintln!(
                        "Read transfer report {} failed: {report_error}",
                        report_path.display()
                    );
                }
            }
            let _ = std::fs::remove_file(output);
            eprintln!("Read failed: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = progress.inner().result() {
        let _ = std::fs::remove_file(output);
        eprintln!("Read progress failed: {error}");
        std::process::exit(1);
    }
    if let Err(error) = output_file.flush() {
        let _ = std::fs::remove_file(output);
        eprintln!("Read flush failed: {error}");
        std::process::exit(1);
    }
    drop(output_file);
    let mut host = progress.finish(status_queries, retries);
    if summary.is_none() {
        host.prepare_ms = prepare_ms;
        host.data_ms = host.end_to_end_ms.saturating_sub(prepare_ms);
        host.bytes_per_second = bytes
            .saturating_mul(1_000)
            .checked_div(host.data_ms)
            .unwrap_or(0);
    }
    let before_host_verification_ms = elapsed_ms(&clock);
    let computed_crc32 = if transfer_report.is_some() {
        let mut completed = std::fs::File::open(output).unwrap_or_else(|error| {
            eprintln!("Read verification failed for {}: {error}", output.display());
            std::process::exit(1);
        });
        Some(
            crate::upload_source::measure_source(&mut completed)
                .unwrap_or_else(|error| {
                    eprintln!("Read verification failed for {}: {error}", output.display());
                    std::process::exit(1);
                })
                .1,
        )
    } else {
        summary.as_ref().map(|value| value.crc32)
    };
    let after_host_verification_ms = elapsed_ms(&clock);
    host.finish_ms = host
        .finish_ms
        .saturating_add(after_host_verification_ms.saturating_sub(before_host_verification_ms));
    host.end_to_end_ms = after_host_verification_ms;
    let firmware = trace_id.map(|trace_id| {
        let snapshot = crate::transfer_trace_client::get_trace(
            &mut session,
            binbook_diagnostic_protocol::TransferDirection::Readback,
            trace_id,
        )
        .unwrap_or_else(|error| {
            eprintln!("Read succeeded, but transfer trace retrieval failed: {error}");
            std::process::exit(1);
        });
        if snapshot.state != binbook_diagnostic_protocol::TransferTraceState::Succeeded {
            eprintln!(
                "Read succeeded, but transfer trace state is {:?}",
                snapshot.state
            );
            std::process::exit(1);
        }
        crate::transfer_trace_client::clear_trace(
            &mut session,
            binbook_diagnostic_protocol::TransferDirection::Readback,
            trace_id,
        )
        .unwrap_or_else(|error| {
            eprintln!("Read succeeded, but transfer trace cleanup failed: {error}");
            std::process::exit(1);
        });
        snapshot
    });
    if let Some(report_path) = transfer_report {
        let protocol_error_delta = firmware.map_or(0, |value| {
            value
                .protocol_errors_end
                .saturating_sub(value.protocol_errors_start)
        });
        crate::transfer_report::TransferReport {
            operation: crate::transfer_report::ReportOperation::Readback,
            mode_requested: read_requested_mode_name(requested_mode),
            mode_selected: read_selected_mode_name(selection.mode),
            name: path,
            bytes,
            expected_crc32: summary.as_ref().map(|value| value.crc32),
            computed_crc32,
            limits: selection.limits,
            host,
            firmware: firmware.as_ref(),
            result: crate::transfer_report::ReportResult {
                ok: true,
                library_visible: None,
                library_refresh_pending: false,
                cleanup_confirmed: None,
                protocol_error_delta,
            },
        }
        .write_atomic(report_path)
        .unwrap_or_else(|error| {
            eprintln!(
                "Read succeeded, but transfer report {} failed: {error}",
                report_path.display()
            );
            std::process::exit(1);
        });
    }
    if let Some(summary) = summary {
        println!(
            "Read {bytes} bytes from {path} into {} (CRC32=0x{:08X}, {} B/s)",
            output.display(),
            summary.crc32,
            summary.bytes_per_second
        );
    } else {
        println!("Read {bytes} bytes from {path} into {}", output.display());
    }
}

fn elapsed_ms(clock: &impl crate::upload::Clock) -> u64 {
    clock.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn read_requested_mode_name(value: RequestedTransferMode) -> &'static str {
    match value {
        RequestedTransferMode::Auto => "auto",
        RequestedTransferMode::Legacy => "legacy",
        RequestedTransferMode::Bulk => "bulk",
    }
}

fn read_selected_mode_name(value: crate::transfer_negotiation::SelectedMode) -> &'static str {
    match value {
        crate::transfer_negotiation::SelectedMode::Legacy => "legacy",
        crate::transfer_negotiation::SelectedMode::Bulk => "bulk",
    }
}

fn read_cleanup_confirmed(error: &crate::storage_readback::ReadbackError) -> Option<bool> {
    let crate::storage_readback::ReadbackError::Protocol(message) = error else {
        return None;
    };
    if message.ends_with("cleanup confirmed: true") {
        Some(true)
    } else if message.ends_with("cleanup confirmed: false") {
        Some(false)
    } else {
        None
    }
}

fn read_requested_mode(value: RequestedTransferMode) -> crate::transfer_negotiation::RequestedMode {
    match value {
        RequestedTransferMode::Auto => crate::transfer_negotiation::RequestedMode::Auto,
        RequestedTransferMode::Legacy => crate::transfer_negotiation::RequestedMode::Legacy,
        RequestedTransferMode::Bulk => crate::transfer_negotiation::RequestedMode::Bulk,
    }
}

fn read_requested_window(
    value: RequestedTransferWindow,
) -> crate::transfer_negotiation::RequestedWindow {
    match value {
        RequestedTransferWindow::Auto => crate::transfer_negotiation::RequestedWindow::Auto,
        RequestedTransferWindow::One => crate::transfer_negotiation::RequestedWindow::One,
        RequestedTransferWindow::Two => crate::transfer_negotiation::RequestedWindow::Two,
        RequestedTransferWindow::Four => crate::transfer_negotiation::RequestedWindow::Four,
        RequestedTransferWindow::Eight => crate::transfer_negotiation::RequestedWindow::Eight,
    }
}
