pub const PROTOCOL_VERSION: u8 = 2;
pub const MAX_FRAME_BYTES: usize = 4126;
pub const MAX_PAYLOAD_BYTES: usize = 4096;
pub const FRAME_DELIMITER: u8 = 0x00;
pub const MAGIC: [u8; 2] = *b"BB";

pub const HELLO_ID_MAX_BYTES: usize = 16;

pub const CAP_KEY: u32 = 1 << 0;
pub const CAP_PAGE: u32 = 1 << 1;
pub const CAP_STATUS: u32 = 1 << 2;
pub const CAP_LOG: u32 = 1 << 3;
pub const CAP_CRASH: u32 = 1 << 4;
pub const CAP_DISPLAY_PROBE: u32 = 1 << 5;
pub const CAP_STORAGE: u32 = 1 << 6;
pub const CAP_REFRESH_MODE: u32 = 1 << 7;
pub const CAP_STORAGE_UPLOAD_WINDOW_2: u32 = 1 << 8;
pub const CAP_STORAGE_READ_WINDOW_2: u32 = 1 << 9;
pub const CAP_STORAGE_TRANSFER_LIMITS: u32 = 1 << 10;
pub const CAP_STORAGE_TRANSFER_METRICS: u32 = 1 << 11;
pub const CAP_STORAGE_BULK_V1: u32 = 1 << 12;

pub const ALL_CAPABILITIES: u32 = CAP_KEY
    | CAP_PAGE
    | CAP_STATUS
    | CAP_LOG
    | CAP_CRASH
    | CAP_DISPLAY_PROBE
    | CAP_STORAGE
    | CAP_REFRESH_MODE
    | CAP_STORAGE_UPLOAD_WINDOW_2
    | CAP_STORAGE_READ_WINDOW_2
    | CAP_STORAGE_TRANSFER_LIMITS
    | CAP_STORAGE_TRANSFER_METRICS;

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
    StoreList = 0x0A,
    StoreUploadBegin = 0x0B,
    StoreUploadWrite = 0x0C,
    StoreUploadCommit = 0x0D,
    StoreAbort = 0x0E,
    StoreDelete = 0x0F,
    StoreRead = 0x10,
    RefreshMode = 0x11,
    StoreTransferLimits = 0x12,
    StoreTransferTrace = 0x13,
    StoreBulkOpen = 0x14,
    StoreBulkData = 0x15,
    StoreBulkCredit = 0x16,
    StoreBulkStatus = 0x17,
    StoreBulkFinish = 0x18,
    StoreBulkAbort = 0x19,
    StoreSdWriteTrace = 0x1A,
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
            0x0A => Some(Opcode::StoreList),
            0x0B => Some(Opcode::StoreUploadBegin),
            0x0C => Some(Opcode::StoreUploadWrite),
            0x0D => Some(Opcode::StoreUploadCommit),
            0x0E => Some(Opcode::StoreAbort),
            0x0F => Some(Opcode::StoreDelete),
            0x10 => Some(Opcode::StoreRead),
            0x11 => Some(Opcode::RefreshMode),
            0x12 => Some(Opcode::StoreTransferLimits),
            0x13 => Some(Opcode::StoreTransferTrace),
            0x14 => Some(Opcode::StoreBulkOpen),
            0x15 => Some(Opcode::StoreBulkData),
            0x16 => Some(Opcode::StoreBulkCredit),
            0x17 => Some(Opcode::StoreBulkStatus),
            0x18 => Some(Opcode::StoreBulkFinish),
            0x19 => Some(Opcode::StoreBulkAbort),
            0x1A => Some(Opcode::StoreSdWriteTrace),
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
    Unsupported = 5,
}

impl Status {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Status::Ok),
            1 => Some(Status::Error),
            2 => Some(Status::BadRequest),
            3 => Some(Status::NotFound),
            4 => Some(Status::InternalError),
            5 => Some(Status::Unsupported),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StorageBackend {
    Sd = 0,
    Flash = 1,
}

impl StorageBackend {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(StorageBackend::Sd),
            1 => Some(StorageBackend::Flash),
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
    PartialWindowBusyMatrix = 0x04,
    CustomLutFastA = 0x05,
    CustomLutFastB = 0x06,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RefreshModeAction {
    Get = 0x01,
    Set = 0x02,
}

impl RefreshModeAction {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(RefreshModeAction::Get),
            0x02 => Some(RefreshModeAction::Set),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RefreshModeCode {
    Balanced = 0x00,
    Fast = 0x01,
}

impl RefreshModeCode {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(RefreshModeCode::Balanced),
            0x01 => Some(RefreshModeCode::Fast),
            _ => None,
        }
    }
}

impl ProbeCode {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(ProbeCode::FullRefreshCurrent),
            0x02 => Some(ProbeCode::ClearWhite),
            0x03 => Some(ProbeCode::WindowCorners),
            0x04 => Some(ProbeCode::PartialWindowBusyMatrix),
            0x05 => Some(ProbeCode::CustomLutFastA),
            0x06 => Some(ProbeCode::CustomLutFastB),
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
pub struct RefreshModePayload {
    pub action: RefreshModeAction,
    pub mode: RefreshModeCode,
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
