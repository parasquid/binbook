pub const DISPLAY_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(70);
pub mod nav_burst;

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
            EVT_INPUT_TRANSITION => "INPUT_TRANSITION",
            EVT_INPUT_DECISION => "INPUT_DECISION",
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
            EVT_TURN_STARTED => "TURN_STARTED",
            EVT_TURN_BOUNDARY_NOOP => "TURN_BOUNDARY_NOOP",
            EVT_RESEED_START => "RESEED_START",
            EVT_RESEED_COMPLETE => "RESEED_COMPLETE",
            EVT_GRAY_DELAY_CANCELLED => "GRAY_DELAY_CANCELLED",
            EVT_GRAY_OVERLAY_START => "GRAY_OVERLAY_START",
            EVT_GRAY_OVERLAY_CANCELLED => "GRAY_OVERLAY_CANCELLED",
            EVT_GRAY_OVERLAY_ACTIVATE => "GRAY_OVERLAY_ACTIVATE",
            EVT_GRAY_OVERLAY_COMPLETE => "GRAY_OVERLAY_COMPLETE",
            EVT_BW_BASE_SYNC_START => "BW_BASE_SYNC_START",
            EVT_BW_BASE_SYNC_CANCELLED => "BW_BASE_SYNC_CANCELLED",
            EVT_BW_BASE_SYNC_COMPLETE => "BW_BASE_SYNC_COMPLETE",
            EVT_CONTROLLER_RAM_STATE => "CONTROLLER_RAM_STATE",
            EVT_WAVEFORM_SELECTED => "WAVEFORM_SELECTED",
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

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ObservedResponse {
        pub sequence: u16,
        pub elapsed_ms: u128,
        pub frame: Vec<u8>,
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

    pub fn send_batch_and_receive_io<T: Read + Write>(
        io: &mut T,
        requests: &[u8],
        expected_opcode: Opcode,
        expected_sequences: &[u16],
        timeout: Duration,
    ) -> Result<Vec<Vec<u8>>, String> {
        let observed = send_batch_observed_io(
            io,
            requests,
            expected_opcode,
            expected_sequences,
            timeout,
            0,
        )?;
        expected_sequences
            .iter()
            .map(|sequence| {
                observed
                    .iter()
                    .find(|response| response.sequence == *sequence)
                    .map(|response| response.frame.clone())
                    .ok_or_else(|| format!("missing observed sequence {sequence}"))
            })
            .collect()
    }

    pub fn send_batch_observed_io<T: Read + Write>(
        io: &mut T,
        requests: &[u8],
        expected_opcode: Opcode,
        expected_sequences: &[u16],
        timeout: Duration,
        inter_key_ms: u64,
    ) -> Result<Vec<ObservedResponse>, String> {
        for (index, &sequence) in expected_sequences.iter().enumerate() {
            if expected_sequences[..index].contains(&sequence) {
                return Err(format!("duplicate expected sequence {sequence}"));
            }
        }

        let started = Instant::now();
        if inter_key_ms == 0 {
            io.write_all(requests)
                .map_err(|e| format!("write failed: {e}"))?;
            io.flush().map_err(|e| format!("flush failed: {e}"))?;
        } else {
            let frames = requests.split_inclusive(|byte| *byte == FRAME_DELIMITER);
            for frame in frames {
                if frame.last() != Some(&FRAME_DELIMITER) {
                    return Err("batch contains an incomplete request frame".into());
                }
                io.write_all(frame)
                    .map_err(|e| format!("write failed: {e}"))?;
                io.flush().map_err(|e| format!("flush failed: {e}"))?;
                std::thread::sleep(Duration::from_millis(inter_key_ms));
            }
        }

        let deadline = started + timeout;
        let mut buffered = Vec::new();
        let mut chunk = [0u8; 256];
        let mut responses = Vec::with_capacity(expected_sequences.len());

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

                let Some(_) = expected_sequences
                    .iter()
                    .position(|&sequence| sequence == header.sequence)
                else {
                    continue;
                };

                if header.kind != FrameKind::Response {
                    return Err("matching frame is not a response".into());
                }
                if header.opcode != expected_opcode {
                    return Err(format!("unexpected opcode {:?}", header.opcode));
                }
                if header.status != Status::Ok {
                    return Err(format!("device returned {:?}", header.status));
                }
                if responses
                    .iter()
                    .any(|response: &ObservedResponse| response.sequence == header.sequence)
                {
                    return Err(format!("duplicate sequence {}", header.sequence));
                }
                responses.push(ObservedResponse {
                    sequence: header.sequence,
                    elapsed_ms: started.elapsed().as_millis(),
                    frame,
                });
                if responses.len() == expected_sequences.len() {
                    return Ok(responses);
                }
            }
            if buffered.len() > MAX_FRAME_BYTES {
                if let Some(end) = buffered.iter().position(|&byte| byte == FRAME_DELIMITER) {
                    buffered.drain(..=end);
                } else {
                    buffered.clear();
                }
            }
        }

        let missing: Vec<String> = expected_sequences
            .iter()
            .filter(|sequence| {
                !responses
                    .iter()
                    .any(|response| response.sequence == **sequence)
            })
            .map(u16::to_string)
            .collect();
        if missing.is_empty() {
            Err("response timeout".into())
        } else {
            Err(format!(
                "response timeout missing sequences {}",
                missing.join(",")
            ))
        }
    }
}

