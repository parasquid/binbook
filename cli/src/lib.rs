pub mod diag_protocol {
    use binbook_diagnostic_protocol::{
        decode_frame, encode_frame, encode_log_get_payload, encode_page_payload,
        encode_probe_payload, FrameHeader, FrameKind, KeyAction, KeyCode, LogGetPayload, Opcode,
        PageAction, ProbeCode, Status, MAX_FRAME_BYTES,
    };

    fn request_header(sequence: u16, opcode: Opcode) -> FrameHeader {
        FrameHeader {
            kind: FrameKind::Request,
            opcode,
            status: Status::Ok,
            sequence,
            payload_len: 0,
        }
    }

    pub fn hello_request(sequence: u16) -> Vec<u8> {
        let header = request_header(sequence, Opcode::Hello);
        let mut buf = [0u8; MAX_FRAME_BYTES];
        let len = encode_frame(&header, &[], &mut buf).unwrap();
        buf[..len].to_vec()
    }

    pub fn key_request(sequence: u16, key: KeyCode) -> Vec<u8> {
        let mut header = request_header(sequence, Opcode::Key);
        header.payload_len = 2;
        let payload = [key as u8, KeyAction::Press as u8];
        let mut buf = [0u8; MAX_FRAME_BYTES];
        let len = encode_frame(&header, &payload, &mut buf).unwrap();
        buf[..len].to_vec()
    }

    pub fn page_goto_request(sequence: u16, page: u32) -> Vec<u8> {
        let mut header = request_header(sequence, Opcode::Page);
        let mut payload_buf = [0u8; 5];
        let plen = encode_page_payload(PageAction::Goto, Some(page), &mut payload_buf).unwrap();
        header.payload_len = plen as u16;
        let mut buf = [0u8; MAX_FRAME_BYTES];
        let len = encode_frame(&header, &payload_buf[..plen], &mut buf).unwrap();
        buf[..len].to_vec()
    }

    pub fn page_action_request(sequence: u16, action: PageAction) -> Vec<u8> {
        let mut header = request_header(sequence, Opcode::Page);
        let mut payload_buf = [0u8; 5];
        let plen = encode_page_payload(action, None, &mut payload_buf).unwrap();
        header.payload_len = plen as u16;
        let mut buf = [0u8; MAX_FRAME_BYTES];
        let len = encode_frame(&header, &payload_buf[..plen], &mut buf).unwrap();
        buf[..len].to_vec()
    }

    pub fn status_request(sequence: u16) -> Vec<u8> {
        let header = request_header(sequence, Opcode::Status);
        let mut buf = [0u8; MAX_FRAME_BYTES];
        let len = encode_frame(&header, &[], &mut buf).unwrap();
        buf[..len].to_vec()
    }

    pub fn log_get_request(sequence: u16, cursor: u32, max_bytes: u16) -> Vec<u8> {
        let mut header = request_header(sequence, Opcode::LogGet);
        let mut payload_buf = [0u8; 6];
        let plen = encode_log_get_payload(
            LogGetPayload {
                cursor_sequence: cursor,
                max_bytes,
            },
            &mut payload_buf,
        )
        .unwrap();
        header.payload_len = plen as u16;
        let mut buf = [0u8; MAX_FRAME_BYTES];
        let len = encode_frame(&header, &payload_buf[..plen], &mut buf).unwrap();
        buf[..len].to_vec()
    }

    pub fn log_clear_request(sequence: u16) -> Vec<u8> {
        let header = request_header(sequence, Opcode::LogClear);
        let mut buf = [0u8; MAX_FRAME_BYTES];
        let len = encode_frame(&header, &[], &mut buf).unwrap();
        buf[..len].to_vec()
    }

    pub fn crash_get_request(sequence: u16) -> Vec<u8> {
        let header = request_header(sequence, Opcode::CrashGet);
        let mut buf = [0u8; MAX_FRAME_BYTES];
        let len = encode_frame(&header, &[], &mut buf).unwrap();
        buf[..len].to_vec()
    }

    pub fn crash_clear_request(sequence: u16) -> Vec<u8> {
        let header = request_header(sequence, Opcode::CrashClear);
        let mut buf = [0u8; MAX_FRAME_BYTES];
        let len = encode_frame(&header, &[], &mut buf).unwrap();
        buf[..len].to_vec()
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ProbeChoice {
        FullRefreshCurrent,
        ClearWhite,
        WindowCorners,
    }

    impl ProbeChoice {
        pub fn to_probe_code(self) -> ProbeCode {
            match self {
                Self::FullRefreshCurrent => ProbeCode::FullRefreshCurrent,
                Self::ClearWhite => ProbeCode::ClearWhite,
                Self::WindowCorners => ProbeCode::WindowCorners,
            }
        }
    }

    pub fn display_probe_request(sequence: u16, probe: ProbeChoice) -> Vec<u8> {
        let mut header = request_header(sequence, Opcode::DisplayProbe);
        let mut payload_buf = [0u8; 1];
        let plen = encode_probe_payload(probe.to_probe_code(), &mut payload_buf).unwrap();
        header.payload_len = plen as u16;
        let mut buf = [0u8; MAX_FRAME_BYTES];
        let len = encode_frame(&header, &payload_buf[..plen], &mut buf).unwrap();
        buf[..len].to_vec()
    }

    pub struct StatusResponse {
        pub current_page: u32,
        pub page_count: u32,
        pub dropped_log_count: u32,
        pub protocol_error_count: u32,
        pub panel_mode: u8,
        pub last_error: i32,
    }

    pub fn decode_status_response(
        frame: &[u8],
    ) -> Result<StatusResponse, binbook_diagnostic_protocol::ProtocolError> {
        let mut payload_buf = [0u8; 256];
        let (header, payload_len) = decode_frame(frame, &mut payload_buf)?;

        if header.opcode != Opcode::Status {
            return Err(binbook_diagnostic_protocol::ProtocolError::UnknownOpcode);
        }
        if header.kind != FrameKind::Response {
            return Err(binbook_diagnostic_protocol::ProtocolError::BadMagic);
        }

        let sp = &payload_buf[..payload_len];
        let decoded = binbook_diagnostic_protocol::decode_status_payload(sp)?;
        Ok(StatusResponse {
            current_page: decoded.current_page,
            page_count: decoded.page_count,
            dropped_log_count: decoded.dropped_log_count,
            protocol_error_count: decoded.protocol_error_count,
            panel_mode: decoded.panel_mode as u8,
            last_error: decoded.last_error,
        })
    }

    pub fn decode_hello_response_payload(
        payload: &[u8],
    ) -> Result<
        binbook_diagnostic_protocol::HelloResponseRef<'_>,
        binbook_diagnostic_protocol::ProtocolError,
    > {
        binbook_diagnostic_protocol::decode_hello_response(payload)
    }

    pub fn format_response(
        frame: &[u8],
        expected_opcode: Opcode,
        expected_sequence: u16,
    ) -> Result<String, String> {
        use binbook_diagnostic_protocol::{
            decode_crash_response, decode_hello_response, decode_log_record,
            decode_log_response_header, decode_status_payload, CAP_CRASH, CAP_DISPLAY_PROBE,
            CAP_KEY, CAP_LOG, CAP_PAGE, CAP_STATUS, LOG_RECORD_BYTES, LOG_RESPONSE_HEADER_BYTES,
        };
        let mut payload = [0u8; MAX_FRAME_BYTES];
        let (header, payload_len) =
            decode_frame(frame, &mut payload).map_err(|e| format!("decode error: {e:?}"))?;
        if header.kind != FrameKind::Response {
            return Err("expected response frame".into());
        }
        if header.sequence != expected_sequence {
            return Err(format!("unexpected sequence {}", header.sequence));
        }
        if header.opcode != expected_opcode {
            return Err(format!("unexpected opcode {:?}", header.opcode));
        }
        if header.status != Status::Ok {
            return Err(format!("device returned {:?}", header.status));
        }
        let payload = &payload[..payload_len];
        match header.opcode {
            Opcode::Hello => {
                let hello = decode_hello_response(payload).map_err(|e| format!("invalid HELLO payload: {e:?}"))?;
                let mut names = Vec::new();
                for (bit, name) in [
                    (CAP_KEY, "KEY"), (CAP_PAGE, "PAGE"), (CAP_STATUS, "STATUS"),
                    (CAP_LOG, "LOG"), (CAP_CRASH, "CRASH"), (CAP_DISPLAY_PROBE, "DISPLAY_PROBE"),
                ] { if hello.capabilities & bit != 0 { names.push(name); } }
                Ok(format!(
                    "protocol={} max_frame={} capabilities={} firmware={} target={}",
                    hello.protocol_version,
                    hello.max_frame_bytes,
                    names.join(","),
                    core::str::from_utf8(hello.firmware_name).map_err(|_| "invalid firmware identity")?,
                    core::str::from_utf8(hello.target).map_err(|_| "invalid target identity")?,
                ))
            }
            Opcode::Status => {
                let value = decode_status_payload(payload).map_err(|e| format!("invalid STATUS payload: {e:?}"))?;
                Ok(format!(
                    "current_page={} page_count={} panel_mode={:?} dropped_log_count={} protocol_error_count={} last_error={}",
                    value.current_page, value.page_count, value.panel_mode, value.dropped_log_count,
                    value.protocol_error_count, value.last_error,
                ))
            }
            Opcode::Page => {
                if payload.len() != 4 { return Err("invalid PAGE response length".into()); }
                Ok(format!("current_page={}", u32::from_le_bytes(payload.try_into().unwrap())))
            }
            Opcode::LogGet | Opcode::LogClear => {
                let log_header = decode_log_response_header(payload).map_err(|e| format!("invalid LOG payload: {e:?}"))?;
                let expected = LOG_RESPONSE_HEADER_BYTES + log_header.record_count as usize * LOG_RECORD_BYTES;
                if payload.len() != expected { return Err("invalid LOG record count".into()); }
                let mut text = format!(
                    "next_cursor={} dropped_log_count={} record_count={}",
                    log_header.next_cursor, log_header.dropped_log_count, log_header.record_count,
                );
                for record_bytes in payload[LOG_RESPONSE_HEADER_BYTES..].chunks_exact(LOG_RECORD_BYTES) {
                    let record = decode_log_record(record_bytes).map_err(|e| format!("invalid log record: {e:?}"))?;
                    text.push_str(&format!(
                        "\nseq={} tick_ms={} level={} subsystem={} event={} arg0={} arg1={} arg2={}",
                        record.sequence, record.tick_ms, record.level, record.subsystem,
                        event_name(record.event), record.arg0, record.arg1, record.arg2,
                    ));
                }
                Ok(text)
            }
            Opcode::CrashGet => match decode_crash_response(payload).map_err(|e| format!("invalid CRASH payload: {e:?}"))? {
                None => Ok("crash=empty".into()),
                Some(summary) => Ok(format!(
                    "crash=present version={} flags={} copied_logs={} panel_mode={} last_error={} last_page={}",
                    summary[4], summary[5], summary[6], summary[7],
                    i32::from_le_bytes(summary[12..16].try_into().unwrap()),
                    u32::from_le_bytes(summary[16..20].try_into().unwrap()),
                )),
            },
            Opcode::Key | Opcode::CrashClear | Opcode::DisplayProbe => {
                if !payload.is_empty() { return Err("unexpected response payload".into()); }
                Ok("ok".into())
            }
        }
    }

    fn event_name(event: u16) -> &'static str {
        use binbook_diagnostic_protocol::*;
        match event {
            EVT_FIRMWARE_STARTED => "FIRMWARE_STARTED",
            EVT_CMD_RECEIPT => "CMD_RECEIPT",
            EVT_CMD_ERROR => "CMD_ERROR",
            EVT_KEY_PRESS => "KEY_PRESS",
            EVT_BUTTON_EVENT => "BUTTON_EVENT",
            EVT_PAGE_DECISION => "PAGE_DECISION",
            EVT_PAGE_TURN => "PAGE_TURN",
            EVT_RENDER_START => "RENDER_START",
            EVT_RENDER_SUCCESS => "RENDER_SUCCESS",
            EVT_RENDER_FAILURE => "RENDER_FAILURE",
            EVT_REFRESH_DECISION => "REFRESH_DECISION",
            EVT_REFRESH_PHASE => "REFRESH_PHASE",
            EVT_PANEL_MODE => "PANEL_MODE",
            EVT_ADC_SAMPLE => "ADC_SAMPLE",
            EVT_IDLE_ENTERED => "IDLE_ENTERED",
            EVT_IDLE_SUMMARY => "IDLE_SUMMARY",
            EVT_IDLE_LEFT => "IDLE_LEFT",
            EVT_DISPLAY_ERROR => "DISPLAY_ERROR",
            EVT_TURN_QUEUED => "TURN_QUEUED",
            EVT_TURN_DEQUEUED => "TURN_DEQUEUED",
            EVT_TURN_DROPPED => "TURN_DROPPED",
            EVT_RESEED_START => "RESEED_START",
            EVT_RESEED_COMPLETE => "RESEED_COMPLETE",
            EVT_DISPLAY_RECOVERY => "DISPLAY_RECOVERY",
            _ => "UNKNOWN",
        }
    }
}

