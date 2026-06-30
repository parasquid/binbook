//! BinBook diagnostic serial protocol for Xteink X4 firmware.
//!
//! This crate provides COBS-framed binary packet encoding/decoding for the
//! diagnostic serial console. It is designed for no_std firmware use with
//! caller-owned buffers and no heap allocation.

#![no_std]

pub const PROTOCOL_VERSION: u8 = 1;
pub const MAX_FRAME_BYTES: usize = 512;
pub const MAX_PAYLOAD_BYTES: usize = 496;
pub const FRAME_DELIMITER: u8 = 0x00;
pub const MAGIC: [u8; 2] = *b"BB";

pub const HELLO_ID_MAX_BYTES: usize = 16;

pub const CAP_KEY: u32 = 1 << 0;
pub const CAP_PAGE: u32 = 1 << 1;
pub const CAP_STATUS: u32 = 1 << 2;
pub const CAP_LOG: u32 = 1 << 3;
pub const CAP_CRASH: u32 = 1 << 4;
pub const CAP_DISPLAY_PROBE: u32 = 1 << 5;

pub const ALL_CAPABILITIES: u32 =
    CAP_KEY | CAP_PAGE | CAP_STATUS | CAP_LOG | CAP_CRASH | CAP_DISPLAY_PROBE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    OutputTooSmall,
    BadMagic,
    BadCrc,
    FrameTooLarge,
    BadCobs,
    UnknownOpcode,
    PayloadTooLarge,
    BadStatus,
    BadPayloadLength,
    BadVersion,
    InvalidValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawFrameHeader {
    pub kind: u8,
    pub opcode: u8,
    pub status: u8,
    pub sequence: u16,
    pub payload_len: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    Request = 1,
    Response = 2,
    Event = 3,
}

impl FrameKind {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(FrameKind::Request),
            2 => Some(FrameKind::Response),
            3 => Some(FrameKind::Event),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    Hello = 0x01,
    Key = 0x02,
    Page = 0x03,
    Status = 0x04,
    LogGet = 0x05,
    LogClear = 0x06,
    CrashGet = 0x07,
    CrashClear = 0x08,
    DisplayProbe = 0x09,
}

impl Opcode {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Opcode::Hello),
            0x02 => Some(Opcode::Key),
            0x03 => Some(Opcode::Page),
            0x04 => Some(Opcode::Status),
            0x05 => Some(Opcode::LogGet),
            0x06 => Some(Opcode::LogClear),
            0x07 => Some(Opcode::CrashGet),
            0x08 => Some(Opcode::CrashClear),
            0x09 => Some(Opcode::DisplayProbe),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    Ok = 0,
    Error = 1,
    BadRequest = 2,
    NotFound = 3,
    InternalError = 4,
}

impl Status {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Status::Ok),
            1 => Some(Status::Error),
            2 => Some(Status::BadRequest),
            3 => Some(Status::NotFound),
            4 => Some(Status::InternalError),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub kind: FrameKind,
    pub opcode: Opcode,
    pub status: Status,
    pub sequence: u16,
    pub payload_len: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyCode {
    Left = 0x01,
    Right = 0x02,
    Up = 0x03,
    Down = 0x04,
    Select = 0x05,
    Back = 0x06,
    Power = 0x07,
}