#[cfg(feature = "serial-device")]
pub mod exercise {
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};

    use binbook_diagnostic_protocol::{
        decode_frame, decode_log_record, decode_log_response_header, decode_status_payload,
        KeyCode, LogRecordPayload, Opcode, StatusPayload, EVT_BW_BASE_SYNC_CANCELLED,
        EVT_BW_BASE_SYNC_COMPLETE, EVT_BW_BASE_SYNC_START, EVT_DISPLAY_ERROR,
        EVT_GRAY_OVERLAY_ACTIVATE, EVT_GRAY_OVERLAY_CANCELLED, EVT_GRAY_OVERLAY_COMPLETE,
        EVT_GRAY_OVERLAY_START, EVT_REFRESH_DECISION, EVT_RENDER_FAILURE, EVT_TURN_DEQUEUED,
        EVT_TURN_DROPPED, EVT_WAVEFORM_SELECTED, LOG_RECORD_BYTES, LOG_RESPONSE_HEADER_BYTES,
        MAX_PAYLOAD_BYTES,
    };

    use crate::{diag_protocol, serial_transport};

    const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
    const DISPLAY_REQUEST_TIMEOUT: Duration = Duration::from_secs(70);
    const BATCH_DISPLAY_TIMEOUT: Duration = Duration::from_secs(210);
    const LOG_POLL_INTERVAL: Duration = Duration::from_millis(500);
    const LOG_BUDGET: u16 = 496;
    const OVERLAY_CANCELLATION_OFFSET: Duration = Duration::from_millis(360);

    pub fn run_staged_gray(port: &str) -> Result<(), String> {
        let mut session = serialport::new(port, 115_200)
            .timeout(Duration::from_secs(2))
            .open()
            .map_err(|e| format!("Failed to open {port}: {e}"))?;
        run_staged_gray_io(&mut session, port)
    }

    pub fn run_staged_gray_io<T: Read + Write>(io: &mut T, _port: &str) -> Result<(), String> {
        let start = Instant::now();
        let mut cursor = 0u32;
        let mut evidence = Vec::new();

        let page_three = diag_protocol::page_goto_request(1, 3);
        validate_text(
            &serial_transport::send_and_receive_io(
                io,
                &page_three,
                Opcode::Page,
                1,
                DISPLAY_REQUEST_TIMEOUT,
            )?,
            Opcode::Page,
            1,
            "current_page=3",
        )?;
        phase("nonzero baseline", start);

        let page_zero = diag_protocol::page_goto_request(2, 0);
        validate_text(
            &serial_transport::send_and_receive_io(
                io,
                &page_zero,
                Opcode::Page,
                2,
                DISPLAY_REQUEST_TIMEOUT,
            )?,
            Opcode::Page,
            2,
            "current_page=0",
        )?;
        phase("page-0 baseline", start);

        let status = diag_protocol::status_request(3);
        validate_text(
            &serial_transport::send_and_receive_io(
                io,
                &status,
                Opcode::Status,
                3,
                REQUEST_TIMEOUT,
            )?,
            Opcode::Status,
            3,
            "current_page=0",
        )?;
        phase("status baseline", start);

        let clear = diag_protocol::log_clear_request(4);
        validate_text(
            &serial_transport::send_and_receive_io(
                io,
                &clear,
                Opcode::LogClear,
                4,
                REQUEST_TIMEOUT,
            )?,
            Opcode::LogClear,
            4,
            "record_count=0",
        )?;
        phase("clear logs", start);

        let key_right = diag_protocol::key_request(5, KeyCode::Right);
        validate_text(
            &serial_transport::send_and_receive_io(
                io,
                &key_right,
                Opcode::Key,
                5,
                DISPLAY_REQUEST_TIMEOUT,
            )?,
            Opcode::Key,
            5,
            "ok",
        )?;
        phase("prime gray", start);
        let poll_deadline = Instant::now() + DISPLAY_REQUEST_TIMEOUT;
        loop {
            let refresh_poll = diag_protocol::log_get_request(6, cursor, LOG_BUDGET);
            let refresh_response = serial_transport::send_and_receive_io(
                io,
                &refresh_poll,
                Opcode::LogGet,
                6,
                REQUEST_TIMEOUT,
            )?;
            let (next, mut records) = decode_log_evidence(&refresh_response, 6)?;
            cursor = next;
            evidence.append(&mut records);
            if evidence
                .iter()
                .any(|record| record.event == EVT_BW_BASE_SYNC_COMPLETE && record.arg0 == 1)
            {
                break;
            }
            if Instant::now() >= poll_deadline {
                return Err("timed out waiting for idle staged refinement".into());
            }
            std::thread::sleep(LOG_POLL_INTERVAL);
        }
        phase("idle staged refinement complete", start);

        let key_right = diag_protocol::key_request(7, KeyCode::Right);
        validate_text(
            &serial_transport::send_and_receive_io(
                io,
                &key_right,
                Opcode::Key,
                7,
                DISPLAY_REQUEST_TIMEOUT,
            )?,
            Opcode::Key,
            7,
            "ok",
        )?;
        let cancel_at = Instant::now() + OVERLAY_CANCELLATION_OFFSET;
        let overlay_poll = diag_protocol::log_get_request(10, cursor, LOG_BUDGET);
        let overlay_response = serial_transport::send_and_receive_io(
            io,
            &overlay_poll,
            Opcode::LogGet,
            10,
            REQUEST_TIMEOUT,
        )?;
        let (next, mut records) = decode_log_evidence(&overlay_response, 10)?;
        cursor = next;
        evidence.append(&mut records);
        if let Some(remaining) = cancel_at.checked_duration_since(Instant::now()) {
            std::thread::sleep(remaining);
        }
        phase("page-2 cancellation window", start);

        let mut batched = Vec::new();
        for (sequence, key) in [(8, KeyCode::Right), (9, KeyCode::Left)] {
            batched.extend_from_slice(&diag_protocol::key_request(sequence, key));
        }
        let batch_responses = serial_transport::send_batch_and_receive_io(
            io,
            &batched,
            Opcode::Key,
            &[8, 9],
            BATCH_DISPLAY_TIMEOUT,
        )?;
        for (frame, sequence) in batch_responses.into_iter().zip([8u16, 9]) {
            validate_text(&frame, Opcode::Key, sequence, "ok")?;
        }
        phase("batched turns", start);
        let poll_deadline = Instant::now() + DISPLAY_REQUEST_TIMEOUT;
        loop {
            let reseed_poll = diag_protocol::log_get_request(10, cursor, LOG_BUDGET);
            let reseed_response = serial_transport::send_and_receive_io(
                io,
                &reseed_poll,
                Opcode::LogGet,
                10,
                REQUEST_TIMEOUT,
            )?;
            let (next, mut records) = decode_log_evidence(&reseed_response, 10)?;
            cursor = next;
            evidence.append(&mut records);
            if evidence
                .iter()
                .any(|record| record.event == EVT_BW_BASE_SYNC_COMPLETE && record.arg0 == 2)
            {
                break;
            }
            if Instant::now() >= poll_deadline {
                return Err("timed out waiting for BW base-sync completion".into());
            }
            std::thread::sleep(LOG_POLL_INTERVAL);
        }
        phase("BW base sync complete", start);

        let key_right = diag_protocol::key_request(11, KeyCode::Right);
        validate_text(
            &serial_transport::send_and_receive_io(
                io,
                &key_right,
                Opcode::Key,
                11,
                DISPLAY_REQUEST_TIMEOUT,
            )?,
            Opcode::Key,
            11,
            "ok",
        )?;
        phase("final turn", start);

        let final_status = diag_protocol::status_request(12);
        let status_response = serial_transport::send_and_receive_io(
            io,
            &final_status,
            Opcode::Status,
            12,
            REQUEST_TIMEOUT,
        )?;
        let final_status = decode_status_evidence(&status_response, 12)?;
        phase("final status", start);

        let final_logs = diag_protocol::log_get_request(13, cursor, LOG_BUDGET);
        let final_response = serial_transport::send_and_receive_io(
            io,
            &final_logs,
            Opcode::LogGet,
            13,
            REQUEST_TIMEOUT,
        )?;
        let (_, mut records) = decode_log_evidence(&final_response, 13)?;
        evidence.append(&mut records);
        validate_staged_gray_evidence(final_status, &evidence)?;
        phase("final logs", start);

        Ok(())
    }

    fn validate_text(
        frame: &[u8],
        opcode: Opcode,
        sequence: u16,
        expected: &str,
    ) -> Result<String, String> {
        let text = diag_protocol::format_response(frame, opcode, sequence)?;
        if !text.contains(expected) {
            return Err(format!("missing expected marker {expected}"));
        }
        Ok(text)
    }

    fn decode_log_evidence(
        frame: &[u8],
        sequence: u16,
    ) -> Result<(u32, Vec<LogRecordPayload>), String> {
        let mut payload = [0u8; MAX_PAYLOAD_BYTES];
        let (header, len) =
            decode_frame(frame, &mut payload).map_err(|e| format!("invalid log frame: {e:?}"))?;
        if header.opcode != Opcode::LogGet || header.sequence != sequence {
            return Err(format!(
                "mismatched log response sequence {}",
                header.sequence
            ));
        }
        let body = &payload[..len];
        let log_header =
            decode_log_response_header(body).map_err(|e| format!("invalid log header: {e:?}"))?;
        let expected =
            LOG_RESPONSE_HEADER_BYTES + log_header.record_count as usize * LOG_RECORD_BYTES;
        if body.len() != expected {
            return Err("log record count does not match payload".into());
        }
        let mut records = Vec::with_capacity(log_header.record_count as usize);
        for chunk in body[LOG_RESPONSE_HEADER_BYTES..].chunks_exact(LOG_RECORD_BYTES) {
            records
                .push(decode_log_record(chunk).map_err(|e| format!("invalid log record: {e:?}"))?);
        }
        Ok((log_header.next_cursor, records))
    }

    fn decode_status_evidence(frame: &[u8], sequence: u16) -> Result<StatusPayload, String> {
        let mut payload = [0u8; MAX_PAYLOAD_BYTES];
        let (header, len) = decode_frame(frame, &mut payload)
            .map_err(|e| format!("invalid status frame: {e:?}"))?;
        if header.opcode != Opcode::Status || header.sequence != sequence {
            return Err(format!(
                "mismatched status response sequence {}",
                header.sequence
            ));
        }
        decode_status_payload(&payload[..len]).map_err(|e| format!("invalid status payload: {e:?}"))
    }

    pub fn validate_staged_gray_evidence(
        status: StatusPayload,
        records: &[LogRecordPayload],
    ) -> Result<(), String> {
        if status.current_page != 3 {
            return Err(format!("final page must be 3, got {}", status.current_page));
        }
        if status.dropped_log_count != 0 || status.protocol_error_count != 0 {
            return Err("diagnostic counters must remain zero".into());
        }
        if status.last_error != 0 {
            return Err(format!(
                "last_error must remain zero, got {}",
                status.last_error
            ));
        }
        if records
            .iter()
            .any(|record| record.event == EVT_TURN_DROPPED)
        {
            return Err("TURN_DROPPED observed".into());
        }
        if records
            .iter()
            .any(|record| matches!(record.event, EVT_RENDER_FAILURE | EVT_DISPLAY_ERROR))
        {
            return Err("display failure observed".into());
        }

        let prime = find_completion(records, 5, 1)?;
        let overlay_start_index = records
            .iter()
            .position(|record| record.event == EVT_GRAY_OVERLAY_START && record.arg0 == 1)
            .ok_or_else(|| "missing staged overlay start for page 1".to_string())?;
        if records[overlay_start_index].tick_ms < prime.tick_ms.saturating_add(350) {
            return Err("staged overlay started less than 350 ms after BW completion".into());
        }
        let waveform_index = records
            .iter()
            .position(|record| {
                record.event == EVT_WAVEFORM_SELECTED && record.arg0 == 2 && record.arg1 == 1
            })
            .ok_or_else(|| "missing waveform hint 2 / LUT revision 1 evidence".to_string())?;
        let activate_index = records
            .iter()
            .position(|record| record.event == EVT_GRAY_OVERLAY_ACTIVATE && record.arg0 == 1)
            .ok_or_else(|| "missing staged overlay activation for page 1".to_string())?;
        let overlay_complete_index = records
            .iter()
            .position(|record| record.event == EVT_GRAY_OVERLAY_COMPLETE && record.arg0 == 1)
            .ok_or_else(|| "missing staged overlay completion for page 1".to_string())?;
        if !(overlay_start_index < waveform_index
            && waveform_index < activate_index
            && activate_index < overlay_complete_index)
        {
            return Err("staged overlay events are missing or reordered".into());
        }
        if records.iter().any(|record| {
            record.event == EVT_REFRESH_DECISION
                && [record.arg0, record.arg1, record.arg2]
                    .iter()
                    .any(|value| matches!(*value, 0xF7 | 0xC7))
        }) {
            return Err("full or absolute-grayscale activation observed".into());
        }

        let queued = [
            completion_index(records, 7, 2)?,
            completion_index(records, 8, 3)?,
            completion_index(records, 9, 2)?,
        ];
        if !(queued[0] < queued[1] && queued[1] < queued[2]) {
            return Err("queued pages were not completed in exact order 2,3,2".into());
        }

        let cancel_index = records
            .iter()
            .position(|record| record.event == EVT_GRAY_OVERLAY_CANCELLED && record.arg0 == 2)
            .ok_or_else(|| "missing cancellation of the queued page-2 overlay".to_string())?;
        let restarted_index = records
            .iter()
            .enumerate()
            .skip(cancel_index + 1)
            .find(|(_, record)| record.event == EVT_GRAY_OVERLAY_START && record.arg0 == 2)
            .map(|(index, _)| index)
            .unwrap_or(records.len());
        if records[cancel_index + 1..restarted_index]
            .iter()
            .any(|record| record.event == EVT_GRAY_OVERLAY_COMPLETE && record.arg0 == 2)
        {
            return Err("canceled page emitted a grayscale completion".into());
        }

        let sync_start_index = records
            .iter()
            .position(|record| record.event == EVT_BW_BASE_SYNC_START && record.arg0 == 2)
            .ok_or_else(|| "missing BW base-sync start for page 2".to_string())?;
        let sync_complete_index = records
            .iter()
            .enumerate()
            .skip(sync_start_index + 1)
            .find(|(_, record)| record.event == EVT_BW_BASE_SYNC_COMPLETE && record.arg0 == 2)
            .map(|(index, _)| index)
            .ok_or_else(|| "missing BW base-sync completion for page 2".to_string())?;
        if records[sync_start_index..=sync_complete_index]
            .iter()
            .any(|record| {
                matches!(
                    record.event,
                    EVT_GRAY_OVERLAY_ACTIVATE | EVT_REFRESH_DECISION
                )
            })
        {
            return Err("visible activation occurred during background BW base sync".into());
        }
        if records.iter().any(|record| {
            record.event == EVT_BW_BASE_SYNC_CANCELLED
                && record.arg0 == 2
                && record.sequence > records[sync_complete_index].sequence
        }) {
            return Err("completed base sync was later reported canceled".into());
        }
        find_completion(records, 11, 3)?;
        Ok(())
    }

    fn find_completion<'a>(
        records: &'a [LogRecordPayload],
        sequence: i32,
        page: i32,
    ) -> Result<&'a LogRecordPayload, String> {
        records
            .iter()
            .find(|record| {
                record.event == EVT_TURN_DEQUEUED && record.arg0 == sequence && record.arg1 == page
            })
            .ok_or_else(|| format!("missing completion sequence={sequence} page={page}"))
    }

    fn completion_index(
        records: &[LogRecordPayload],
        sequence: i32,
        page: i32,
    ) -> Result<usize, String> {
        records
            .iter()
            .position(|record| {
                record.event == EVT_TURN_DEQUEUED && record.arg0 == sequence && record.arg1 == page
            })
            .ok_or_else(|| format!("missing completion sequence={sequence} page={page}"))
    }

    fn phase(name: &str, start: Instant) {
        println!(
            "[exercise] phase={name} elapsed_ms={}",
            start.elapsed().as_millis()
        );
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
    Exercise {
        #[command(subcommand)]
        exercise: ExerciseCommand,
    },
}

#[derive(Subcommand)]
pub enum ExerciseCommand {
    StagedGray {
        #[arg(short, long)]
        port: String,
    },
    NavBurst {
        #[arg(short, long)]
        port: String,
        #[arg(long, default_value_t = 10, value_parser = parse_rounds)]
        rounds: u16,
        #[arg(long, default_value_t = 0)]
        inter_key_ms: u64,
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

fn parse_rounds(value: &str) -> Result<u16, String> {
    let rounds = value
        .parse::<u16>()
        .map_err(|error| format!("invalid rounds: {error}"))?;
    if (1..=100).contains(&rounds) {
        Ok(rounds)
    } else {
        Err("rounds must be between 1 and 100".into())
    }
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
