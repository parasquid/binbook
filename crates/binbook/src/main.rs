use binbook::{Cli, Commands, DiagCommand};
#[cfg(feature = "serial-device")]
use binbook::{ExerciseCommand, PageAction, ProbeCommand, StorageCommand};
use clap::Parser;

#[cfg(feature = "serial-device")]
fn run_diag(cmd: DiagCommand) {
    use binbook::diag_protocol;
    use binbook::serial_transport::SerialSession;
    use binbook_diagnostic_protocol::{KeyCode, Opcode};
    use std::time::Duration;

    if let DiagCommand::Exercise { exercise } = &cmd {
        match exercise {
            ExerciseCommand::StagedGray { port } => {
                if let Err(error) = binbook::exercise::run_staged_gray(port) {
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
                if let Err(error) = binbook::nav_burst::run_nav_burst(
                    port,
                    *rounds,
                    *inter_key_ms,
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
            };
            (
                port.clone(),
                diag_protocol::display_probe_request(1, choice),
                Opcode::DisplayProbe,
                binbook::DISPLAY_PROBE_TIMEOUT,
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
                    Duration::from_secs(5),
                )
            }
            StorageCommand::Read {
                port,
                path,
                output: _output,
            } => {
                let frame = diag_protocol::store_read_request(1, path);
                (
                    port.clone(),
                    frame,
                    Opcode::StoreRead,
                    Duration::from_secs(10),
                )
            }
            StorageCommand::Delete { port, path } => {
                let frame = diag_protocol::store_delete_request(1, path);
                (
                    port.clone(),
                    frame,
                    Opcode::StoreDelete,
                    Duration::from_secs(5),
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

#[cfg(feature = "serial-device")]
fn upload_file(port: &str, path: &str, file: &std::path::Path) {
    use binbook::serial_transport::SerialSession;
    use binbook_diagnostic_protocol::{
        decode_frame, decode_store_upload_begin_response, decode_store_upload_write_response,
        encode_frame, encode_store_upload_begin_request, encode_store_upload_commit_request,
        encode_store_upload_write_request, FrameHeader, FrameKind, Opcode, Status, StorageBackend,
        StoreUploadBeginRequest, MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES,
    };
    use std::time::Duration;

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
        path,
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
        Duration::from_secs(5),
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

    const CHUNK_SIZE: usize = 4096;
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
            Duration::from_secs(5),
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
    match session.send_and_receive(
        &commit_frame[..commit_flen],
        Opcode::StoreUploadCommit,
        100,
        Duration::from_secs(5),
    ) {
        Ok(_) => {
            println!(
                "Uploaded {} ({} bytes, CRC32=0x{:08X}) as {}",
                file.display(),
                data.len(),
                crc32,
                path
            );
        }
        Err(e) => {
            eprintln!("Upload commit failed: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(feature = "serial-device")]
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

#[cfg(not(feature = "serial-device"))]
fn run_diag(cmd: DiagCommand) {
    let _ = cmd;
    eprintln!("serial-device feature is not enabled. Rebuild with --features serial-device.");
    std::process::exit(1);
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Encode {
            input,
            output,
            input_format,
            profile,
            pixel_format,
            no_dither,
            font_family,
        } => exit_on_error(binbook::run_encode(
            &input,
            &output,
            input_format,
            profile,
            pixel_format,
            no_dither,
            font_family,
        )),
        Commands::Decode { book, page, output } => {
            exit_on_error(binbook::run_decode(&book, page, &output));
        }
        Commands::Inspect {
            book,
            validate,
            strict,
            json,
        } => exit_on_error(binbook::run_inspect(&book, validate, strict, json)),
        Commands::Flash { port, firmware } => {
            println!("Flashing {} to {}...", firmware.display(), port);
        }
        Commands::Upload { port, file, name } => {
            println!("Uploading {} as {} to {}...", file.display(), name, port);
        }
        Commands::List { port } => {
            println!("Listing books on {}...", port);
        }
        Commands::Delete { port, name } => {
            println!("Deleting {} from {}...", name, port);
        }
        Commands::Diag(cmd) => run_diag(cmd),
    }
}

fn exit_on_error(result: Result<(), binbook::CliError>) {
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