impl KeyCode {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(KeyCode::Left),
            0x02 => Some(KeyCode::Right),
            0x03 => Some(KeyCode::Up),
            0x04 => Some(KeyCode::Down),
            0x05 => Some(KeyCode::Select),
            0x06 => Some(KeyCode::Back),
            0x07 => Some(KeyCode::Power),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyAction {
    Press = 0x01,
    Release = 0x02,
}

impl KeyAction {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(KeyAction::Press),
            0x02 => Some(KeyAction::Release),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PageAction {
    Next = 0x01,
    Previous = 0x02,
    First = 0x03,
    Last = 0x04,
    Goto = 0x05,
    Current = 0x06,
}

impl PageAction {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(PageAction::Next),
            0x02 => Some(PageAction::Previous),
            0x03 => Some(PageAction::First),
            0x04 => Some(PageAction::Last),
            0x05 => Some(PageAction::Goto),
            0x06 => Some(PageAction::Current),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PanelModeCode {
    Unknown = 0x00,
    Grayscale = 0x01,
    Bw = 0x02,
}

impl PanelModeCode {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(PanelModeCode::Unknown),
            0x01 => Some(PanelModeCode::Grayscale),
            0x02 => Some(PanelModeCode::Bw),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ProbeCode {
    FullRefreshCurrent = 0x01,
    ClearWhite = 0x02,
    WindowCorners = 0x03,
}

impl ProbeCode {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(ProbeCode::FullRefreshCurrent),
            0x02 => Some(ProbeCode::ClearWhite),
            0x03 => Some(ProbeCode::WindowCorners),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelloResponse<'a> {
    pub protocol_version: u8,
    pub max_frame_bytes: u16,
    pub capabilities: u32,
    pub firmware_name: &'a str,
    pub target: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelloResponseRef<'a> {
    pub protocol_version: u8,
    pub max_frame_bytes: u16,
    pub capabilities: u32,
    pub firmware_name: &'a [u8],
    pub target: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPayload {
    pub key: KeyCode,
    pub action: KeyAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PagePayload {
    pub action: PageAction,
    pub page_index: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusPayload {
    pub current_page: u32,
    pub page_count: u32,
    pub panel_mode: PanelModeCode,
    pub dropped_log_count: u32,
    pub protocol_error_count: u32,
    pub last_error: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogGetPayload {
    pub cursor_sequence: u32,
    pub max_bytes: u16,
}

pub const LOG_RESPONSE_HEADER_BYTES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogResponseHeader {
    pub next_cursor: u32,
    pub dropped_log_count: u32,
    pub record_count: u16,
}

pub const CRASH_SUMMARY_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogRecordPayload {
    pub sequence: u32,
    pub tick_ms: u32,
    pub level: u8,
    pub subsystem: u8,
    pub event: u16,
    pub arg0: i32,
    pub arg1: i32,
    pub arg2: i32,
}

pub fn encode_hello_response(
    value: &HelloResponse<'_>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let name_bytes = value.firmware_name.as_bytes();
    let target_bytes = value.target.as_bytes();
    if name_bytes.len() > HELLO_ID_MAX_BYTES || target_bytes.len() > HELLO_ID_MAX_BYTES {
        return Err(ProtocolError::InvalidValue);
    }
    let total = 1 + 2 + 4 + 1 + name_bytes.len() + 1 + target_bytes.len();
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    let mut pos = 0;
    out[pos] = value.protocol_version;
    pos += 1;
    out[pos..pos + 2].copy_from_slice(&value.max_frame_bytes.to_le_bytes());
    pos += 2;
    out[pos..pos + 4].copy_from_slice(&value.capabilities.to_le_bytes());
    pos += 4;
    out[pos] = name_bytes.len() as u8;
    pos += 1;
    out[pos..pos + name_bytes.len()].copy_from_slice(name_bytes);
    pos += name_bytes.len();
    out[pos] = target_bytes.len() as u8;
    pos += 1;
    out[pos..pos + target_bytes.len()].copy_from_slice(target_bytes);
    pos += target_bytes.len();
    Ok(pos)
}

pub fn decode_hello_response(payload: &[u8]) -> Result<HelloResponseRef<'_>, ProtocolError> {
    if payload.len() < 8 {
        return Err(ProtocolError::PayloadTooLarge);
    }
    let mut pos = 0;
    let protocol_version = payload[pos];
    pos += 1;
    let max_frame_bytes = u16::from_le_bytes([payload[pos], payload[pos + 1]]);
    pos += 2;
    let capabilities = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let name_len = payload[pos] as usize;
    pos += 1;
    if payload.len() < pos + name_len + 1 {
        return Err(ProtocolError::PayloadTooLarge);
    }
    let firmware_name = &payload[pos..pos + name_len];
    pos += name_len;
    let target_len = payload[pos] as usize;
    pos += 1;
    if payload.len() < pos + target_len {
        return Err(ProtocolError::PayloadTooLarge);
    }
    if payload.len() != pos + target_len
        || name_len > HELLO_ID_MAX_BYTES
        || target_len > HELLO_ID_MAX_BYTES
    {
        return Err(ProtocolError::BadPayloadLength);
    }
    let target = &payload[pos..pos + target_len];
    Ok(HelloResponseRef {
        protocol_version,
        max_frame_bytes,
        capabilities,
        firmware_name,
        target,
    })
}

pub fn encode_key_payload(
    key: KeyCode,
    action: KeyAction,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    if out.len() < 2 {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = key as u8;
    out[1] = action as u8;
    Ok(2)
}

pub fn decode_key_payload(payload: &[u8]) -> Result<KeyPayload, ProtocolError> {
    if payload.len() != 2 {
        return Err(ProtocolError::BadPayloadLength);
    }
    let key = KeyCode::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)?;
    let action = KeyAction::from_u8(payload[1]).ok_or(ProtocolError::InvalidValue)?;
    Ok(KeyPayload { key, action })
}

pub fn encode_page_payload(
    action: PageAction,
    page: Option<u32>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    match action {
        PageAction::Goto => {
            let page_index = page.ok_or(ProtocolError::InvalidValue)?;
            if out.len() < 5 {
                return Err(ProtocolError::OutputTooSmall);
            }
            out[0] = action as u8;
            out[1..5].copy_from_slice(&page_index.to_le_bytes());
            Ok(5)
        }
        _ => {
            if out.is_empty() {
                return Err(ProtocolError::OutputTooSmall);
            }
            out[0] = action as u8;
            Ok(1)
        }
    }
}

pub fn decode_page_payload(payload: &[u8]) -> Result<PagePayload, ProtocolError> {
    if payload.is_empty() {
        return Err(ProtocolError::PayloadTooLarge);
    }
    let action = PageAction::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)?;
    match action {
        PageAction::Goto => {
            if payload.len() != 5 {
                return Err(ProtocolError::BadPayloadLength);
            }
            let page_index = u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]);
            Ok(PagePayload {
                action,
                page_index: Some(page_index),
            })
        }
        _ if payload.len() == 1 => Ok(PagePayload {
            action,
            page_index: None,
        }),
        _ => Err(ProtocolError::BadPayloadLength),
    }
}

pub fn encode_page_response(page: u32, out: &mut [u8]) -> Result<usize, ProtocolError> {
    if out.len() < 4 {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&page.to_le_bytes());
    Ok(4)
}

pub fn encode_status_payload(value: StatusPayload, out: &mut [u8]) -> Result<usize, ProtocolError> {
    let total = 4 + 4 + 1 + 4 + 4 + 4;
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    let mut pos = 0;
    out[pos..pos + 4].copy_from_slice(&value.current_page.to_le_bytes());
    pos += 4;
    out[pos..pos + 4].copy_from_slice(&value.page_count.to_le_bytes());
    pos += 4;
    out[pos] = value.panel_mode as u8;
    pos += 1;
    out[pos..pos + 4].copy_from_slice(&value.dropped_log_count.to_le_bytes());
    pos += 4;
    out[pos..pos + 4].copy_from_slice(&value.protocol_error_count.to_le_bytes());
    pos += 4;
    out[pos..pos + 4].copy_from_slice(&value.last_error.to_le_bytes());
    pos += 4;
    Ok(pos)
}

pub fn decode_status_payload(payload: &[u8]) -> Result<StatusPayload, ProtocolError> {
    let expected = 4 + 4 + 1 + 4 + 4 + 4;
    if payload.len() != expected {
        return Err(ProtocolError::BadPayloadLength);
    }
    let mut pos = 0;
    let current_page = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let page_count = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let panel_mode = PanelModeCode::from_u8(payload[pos]).ok_or(ProtocolError::InvalidValue)?;
    pos += 1;
    let dropped_log_count = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let protocol_error_count = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let last_error = i32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    Ok(StatusPayload {
        current_page,
        page_count,
        panel_mode,
        dropped_log_count,
        protocol_error_count,
        last_error,
    })
}

pub fn encode_log_get_payload(
    value: LogGetPayload,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let total = 4 + 2;
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&value.cursor_sequence.to_le_bytes());
    out[4..6].copy_from_slice(&value.max_bytes.to_le_bytes());
    Ok(total)
}

pub fn decode_log_get_payload(payload: &[u8]) -> Result<LogGetPayload, ProtocolError> {
    let expected = 4 + 2;
    if payload.len() != expected {
        return Err(ProtocolError::BadPayloadLength);
    }
    let cursor_sequence = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let max_bytes = u16::from_le_bytes([payload[4], payload[5]]);
    Ok(LogGetPayload {
        cursor_sequence,
        max_bytes,
    })
}

pub fn encode_log_response_header(
    value: LogResponseHeader,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    if out.len() < LOG_RESPONSE_HEADER_BYTES {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&value.next_cursor.to_le_bytes());
    out[4..8].copy_from_slice(&value.dropped_log_count.to_le_bytes());
    out[8..10].copy_from_slice(&value.record_count.to_le_bytes());
    Ok(LOG_RESPONSE_HEADER_BYTES)
}

pub fn decode_log_response_header(payload: &[u8]) -> Result<LogResponseHeader, ProtocolError> {
    if payload.len() < LOG_RESPONSE_HEADER_BYTES {
        return Err(ProtocolError::BadPayloadLength);
    }
    Ok(LogResponseHeader {
        next_cursor: u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]),
        dropped_log_count: u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]),
        record_count: u16::from_le_bytes([payload[8], payload[9]]),
    })
}

