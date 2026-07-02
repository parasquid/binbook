use binbook_diagnostic_protocol::{
    decode_frame, decode_store_list_entry, FrameKind, Opcode, Status, CAP_STORAGE, MAX_FRAME_BYTES,
};

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
        decode_log_response_header, decode_status_payload, CAP_CRASH, CAP_DISPLAY_PROBE, CAP_KEY,
        CAP_LOG, CAP_PAGE, CAP_STATUS, LOG_RECORD_BYTES, LOG_RESPONSE_HEADER_BYTES,
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
                    (CAP_STORAGE, "STORAGE"),
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
            Opcode::StoreList => {
                if payload.len() < 4 {
                    return Err("invalid StoreList response: too short".into());
                }
                let count = u32::from_le_bytes(payload[..4].try_into().unwrap());
                let mut text = format!("entry_count={}", count);
                let mut pos = 4usize;
                while pos < payload.len() {
                    match decode_store_list_entry(&payload[pos..]) {
                        Ok((name_len_hint, name_bytes, flags)) => {
                            let name = core::str::from_utf8(name_bytes).unwrap_or("<invalid utf8>");
                            text.push_str(&format!("\n  {} flags={}", name, flags));
                            pos += 2 + name_len_hint as usize + 4;
                        }
                        Err(_) => break,
                    }
                }
                Ok(text)
            }
            Opcode::StoreRead => {
                if payload.is_empty() {
                    return Err("file not found or empty".into());
                }
                Ok(format!("data_len={}", payload.len()))
            }
            Opcode::StoreDelete | Opcode::StoreUploadCommit | Opcode::StoreAbort => {
                if !payload.is_empty() { return Err("unexpected response payload".into()); }
                Ok("ok".into())
            }
            _ => Err(format!("unsupported opcode {:?}", header.opcode)),
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
        EVT_REQUEST_ENQUEUE => "REQUEST_ENQUEUE",
        EVT_REQUEST_RECEIVE => "REQUEST_RECEIVE",
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
        EVT_DISPLAY_REQUEST_START => "DISPLAY_REQUEST_START",
        EVT_DISPLAY_REQUEST_END => "DISPLAY_REQUEST_END",
        EVT_PAGE_METADATA_READ => "PAGE_METADATA_READ",
        EVT_PLANE_WRITE_START => "PLANE_WRITE_START",
        EVT_PLANE_ROW_FILL_SUMMARY => "PLANE_ROW_FILL_SUMMARY",
        EVT_PLANE_SPI_WRITE_SUMMARY => "PLANE_SPI_WRITE_SUMMARY",
        EVT_PLANE_WRITE_END => "PLANE_WRITE_END",
        EVT_CONTROLLER_RAM_STATE => "CONTROLLER_RAM_STATE",
        EVT_WAVEFORM_SELECTED => "WAVEFORM_SELECTED",
        EVT_BUSY_WAIT_START => "BUSY_WAIT_START",
        EVT_BUSY_WAIT_END => "BUSY_WAIT_END",
        EVT_REFRESH_TRIGGER => "REFRESH_TRIGGER",
        EVT_DISPLAY_RECOVERY => "DISPLAY_RECOVERY",
        _ => "UNKNOWN",
    }
}
