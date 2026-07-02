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
pub const EVT_REQUEST_ENQUEUE: u16 = 0x0207;
pub const EVT_REQUEST_RECEIVE: u16 = 0x0208;
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
pub const EVT_DISPLAY_REQUEST_START: u16 = 0x030D;
pub const EVT_DISPLAY_REQUEST_END: u16 = 0x030E;
pub const EVT_PAGE_METADATA_READ: u16 = 0x030F;
pub const EVT_PLANE_WRITE_START: u16 = 0x0310;
pub const EVT_PLANE_ROW_FILL_SUMMARY: u16 = 0x0311;
pub const EVT_PLANE_SPI_WRITE_SUMMARY: u16 = 0x0312;
pub const EVT_PLANE_WRITE_END: u16 = 0x0313;
pub const EVT_CONTROLLER_RAM_STATE: u16 = 0x0402;
pub const EVT_WAVEFORM_SELECTED: u16 = 0x0403;
pub const EVT_BUSY_WAIT_START: u16 = 0x0404;
pub const EVT_BUSY_WAIT_END: u16 = 0x0405;
pub const EVT_REFRESH_TRIGGER: u16 = 0x0406;