pub fn encode_crash_response(
    summary: Option<&[u8; CRASH_SUMMARY_BYTES]>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let required = if summary.is_some() {
        1 + CRASH_SUMMARY_BYTES
    } else {
        1
    };
    if out.len() < required {
        return Err(ProtocolError::OutputTooSmall);
    }
    match summary {
        Some(summary) => {
            out[0] = 1;
            out[1..required].copy_from_slice(summary);
        }
        None => out[0] = 0,
    }
    Ok(required)
}

pub fn decode_crash_response(payload: &[u8]) -> Result<Option<&[u8]>, ProtocolError> {
    match payload {
        [0] => Ok(None),
        bytes if bytes.len() == 1 + CRASH_SUMMARY_BYTES && bytes[0] == 1 => Ok(Some(&bytes[1..])),
        _ => Err(ProtocolError::BadPayloadLength),
    }
}

pub const LOG_RECORD_BYTES: usize = 24;

pub fn encode_log_record(value: LogRecordPayload, out: &mut [u8]) -> Result<usize, ProtocolError> {
    if out.len() < LOG_RECORD_BYTES {
        return Err(ProtocolError::OutputTooSmall);
    }
    let mut pos = 0;
    out[pos..pos + 4].copy_from_slice(&value.sequence.to_le_bytes());
    pos += 4;
    out[pos..pos + 4].copy_from_slice(&value.tick_ms.to_le_bytes());
    pos += 4;
    out[pos] = value.level;
    pos += 1;
    out[pos] = value.subsystem;
    pos += 1;
    out[pos..pos + 2].copy_from_slice(&value.event.to_le_bytes());
    pos += 2;
    out[pos..pos + 4].copy_from_slice(&value.arg0.to_le_bytes());
    pos += 4;
    out[pos..pos + 4].copy_from_slice(&value.arg1.to_le_bytes());
    pos += 4;
    out[pos..pos + 4].copy_from_slice(&value.arg2.to_le_bytes());
    pos += 4;
    Ok(pos)
}

