#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelConfig {
    pub width: u16,
    pub height: u16,
    pub driver_output: [u8; 3],
    pub booster_soft_start: [u8; 5],
    pub temperature_sensor: u8,
    pub bw_border_waveform: u8,
    pub grayscale_border_waveform: u8,
    pub busy_timeout_ms: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Waveform<'a> {
    pub lut: &'a [u8],
    pub gate_voltage: u8,
    pub source_voltage: [u8; 3],
    pub vcom_voltage: u8,
}
