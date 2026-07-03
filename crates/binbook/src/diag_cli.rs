use std::time::Duration;

use binbook_diagnostic_protocol::{KeyCode, Opcode};

use crate::{
    diag_protocol, serial_transport::SerialSession, DiagCommand, ExerciseCommand, PageAction,
    ProbeCommand, RefreshModeAction, RefreshModeChoice, StorageCommand,
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
                let path_str = path.as_deref().unwrap_or("/");
                let frame = diag_protocol::store_list_request(1, path_str);
                (
                    port.clone(),
                    frame,
                    Opcode::StoreList,
                    STORAGE_REQUEST_TIMEOUT,
                )
            }
            StorageCommand::Read { port, path, output } => {
                if let Some(output) = output {
                    return read_file(port, path, output);
                }
                let frame = diag_protocol::store_read_request(1, path);
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
            StorageCommand::Upload { port, path, file } => {
                return upload_file(port, path, file);
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

fn read_file(port: &str, path: &str, output: &std::path::Path) {
    use binbook_diagnostic_protocol::{
        decode_frame, decode_store_read_response, Opcode, MAX_PAYLOAD_BYTES,
    };

    let mut session = match SerialSession::open(port) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    let mut out = Vec::new();
    let mut offset = 0u32;
    let mut sequence = 1u16;
    loop {
        let request = diag_protocol::store_read_request_range(
            sequence,
            path,
            offset,
            (MAX_PAYLOAD_BYTES - 5) as u32,
        );
        let response = match session.send_and_receive(
            &request,
            Opcode::StoreRead,
            sequence,
            STORAGE_REQUEST_TIMEOUT,
        ) {
            Ok(response) => response,
            Err(error) => {
                eprintln!("Read failed: {error}");
                std::process::exit(1);
            }
        };
        let mut payload = [0u8; MAX_PAYLOAD_BYTES];
        let (_header, payload_len) = decode_frame(&response, &mut payload).unwrap();
        let decoded = match decode_store_read_response(&payload[..payload_len]) {
            Ok(decoded) => decoded,
            Err(error) => {
                eprintln!("Invalid read response: {error:?}");
                std::process::exit(1);
            }
        };
        out.extend_from_slice(decoded.data);
        offset = offset.saturating_add(decoded.data.len() as u32);
        if decoded.eof {
            break;
        }
        sequence = sequence.wrapping_add(1).max(1);
    }

    if let Err(error) = std::fs::write(output, &out) {
        eprintln!("Error writing {}: {error}", output.display());
        std::process::exit(1);
    }
    println!(
        "Read {} bytes from {} into {}",
        out.len(),
        path,
        output.display()
    );
}

fn upload_file(port: &str, path: &str, file: &std::path::Path) {
    use binbook_diagnostic_protocol::{
        decode_frame, decode_store_upload_begin_response, decode_store_upload_commit_response,
        decode_store_upload_write_response, encode_frame, encode_store_upload_begin_request,
        encode_store_upload_commit_request, encode_store_upload_write_request, FrameHeader,
        FrameKind, Status, StorageBackend, StoreUploadBeginRequest, StoreUploadCommitResult,
        MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES,
    };

    let data = match std::fs::read(file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error reading {}: {e}", file.display());
            std::process::exit(1);
        }
    };

    let crc32 = crc32_simple(&data);

    let mut session = match SerialSession::open(port) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    let req = StoreUploadBeginRequest {
        backend: StorageBackend::Sd,
        name: path,
        file_size: data.len() as u32,
        expected_crc32: crc32,
    };
    let mut payload_buf = [0u8; MAX_PAYLOAD_BYTES];
    let plen = encode_store_upload_begin_request(&req, &mut payload_buf).unwrap();
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::StoreUploadBegin,
        status: Status::Ok,
        sequence: 1,
        payload_len: plen as u16,
    };
    let mut frame_buf = [0u8; MAX_FRAME_BYTES];
    let flen = encode_frame(&header, &payload_buf[..plen], &mut frame_buf).unwrap();
    let resp = match session.send_and_receive(
        &frame_buf[..flen],
        Opcode::StoreUploadBegin,
        1,
        STORAGE_REQUEST_TIMEOUT,
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Upload begin failed: {e}");
            std::process::exit(1);
        }
    };
    let mut resp_payload = [0u8; MAX_PAYLOAD_BYTES];
    let (_resp_header, resp_plen) = decode_frame(&resp, &mut resp_payload).unwrap();
    let upload_id = match decode_store_upload_begin_response(&resp_payload[..resp_plen]) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Invalid upload begin response: {e:?}");
            std::process::exit(1);
        }
    };

    const CHUNK_SIZE: usize = MAX_PAYLOAD_BYTES - 8;
    for (chunk_index, chunk) in data.chunks(CHUNK_SIZE).enumerate() {
        let offset = (chunk_index * CHUNK_SIZE) as u32;
        let mut chunk_payload = [0u8; MAX_PAYLOAD_BYTES];
        let write_req = binbook_diagnostic_protocol::StoreUploadWriteRequest {
            upload_id,
            offset,
            data: chunk,
        };
        let wplen = encode_store_upload_write_request(&write_req, &mut chunk_payload).unwrap();
        let chunk_header = FrameHeader {
            kind: FrameKind::Request,
            opcode: Opcode::StoreUploadWrite,
            status: Status::Ok,
            sequence: 2 + chunk_index as u16,
            payload_len: wplen as u16,
        };
        let mut chunk_frame = [0u8; MAX_FRAME_BYTES];
        let cflen = encode_frame(&chunk_header, &chunk_payload[..wplen], &mut chunk_frame).unwrap();
        let cresp = match session.send_and_receive(
            &chunk_frame[..cflen],
            Opcode::StoreUploadWrite,
            2 + chunk_index as u16,
            STORAGE_REQUEST_TIMEOUT,
        ) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Upload write chunk {chunk_index} failed: {e}");
                std::process::exit(1);
            }
        };
        let mut cresp_payload = [0u8; MAX_PAYLOAD_BYTES];
        let (_cresp_header, cresp_plen) = decode_frame(&cresp, &mut cresp_payload).unwrap();
        let _accepted = match decode_store_upload_write_response(&cresp_payload[..cresp_plen]) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("Invalid write response for chunk {chunk_index}: {e:?}");
                std::process::exit(1);
            }
        };
    }

    let mut commit_payload = [0u8; MAX_PAYLOAD_BYTES];
    let cplen = encode_store_upload_commit_request(upload_id, &mut commit_payload).unwrap();
    let commit_header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::StoreUploadCommit,
        status: Status::Ok,
        sequence: 100,
        payload_len: cplen as u16,
    };
    let mut commit_frame = [0u8; MAX_FRAME_BYTES];
    let commit_flen =
        encode_frame(&commit_header, &commit_payload[..cplen], &mut commit_frame).unwrap();
    let commit_resp = match session.send_and_receive(
        &commit_frame[..commit_flen],
        Opcode::StoreUploadCommit,
        100,
        STORAGE_REQUEST_TIMEOUT,
    ) {
        Ok(response) => response,
        Err(e) => {
            eprintln!("Upload commit failed: {e}");
            std::process::exit(1);
        }
    };
    let mut commit_resp_payload = [0u8; MAX_PAYLOAD_BYTES];
    let (_commit_header, commit_resp_len) =
        decode_frame(&commit_resp, &mut commit_resp_payload).unwrap();
    let commit = match decode_store_upload_commit_response(&commit_resp_payload[..commit_resp_len])
    {
        Ok(value) => value,
        Err(e) => {
            eprintln!("Invalid upload commit response: {e:?}");
            std::process::exit(1);
        }
    };
    if commit.result != StoreUploadCommitResult::Ok {
        eprintln!(
            "Upload CRC mismatch: expected 0x{crc32:08X}, computed 0x{:08X}",
            commit.computed_crc32
        );
        std::process::exit(1);
    }
    println!(
        "Uploaded {} ({} bytes, CRC32=0x{:08X}) as {}",
        file.display(),
        data.len(),
        crc32,
        path
    );
}

fn crc32_simple(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}
