pub struct Command;

impl Command {
    pub const SW_RESET: u8 = 0x12;
    pub const DRIVER_OUTPUT_CTRL: u8 = 0x01;
    pub const BOOSTER_SOFT_START: u8 = 0x0c;
    pub const DATA_ENTRY_MODE: u8 = 0x11;
    pub const SET_RAM_X_ADDR: u8 = 0x44;
    pub const SET_RAM_Y_ADDR: u8 = 0x45;
    pub const SET_RAM_X_COUNTER: u8 = 0x4e;
    pub const SET_RAM_Y_COUNTER: u8 = 0x4f;
    pub const DISPLAY_UPDATE_CTRL2: u8 = 0x22;
    pub const MASTER_ACTIVATION: u8 = 0x20;
    pub const BORDER_WAVEFORM: u8 = 0x3c;
    pub const TEMP_SENSOR_CTRL: u8 = 0x18;
    pub const GATE_VOLTAGE: u8 = 0x03;
    pub const SOURCE_VOLTAGE: u8 = 0x04;
    pub const VCOM_VOLTAGE: u8 = 0x2c;
    pub const DISPLAY_UPDATE_CTRL1: u8 = 0x21;
    pub const WRITE_RAM_BW: u8 = 0x24;
    pub const WRITE_RAM: u8 = Self::WRITE_RAM_BW;
    pub const WRITE_RAM_RED: u8 = 0x26;
    pub const WRITE_LUT: u8 = 0x32;
    pub const UPDATE_CTRL_NORMAL: u8 = 0xf7;
    pub const UPDATE_CTRL_FAST: u8 = 0xfc;
    pub const UPDATE_CTRL_GRAYSCALE: u8 = 0xc7;
    pub const UPDATE_CTRL_STAGED_GRAYSCALE: u8 = 0x0c;
}