#[cfg(feature = "serial-device")]
pub mod serial_transport {
    use binbook_diagnostic_protocol::{
        decode_frame, FrameKind, Opcode, Status, FRAME_DELIMITER, MAX_FRAME_BYTES,
    };
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};

    pub struct SerialSession {
        port: Box<dyn serialport::SerialPort>,
    }

    impl SerialSession {
        pub fn open(port_path: &str) -> Result<Self, String> {
            let port = serialport::new(port_path, 115_200)
                .timeout(Duration::from_secs(2))
                .open()
                .map_err(|e| format!("Failed to open {port_path}: {e}"))?;
            Ok(Self { port })
        }

        pub fn send_and_receive(
            &mut self,
            frame: &[u8],
            opcode: Opcode,
            sequence: u16,
            timeout: Duration,
        ) -> Result<Vec<u8>, String> {
            send_and_receive_io(&mut self.port, frame, opcode, sequence, timeout)
        }
    }

    pub fn send_and_receive_io<T: Read + Write>(
        io: &mut T,
        request: &[u8],
        expected_opcode: Opcode,
        expected_sequence: u16,
        timeout: Duration,
    ) -> Result<Vec<u8>, String> {
        io.write_all(request)
            .map_err(|e| format!("write failed: {e}"))?;
        io.flush().map_err(|e| format!("flush failed: {e}"))?;
        let deadline = Instant::now() + timeout;
        let mut buffered = Vec::new();
        let mut chunk = [0u8; 256];
        while Instant::now() < deadline {
            match io.read(&mut chunk) {
                Ok(0) => std::thread::yield_now(),
                Ok(count) => buffered.extend_from_slice(&chunk[..count]),
                Err(error)
                    if error.kind() == std::io::ErrorKind::TimedOut
                        || error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(format!("read failed: {error}")),
            }
            while let Some(end) = buffered.iter().position(|&byte| byte == FRAME_DELIMITER) {
                let frame: Vec<u8> = buffered.drain(..=end).collect();
                if frame.len() > MAX_FRAME_BYTES {
                    continue;
                }
                let mut payload = [0u8; MAX_FRAME_BYTES];
                let Ok((header, _)) = decode_frame(&frame, &mut payload) else {
                    continue;
                };
                if header.sequence != expected_sequence {
                    continue;
                }
                if header.kind != FrameKind::Response {
                    return Err("matching frame is not a response".into());
                }
                if header.opcode != expected_opcode {
                    return Err(format!("unexpected opcode {:?}", header.opcode));
                }
                if header.status != Status::Ok {
                    return Err(format!("device returned {:?}", header.status));
                }
                return Ok(frame);
            }
            if buffered.len() > MAX_FRAME_BYTES {
                if let Some(end) = buffered.iter().position(|&byte| byte == FRAME_DELIMITER) {
                    buffered.drain(..=end);
                } else {
                    buffered.clear();
                }
            }
        }
        Err("response timeout without matching sequence".into())
    }
}