pub fn decode_log_record(payload: &[u8]) -> Result<LogRecordPayload, ProtocolError> {
    if payload.len() < LOG_RECORD_BYTES {
        return Err(ProtocolError::PayloadTooLarge);
    }
    let mut pos = 0;
    let sequence = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let tick_ms = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let level = payload[pos];
    pos += 1;
    let subsystem = payload[pos];
    pos += 1;
    let event = u16::from_le_bytes([payload[pos], payload[pos + 1]]);
    pos += 2;
    let arg0 = i32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let arg1 = i32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let arg2 = i32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    Ok(LogRecordPayload {
        sequence,
        tick_ms,
        level,
        subsystem,
        event,
        arg0,
        arg1,
        arg2,
    })
}

pub fn encode_probe_payload(probe: ProbeCode, out: &mut [u8]) -> Result<usize, ProtocolError> {
    if out.is_empty() {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = probe as u8;
    Ok(1)
}

pub fn decode_probe_payload(payload: &[u8]) -> Result<ProbeCode, ProtocolError> {
    if payload.len() != 1 {
        return Err(ProtocolError::BadPayloadLength);
    }
    ProbeCode::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DiagLevelCode {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
    Fatal = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DiagSubsystemCode {
    System = 0,
    Display = 1,
    Input = 2,
    Navigation = 3,
    Storage = 4,
    Protocol = 5,
    Idle = 6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagEventCode(pub u16);

pub const EVT_FIRMWARE_STARTED: u16 = 0x0001;
pub const EVT_CMD_RECEIPT: u16 = 0x0010;
pub const EVT_CMD_ERROR: u16 = 0x0011;
pub const EVT_KEY_PRESS: u16 = 0x0100;
pub const EVT_BUTTON_EVENT: u16 = 0x0101;
pub const EVT_INPUT_TRANSITION: u16 = 0x0102;
pub const EVT_INPUT_DECISION: u16 = 0x0103;
pub const EVT_PAGE_DECISION: u16 = 0x0200;
pub const EVT_PAGE_TURN: u16 = 0x0201;
pub const EVT_RENDER_START: u16 = 0x0300;
pub const EVT_RENDER_SUCCESS: u16 = 0x0301;
pub const EVT_RENDER_FAILURE: u16 = 0x0302;
pub const EVT_REFRESH_DECISION: u16 = 0x0400;
pub const EVT_REFRESH_PHASE: u16 = 0x0401;
pub const EVT_PANEL_MODE: u16 = 0x0500;
pub const EVT_ADC_SAMPLE: u16 = 0x0600;
pub const EVT_IDLE_ENTERED: u16 = 0x0700;
pub const EVT_IDLE_SUMMARY: u16 = 0x0701;
pub const EVT_IDLE_LEFT: u16 = 0x0702;
pub const EVT_DISPLAY_ERROR: u16 = 0x0800;
pub const EVT_TURN_QUEUED: u16 = 0x0202;
pub const EVT_TURN_DEQUEUED: u16 = 0x0203;
pub const EVT_TURN_DROPPED: u16 = 0x0204;
pub const EVT_TURN_STARTED: u16 = 0x0205;
pub const EVT_TURN_BOUNDARY_NOOP: u16 = 0x0206;
pub const EVT_RESEED_START: u16 = 0x0303;
pub const EVT_RESEED_COMPLETE: u16 = 0x0304;
pub const EVT_DISPLAY_RECOVERY: u16 = 0x0801;
pub const EVT_GRAY_DELAY_CANCELLED: u16 = 0x0305;
pub const EVT_GRAY_OVERLAY_START: u16 = 0x0306;
pub const EVT_GRAY_OVERLAY_CANCELLED: u16 = 0x0307;
pub const EVT_GRAY_OVERLAY_ACTIVATE: u16 = 0x0308;
pub const EVT_GRAY_OVERLAY_COMPLETE: u16 = 0x0309;
pub const EVT_BW_BASE_SYNC_START: u16 = 0x030A;
pub const EVT_BW_BASE_SYNC_CANCELLED: u16 = 0x030B;
pub const EVT_BW_BASE_SYNC_COMPLETE: u16 = 0x030C;
pub const EVT_CONTROLLER_RAM_STATE: u16 = 0x0402;
pub const EVT_WAVEFORM_SELECTED: u16 = 0x0403;

pub fn crc16_ccitt_false(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

pub fn cobs_encode(input: &[u8], output: &mut [u8]) -> Result<usize, ProtocolError> {
    if output.len() < input.len() + 2 {
        return Err(ProtocolError::OutputTooSmall);
    }

    let mut read_pos = 0;
    let mut write_pos = 1;
    let mut code_pos = 0;
    let mut code = 1u8;

    while read_pos < input.len() {
        if input[read_pos] == 0 {
            output[code_pos] = code;
            code_pos = write_pos;
            write_pos += 1;
            code = 1;
        } else {
            if code == 0xFF {
                output[code_pos] = code;
                code_pos = write_pos;
                write_pos += 1;
                code = 1;
            }
            output[write_pos] = input[read_pos];
            write_pos += 1;
            code += 1;
        }
        read_pos += 1;
    }

    output[code_pos] = code;
    output[write_pos] = FRAME_DELIMITER;
    write_pos += 1;

    Ok(write_pos)
}

pub fn cobs_decode(input: &[u8], output: &mut [u8]) -> Result<usize, ProtocolError> {
    if input.is_empty() || input[input.len() - 1] != FRAME_DELIMITER {
        return Err(ProtocolError::BadCobs);
    }

    let mut read_pos = 0;
    let mut write_pos = 0;

    loop {
        if read_pos >= input.len() - 1 {
            break;
        }

        let code = input[read_pos] as usize;
        read_pos += 1;

        if code == 0 {
            return Err(ProtocolError::BadCobs);
        }

        let end = read_pos + code - 1;
        if end > input.len() - 1 {
            return Err(ProtocolError::BadCobs);
        }

        while read_pos < end {
            if write_pos >= output.len() {
                return Err(ProtocolError::OutputTooSmall);
            }
            output[write_pos] = input[read_pos];
            write_pos += 1;
            read_pos += 1;
        }

        if code < 0xFF && read_pos < input.len() - 1 {
            if write_pos >= output.len() {
                return Err(ProtocolError::OutputTooSmall);
            }
            output[write_pos] = 0;
            write_pos += 1;
        }
    }

    Ok(write_pos)
}

pub fn encode_frame(
    header: &FrameHeader,
    payload: &[u8],
    output: &mut [u8; MAX_FRAME_BYTES],
) -> Result<usize, ProtocolError> {
    encode_raw_frame(
        &RawFrameHeader {
            kind: header.kind as u8,
            opcode: header.opcode as u8,
            status: header.status as u8,
            sequence: header.sequence,
            payload_len: header.payload_len,
        },
        payload,
        output,
    )
}

pub fn encode_raw_frame(
    header: &RawFrameHeader,
    payload: &[u8],
    output: &mut [u8; MAX_FRAME_BYTES],
) -> Result<usize, ProtocolError> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(ProtocolError::PayloadTooLarge);
    }
    if header.payload_len as usize != payload.len() {
        return Err(ProtocolError::BadPayloadLength);
    }

    let mut raw = [0u8; MAX_FRAME_BYTES];
    raw[0..2].copy_from_slice(&MAGIC);
    raw[2] = PROTOCOL_VERSION;
    raw[3] = header.kind;
    raw[4] = header.opcode;
    raw[5] = header.status;
    raw[6..8].copy_from_slice(&header.sequence.to_le_bytes());
    raw[8..10].copy_from_slice(&header.payload_len.to_le_bytes());
    raw[10..10 + payload.len()].copy_from_slice(payload);
    let pos = 10 + payload.len();

    let crc = crc16_ccitt_false(&raw[..pos]);
    raw[pos..pos + 2].copy_from_slice(&crc.to_le_bytes());
    let total_len = pos + 2;

    cobs_encode(&raw[..total_len], output)
}

pub fn decode_raw_frame(
    input: &[u8],
    payload_out: &mut [u8],
) -> Result<(RawFrameHeader, usize), ProtocolError> {
    if input.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }

    let mut raw = [0u8; MAX_FRAME_BYTES];
    let raw_len = cobs_decode(input, &mut raw)?;

    if raw_len < 12 {
        return Err(ProtocolError::BadMagic);
    }

    if raw[0..2] != MAGIC {
        return Err(ProtocolError::BadMagic);
    }

    if raw[2] != PROTOCOL_VERSION {
        return Err(ProtocolError::BadVersion);
    }

    let sequence = u16::from_le_bytes([raw[6], raw[7]]);
    let payload_len = u16::from_le_bytes([raw[8], raw[9]]) as usize;

    let header_len = 10;
    let total_len = header_len + payload_len + 2;

    if raw_len != total_len {
        return Err(ProtocolError::BadPayloadLength);
    }

    let expected_crc = u16::from_le_bytes([raw[raw_len - 2], raw[raw_len - 1]]);
    let actual_crc = crc16_ccitt_false(&raw[..raw_len - 2]);
    if expected_crc != actual_crc {
        return Err(ProtocolError::BadCrc);
    }

    if payload_out.len() < payload_len {
        return Err(ProtocolError::OutputTooSmall);
    }
    payload_out[..payload_len].copy_from_slice(&raw[header_len..header_len + payload_len]);

    Ok((
        RawFrameHeader {
            kind: raw[3],
            opcode: raw[4],
            status: raw[5],
            sequence,
            payload_len: payload_len as u16,
        },
        payload_len,
    ))
}

pub fn decode_frame(
    input: &[u8],
    payload_out: &mut [u8],
) -> Result<(FrameHeader, usize), ProtocolError> {
    let (raw, payload_len) = decode_raw_frame(input, payload_out)?;
    Ok((
        FrameHeader {
            kind: FrameKind::from_u8(raw.kind).ok_or(ProtocolError::BadMagic)?,
            opcode: Opcode::from_u8(raw.opcode).ok_or(ProtocolError::UnknownOpcode)?,
            status: Status::from_u8(raw.status).ok_or(ProtocolError::BadStatus)?,
            sequence: raw.sequence,
            payload_len: raw.payload_len,
        },
        payload_len,
    ))
}