pub mod protocol {
    pub fn list_command() -> String {
        "LIST\n".to_owned()
    }

    pub fn delete_command(name: &str) -> String {
        format!("DELETE {name}\n")
    }

    pub fn upload_command(name: &str, size: u64) -> String {
        format!("UPLOAD {name} {size}\n")
    }
}

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "binbook-cli")]
#[command(about = "CLI tool for BinBook device management")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Flash {
        #[arg(short, long)]
        port: String,
        #[arg(short, long)]
        firmware: PathBuf,
    },
    Upload {
        #[arg(short, long)]
        port: String,
        #[arg(short, long)]
        file: PathBuf,
        #[arg(short, long)]
        name: String,
    },
    List {
        #[arg(short, long)]
        port: String,
    },
    Delete {
        #[arg(short, long)]
        port: String,
        #[arg(short, long)]
        name: String,
    },
    #[command(subcommand)]
    Diag(DiagCommand),
}

#[derive(Subcommand)]
pub enum DiagCommand {
    Hello {
        #[arg(short, long)]
        port: String,
    },
    Key {
        #[arg(short, long)]
        port: String,
        #[arg(value_parser = ["LEFT", "RIGHT", "UP", "DOWN", "SELECT", "BACK", "POWER"])]
        key: String,
    },
    Page {
        #[arg(short, long)]
        port: String,
        #[command(subcommand)]
        action: PageAction,
    },
    Status {
        #[arg(short, long)]
        port: String,
    },
    Logs {
        #[arg(short, long)]
        port: String,
        #[arg(long)]
        since: Option<u32>,
        #[arg(long)]
        clear: bool,
    },
    Crash {
        #[arg(short, long)]
        port: String,
        #[arg(long)]
        clear: bool,
    },
    Probe {
        #[arg(short, long)]
        port: String,
        #[command(subcommand)]
        probe: ProbeCommand,
    },
}

#[derive(Subcommand)]
pub enum PageAction {
    Next,
    Previous,
    First,
    Last,
    Current,
    Goto { page: u32 },
}

#[derive(Subcommand)]
pub enum ProbeCommand {
    WindowCorners,
    ClearWhite,
    FullRefreshCurrent,
}
