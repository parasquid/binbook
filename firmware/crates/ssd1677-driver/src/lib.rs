//! SSD1677/GDEQ0426T82 e-ink display driver.
//!
//! This crate implements the command layer for the SSD1677 controller,
//! providing black/white and grayscale refresh paths for the Xteink X4's
//! 800x480 panel.

#![no_std]

use xteink_hal::{Delay, HalError, HalResult, InputPin, OutputPin, RefreshMode, Spi};

const BUSY_TIMEOUT_MS: u32 = 60_000;
const SSD1677_LUT_4G: [u8; 112] = [
    0x80, 0x48, 0x4a, 0x22, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0a, 0x48,
    0x68, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x88, 0x48, 0x60, 0x08,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xa8, 0x48, 0x45, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x07, 0x1e, 0x1c, 0x02, 0x00, 0x05, 0x01, 0x05, 0x01, 0x02,
    0x08, 0x01, 0x01, 0x04, 0x04, 0x00, 0x02, 0x01, 0x02, 0x02, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x01, 0x22, 0x22, 0x22, 0x22, 0x22, 0x17, 0x41, 0xa8,
    0x32, 0x30, 0x00, 0x00,
];

/// SSD1677 command bytes.
pub struct Ssd1677;

impl Ssd1677 {
    pub const SW_RESET: u8 = 0x12;
    pub const DEEP_SLEEP: u8 = 0x10;
    pub const DRIVER_OUTPUT_CTRL: u8 = 0x01;
    pub const BOOSTER_SOFT_START: u8 = 0x0C;
    pub const DATA_ENTRY_MODE: u8 = 0x11;
    pub const SET_RAM_X_ADDR: u8 = 0x44;
    pub const SET_RAM_Y_ADDR: u8 = 0x45;
    pub const SET_RAM_X_COUNTER: u8 = 0x4E;
    pub const SET_RAM_Y_COUNTER: u8 = 0x4F;
    pub const WRITE_RAM: u8 = 0x24;
    pub const DISPLAY_UPDATE_CTRL2: u8 = 0x22;
    pub const MASTER_ACTIVATION: u8 = 0x20;
    pub const BORDER_WAVEFORM: u8 = 0x3C;
    pub const TEMP_SENSOR_CTRL: u8 = 0x18;
    pub const GATE_VOLTAGE: u8 = 0x03;
    pub const SOURCE_VOLTAGE: u8 = 0x04;
    pub const VCOM_VOLTAGE: u8 = 0x2C;
    pub const DISPLAY_UPDATE_CTRL1: u8 = 0x21;
    pub const WRITE_RAM_BW: u8 = 0x24;
    pub const WRITE_RAM_RED: u8 = 0x26;
    pub const WRITE_LUT: u8 = 0x32;
    pub const UPDATE_CTRL_NORMAL: u8 = 0xF7;
    pub const UPDATE_CTRL_FAST: u8 = 0xFC;
    pub const UPDATE_CTRL_GRAYSCALE: u8 = 0xC7;
    pub const DATA_ENTRY_X_INC_Y_INC_HORIZONTAL: u8 = 0x03;
}

/// SSD1677 display driver.
pub struct Ssd1677Driver<SPI, CS, DC, RST, BUSY> {
    spi: SPI,
    cs: CS,
    dc: DC,
    rst: RST,
    busy: BUSY,
}

impl<SPI, CS, DC, RST, BUSY> Ssd1677Driver<SPI, CS, DC, RST, BUSY>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    /// Create a new SSD1677 driver instance.
    pub fn new(spi: SPI, cs: CS, dc: DC, rst: RST, busy: BUSY) -> Self {
        Self {
            spi,
            cs,
            dc,
            rst,
            busy,
        }
    }

    fn send_cmd(&mut self, cmd: u8) -> HalResult<()> {
        self.dc.set_low()?;
        self.cs.set_low()?;
        self.spi.write(&[cmd])?;
        self.cs.set_high()?;
        Ok(())
    }

    fn send_data(&mut self, data: &[u8]) -> HalResult<()> {
        self.dc.set_high()?;
        self.cs.set_low()?;
        self.spi.write(data)?;
        self.cs.set_high()?;
        Ok(())
    }

    fn send_cmd_data(&mut self, cmd: u8, data: &[u8]) -> HalResult<()> {
        self.send_cmd(cmd)?;
        self.send_data(data)
    }

    /// Initialize display with the Xteink X4 panel sequence.
    pub fn init_with_delay(&mut self, delay: &dyn Delay) -> HalResult<()> {
        self.rst.set_high()?;
        delay.ms(20);
        self.rst.set_low()?;
        delay.ms(20);
        self.rst.set_high()?;
        delay.ms(200);

        self.wait_ready_with_delay(delay)?;

        self.send_cmd(Ssd1677::SW_RESET)?;
        self.wait_ready_with_delay(delay)?;

        self.send_cmd_data(Ssd1677::TEMP_SENSOR_CTRL, &[0x80])?;
        self.send_cmd_data(Ssd1677::BOOSTER_SOFT_START, &[0xAE, 0xC7, 0xC3, 0xC0, 0x80])?;
        self.send_cmd_data(Ssd1677::DRIVER_OUTPUT_CTRL, &[0xDF, 0x01, 0x02])?;
        self.send_cmd_data(
            Ssd1677::DATA_ENTRY_MODE,
            &[Ssd1677::DATA_ENTRY_X_INC_Y_INC_HORIZONTAL],
        )?;
        self.send_cmd_data(Ssd1677::BORDER_WAVEFORM, &[0x01])?;
        self.send_cmd_data(Ssd1677::SET_RAM_X_ADDR, &[0x00, 0x00, 0x1F, 0x03])?;
        self.send_cmd_data(Ssd1677::SET_RAM_Y_ADDR, &[0x00, 0x00, 0xDF, 0x01])?;
        self.send_cmd_data(Ssd1677::SET_RAM_X_COUNTER, &[0x00, 0x00])?;
        self.send_cmd_data(Ssd1677::SET_RAM_Y_COUNTER, &[0x00, 0x00])?;

        Ok(())
    }

    /// Initialize display with the Xteink X4 four-level grayscale sequence.
    pub fn init_grayscale_with_delay(&mut self, delay: &dyn Delay) -> HalResult<()> {
        self.rst.set_high()?;
        delay.ms(20);
        self.rst.set_low()?;
        delay.ms(20);
        self.rst.set_high()?;
        delay.ms(200);

        self.wait_ready_with_delay(delay)?;

        self.send_cmd(Ssd1677::SW_RESET)?;
        self.wait_ready_with_delay(delay)?;

        self.send_cmd_data(Ssd1677::BOOSTER_SOFT_START, &[0xAE, 0xC7, 0xC3, 0xC0, 0x80])?;
        self.send_cmd_data(Ssd1677::DRIVER_OUTPUT_CTRL, &[0xDF, 0x01, 0x02])?;
        self.send_cmd_data(
            Ssd1677::DATA_ENTRY_MODE,
            &[Ssd1677::DATA_ENTRY_X_INC_Y_INC_HORIZONTAL],
        )?;
        self.send_cmd_data(Ssd1677::BORDER_WAVEFORM, &[0x00])?;
        self.send_cmd_data(Ssd1677::TEMP_SENSOR_CTRL, &[0x80])?;
        self.send_cmd_data(Ssd1677::SET_RAM_X_ADDR, &[0x00, 0x00, 0x1F, 0x03])?;
        self.send_cmd_data(Ssd1677::SET_RAM_Y_ADDR, &[0x00, 0x00, 0xDF, 0x01])?;
        self.send_cmd_data(Ssd1677::SET_RAM_X_COUNTER, &[0x00, 0x00])?;
        self.send_cmd_data(Ssd1677::SET_RAM_Y_COUNTER, &[0x00, 0x00])?;
        self.send_cmd_data(Ssd1677::WRITE_LUT, &SSD1677_LUT_4G[..105])?;
        self.send_cmd_data(Ssd1677::GATE_VOLTAGE, &SSD1677_LUT_4G[105..106])?;
        self.send_cmd_data(Ssd1677::SOURCE_VOLTAGE, &SSD1677_LUT_4G[106..109])?;
        self.send_cmd_data(Ssd1677::VCOM_VOLTAGE, &SSD1677_LUT_4G[109..110])?;

        Ok(())
    }

    /// Wait for display ready with a bounded polling loop.
    pub fn wait_ready_with_delay(&mut self, delay: &dyn Delay) -> HalResult<()> {
        let mut timeout_ms = BUSY_TIMEOUT_MS;
        while self.busy.is_high()? {
            delay.ms(1);
            timeout_ms -= 1;
            if timeout_ms == 0 {
                return Err(HalError::Timeout);
            }
        }
        Ok(())
    }

    /// Set the RAM address window in SSD1677 physical pixel coordinates.
    pub fn set_window(&mut self, x: u16, y: u16, width: u16, height: u16) -> HalResult<()> {
        if width == 0 || height == 0 {
            return Err(HalError::InvalidParam);
        }

        let x_end = x + width - 1;
        let y_end = y + height - 1;

        self.send_cmd_data(
            Ssd1677::SET_RAM_X_ADDR,
            &[
                (x & 0xFF) as u8,
                (x >> 8) as u8,
                (x_end & 0xFF) as u8,
                (x_end >> 8) as u8,
            ],
        )?;
        self.send_cmd_data(
            Ssd1677::SET_RAM_Y_ADDR,
            &[
                (y & 0xFF) as u8,
                (y >> 8) as u8,
                (y_end & 0xFF) as u8,
                (y_end >> 8) as u8,
            ],
        )?;
        self.send_cmd_data(
            Ssd1677::SET_RAM_X_COUNTER,
            &[(x & 0xFF) as u8, (x >> 8) as u8],
        )?;
        self.send_cmd_data(
            Ssd1677::SET_RAM_Y_COUNTER,
            &[(y & 0xFF) as u8, (y >> 8) as u8],
        )?;

        Ok(())
    }

    /// Write a single GRAY1 row.
    pub fn write_row(&mut self, row: u16, data: &[u8]) -> HalResult<()> {
        self.write_row_to_ram(Ssd1677::WRITE_RAM, row, data)
    }

    /// Write a single GRAY1 row to the secondary/red RAM plane.
    pub fn write_red_row(&mut self, row: u16, data: &[u8]) -> HalResult<()> {
        self.write_row_to_ram(Ssd1677::WRITE_RAM_RED, row, data)
    }

    fn write_row_to_ram(&mut self, command: u8, row: u16, data: &[u8]) -> HalResult<()> {
        self.send_cmd_data(
            Ssd1677::SET_RAM_Y_COUNTER,
            &[(row & 0xFF) as u8, (row >> 8) as u8],
        )?;
        self.send_cmd_data(Ssd1677::SET_RAM_X_COUNTER, &[0x00, 0x00])?;
        self.send_cmd_data(command, data)
    }

    /// Stream contiguous rows to black RAM after setting the current window/counters.
    pub fn write_frame_rows<const ROW_BYTES: usize>(
        &mut self,
        row_count: u16,
        fill_row: impl FnMut(u16, &mut [u8; ROW_BYTES]),
    ) -> HalResult<()> {
        self.write_frame_rows_to_ram(Ssd1677::WRITE_RAM, row_count, fill_row)
    }

    /// Stream contiguous rows to secondary/red RAM after setting the current window/counters.
    pub fn write_red_frame_rows<const ROW_BYTES: usize>(
        &mut self,
        row_count: u16,
        fill_row: impl FnMut(u16, &mut [u8; ROW_BYTES]),
    ) -> HalResult<()> {
        self.write_frame_rows_to_ram(Ssd1677::WRITE_RAM_RED, row_count, fill_row)
    }

    fn write_frame_rows_to_ram<const ROW_BYTES: usize>(
        &mut self,
        command: u8,
        row_count: u16,
        mut fill_row: impl FnMut(u16, &mut [u8; ROW_BYTES]),
    ) -> HalResult<()> {
        let mut row = [0xFF; ROW_BYTES];

        self.send_cmd(command)?;
        self.dc.set_high()?;
        self.cs.set_low()?;

        for y in 0..row_count {
            fill_row(y, &mut row);
            self.spi.write(&row)?;
        }

        self.cs.set_high()
    }

    /// Fill a pixel-aligned window in black RAM with one byte value.
    pub fn write_solid_window(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        value: u8,
    ) -> HalResult<()> {
        self.write_solid_window_to_ram(Ssd1677::WRITE_RAM, x, y, width, height, value)
    }

    /// Fill a pixel-aligned window in secondary/red RAM with one byte value.
    pub fn write_red_solid_window(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        value: u8,
    ) -> HalResult<()> {
        self.write_solid_window_to_ram(Ssd1677::WRITE_RAM_RED, x, y, width, height, value)
    }

    fn write_solid_window_to_ram(
        &mut self,
        command: u8,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        value: u8,
    ) -> HalResult<()> {
        if width == 0 || height == 0 {
            return Err(HalError::InvalidParam);
        }

        self.set_window(x, y, width, height)?;
        self.send_cmd(command)?;
        self.dc.set_high()?;
        self.cs.set_low()?;

        let row_byte_count = width.div_ceil(8) as usize;
        let row = [value; 100];
        for _ in 0..height {
            self.spi.write(&row[..row_byte_count])?;
        }

        self.cs.set_high()
    }

    /// Trigger a display refresh.
    pub fn refresh_with_delay(&mut self, mode: RefreshMode, delay: &dyn Delay) -> HalResult<()> {
        let ctrl = match mode {
            RefreshMode::Full => Ssd1677::UPDATE_CTRL_NORMAL,
            RefreshMode::Partial => Ssd1677::UPDATE_CTRL_FAST,
            RefreshMode::Grayscale => Ssd1677::UPDATE_CTRL_GRAYSCALE,
        };
        if matches!(mode, RefreshMode::Partial | RefreshMode::Grayscale) {
            self.send_cmd_data(Ssd1677::DISPLAY_UPDATE_CTRL1, &[0x00, 0x00])?;
        }
        self.send_cmd_data(Ssd1677::DISPLAY_UPDATE_CTRL2, &[ctrl])?;
        self.send_cmd(Ssd1677::MASTER_ACTIVATION)?;
        self.wait_ready_with_delay(delay)
    }

    /// Clear display RAM to white, then perform a full refresh.
    pub fn clear_with_delay(&mut self, delay: &dyn Delay) -> HalResult<()> {
        self.set_window(0, 0, 800, 480)?;
        self.write_red_frame_rows::<100>(480, |_row, row_buf| row_buf.fill(0xFF))?;
        self.set_window(0, 0, 800, 480)?;
        self.write_frame_rows::<100>(480, |_row, row_buf| row_buf.fill(0xFF))?;
        self.refresh_with_delay(RefreshMode::Full, delay)
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::format;
    use std::rc::Rc;
    use std::string::String;
    use std::vec::Vec;

    #[derive(Debug, Default)]
    struct MockSpi {
        writes: Vec<Vec<u8>>,
    }

    impl Spi for MockSpi {
        fn write_command(&mut self, cmd: u8, data: &[u8]) -> HalResult<()> {
            self.writes.push([&[cmd], data].concat());
            Ok(())
        }

        fn write(&mut self, data: &[u8]) -> HalResult<()> {
            self.writes.push(data.to_vec());
            Ok(())
        }

        fn read(&mut self, buf: &mut [u8]) -> HalResult<()> {
            buf.fill(0);
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct MockOutputPin;

    impl OutputPin for MockOutputPin {
        fn set_high(&mut self) -> HalResult<()> {
            Ok(())
        }

        fn set_low(&mut self) -> HalResult<()> {
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct MockBusyPin;

    impl InputPin for MockBusyPin {
        fn is_high(&self) -> HalResult<bool> {
            Ok(false)
        }
    }

    #[derive(Debug, Default)]
    struct MockDelay;

    impl Delay for MockDelay {
        fn ms(&self, _ms: u32) {}
    }

    fn driver() -> Ssd1677Driver<MockSpi, MockOutputPin, MockOutputPin, MockOutputPin, MockBusyPin>
    {
        Ssd1677Driver::new(
            MockSpi::default(),
            MockOutputPin,
            MockOutputPin,
            MockOutputPin,
            MockBusyPin,
        )
    }

    fn data_after_nth(writes: &[Vec<u8>], command: u8, nth: usize) -> &[u8] {
        let command_pos = writes
            .iter()
            .enumerate()
            .filter_map(|(index, write)| (write.as_slice() == [command]).then_some(index))
            .nth(nth)
            .expect("command not written");
        writes
            .get(command_pos + 1)
            .expect("command has no following data")
            .as_slice()
    }

    fn data_after(writes: &[Vec<u8>], command: u8) -> &[u8] {
        data_after_nth(writes, command, 0)
    }

    #[derive(Clone, Debug)]
    struct RecordingOutputPin {
        name: &'static str,
        events: Rc<RefCell<Vec<String>>>,
    }

    impl RecordingOutputPin {
        fn new(name: &'static str, events: Rc<RefCell<Vec<String>>>) -> Self {
            Self { name, events }
        }
    }

    impl OutputPin for RecordingOutputPin {
        fn set_high(&mut self) -> HalResult<()> {
            self.events.borrow_mut().push(format!("{}=high", self.name));
            Ok(())
        }

        fn set_low(&mut self) -> HalResult<()> {
            self.events.borrow_mut().push(format!("{}=low", self.name));
            Ok(())
        }
    }

    #[derive(Debug)]
    struct RecordingDelay {
        events: Rc<RefCell<Vec<String>>>,
    }

    impl Delay for RecordingDelay {
        fn ms(&self, ms: u32) {
            self.events.borrow_mut().push(format!("delay={ms}"));
        }
    }

    #[test]
    fn init_sets_full_xteink_x4_ram_window() {
        let mut driver = driver();

        driver.init_with_delay(&MockDelay).unwrap();

        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::SET_RAM_X_ADDR),
            &[0x00, 0x00, 0x1F, 0x03],
            "Xteink X4 SSD1677 path uses 16-bit physical pixel X range 0..799",
        );
        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::SET_RAM_Y_ADDR),
            &[0x00, 0x00, 0xDF, 0x01],
            "480 physical rows require little-endian window 0..479",
        );
        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::DATA_ENTRY_MODE),
            &[Ssd1677::DATA_ENTRY_X_INC_Y_INC_HORIZONTAL],
            "match SquidScript's horizontal X-increment/Y-increment write mode",
        );
        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::SET_RAM_X_COUNTER),
            &[0x00, 0x00],
            "SSD1677 X counter is sent as 16-bit little-endian physical pixel coordinate",
        );
    }

    #[test]
    fn init_matches_squidscript_bw_command_sequence() {
        let mut driver = driver();

        driver.init_with_delay(&MockDelay).unwrap();

        let expected: &[Vec<u8>] = &[
            Vec::from([Ssd1677::TEMP_SENSOR_CTRL]),
            Vec::from([0x80]),
            Vec::from([0x0C]),
            Vec::from([0xAE, 0xC7, 0xC3, 0xC0, 0x80]),
            Vec::from([Ssd1677::DRIVER_OUTPUT_CTRL]),
            Vec::from([0xDF, 0x01, 0x02]),
            Vec::from([Ssd1677::DATA_ENTRY_MODE]),
            Vec::from([Ssd1677::DATA_ENTRY_X_INC_Y_INC_HORIZONTAL]),
            Vec::from([Ssd1677::BORDER_WAVEFORM]),
            Vec::from([0x01]),
            Vec::from([Ssd1677::SET_RAM_X_ADDR]),
            Vec::from([0x00, 0x00, 0x1F, 0x03]),
        ];

        assert!(driver
            .spi
            .writes
            .windows(expected.len())
            .any(|window| { window == expected }));
    }

    #[test]
    fn init_uses_squidscript_hardware_reset_timing() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let delay = RecordingDelay {
            events: Rc::clone(&events),
        };
        let rst = RecordingOutputPin::new("rst", Rc::clone(&events));
        let mut driver = Ssd1677Driver::new(
            MockSpi::default(),
            MockOutputPin,
            MockOutputPin,
            rst,
            MockBusyPin,
        );

        driver.init_with_delay(&delay).unwrap();

        assert_eq!(
            events
                .borrow()
                .iter()
                .take(6)
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "rst=high",
                "delay=20",
                "rst=low",
                "delay=20",
                "rst=high",
                "delay=200",
            ],
        );
    }

    #[test]
    fn set_window_sends_little_endian_physical_pixel_ranges() {
        let mut driver = driver();

        driver.set_window(0, 0x0020, 800, 240).unwrap();

        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::SET_RAM_X_ADDR),
            &[0x00, 0x00, 0x1F, 0x03],
        );
        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::SET_RAM_Y_ADDR),
            &[0x20, 0x00, 0x0F, 0x01],
        );
    }

    #[test]
    fn write_row_sends_xy_counters_little_endian_before_row_data() {
        let mut driver = driver();

        driver.write_row(0x0123, &[0xAA, 0x55]).unwrap();

        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::SET_RAM_Y_COUNTER),
            &[0x23, 0x01],
            "SSD1677 Y counter is sent low byte then high byte",
        );
        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::SET_RAM_X_COUNTER),
            &[0x00, 0x00],
            "SSD1677 X counter is sent as 16-bit little-endian physical pixel coordinate",
        );
        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::WRITE_RAM),
            &[0xAA, 0x55],
        );
    }

    #[test]
    fn write_red_row_targets_secondary_ram_plane() {
        let mut driver = driver();

        driver.write_red_row(0x002A, &[0xFF, 0x00]).unwrap();

        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::SET_RAM_Y_COUNTER),
            &[0x2A, 0x00],
        );
        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::SET_RAM_X_COUNTER),
            &[0x00, 0x00],
        );
        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::WRITE_RAM_RED),
            &[0xFF, 0x00],
        );
    }

    #[test]
    fn write_frame_rows_streams_after_single_write_ram_command() {
        let mut driver = driver();

        driver
            .write_frame_rows::<2>(3, |row, row_buf| {
                row_buf[0] = row as u8;
                row_buf[1] = 0xA0 + row as u8;
            })
            .unwrap();

        let write_ram_commands = driver
            .spi
            .writes
            .iter()
            .filter(|write| write.as_slice() == [Ssd1677::WRITE_RAM])
            .count();
        assert_eq!(write_ram_commands, 1);

        assert_eq!(driver.spi.writes[1], [0x00, 0xA0]);
        assert_eq!(driver.spi.writes[2], [0x01, 0xA1]);
        assert_eq!(driver.spi.writes[3], [0x02, 0xA2]);
    }

    #[test]
    fn write_solid_window_sets_window_and_streams_window_rows_once() {
        let mut driver = driver();

        driver.write_solid_window(672, 384, 128, 96, 0x00).unwrap();

        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::SET_RAM_X_ADDR),
            &[0xA0, 0x02, 0x1F, 0x03],
        );
        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::SET_RAM_Y_ADDR),
            &[0x80, 0x01, 0xDF, 0x01],
        );

        let write_ram_commands = driver
            .spi
            .writes
            .iter()
            .filter(|write| write.as_slice() == [Ssd1677::WRITE_RAM])
            .count();
        assert_eq!(write_ram_commands, 1);

        let command_pos = driver
            .spi
            .writes
            .iter()
            .position(|write| write.as_slice() == [Ssd1677::WRITE_RAM])
            .unwrap();
        assert_eq!(driver.spi.writes[command_pos + 1], [0x00; 16]);
        assert_eq!(driver.spi.writes[command_pos + 96], [0x00; 16]);
    }

    #[test]
    fn full_refresh_matches_squidscript_update_sequence() {
        let mut driver = driver();

        driver
            .refresh_with_delay(RefreshMode::Full, &MockDelay)
            .unwrap();

        assert_eq!(
            driver.spi.writes,
            [
                Vec::from([Ssd1677::DISPLAY_UPDATE_CTRL2]),
                Vec::from([0xF7]),
                Vec::from([Ssd1677::MASTER_ACTIVATION])
            ],
        );
    }

    #[test]
    fn partial_refresh_matches_squidscript_update_sequence() {
        let mut driver = driver();

        driver
            .refresh_with_delay(RefreshMode::Partial, &MockDelay)
            .unwrap();

        assert_eq!(
            driver.spi.writes,
            [
                Vec::from([Ssd1677::DISPLAY_UPDATE_CTRL1]),
                Vec::from([0x00, 0x00]),
                Vec::from([Ssd1677::DISPLAY_UPDATE_CTRL2]),
                Vec::from([0xFC]),
                Vec::from([Ssd1677::MASTER_ACTIVATION]),
            ],
        );
    }

    #[test]
    fn grayscale_init_writes_lut_and_voltage_commands() {
        let mut driver = driver();

        driver.init_grayscale_with_delay(&MockDelay).unwrap();

        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::BORDER_WAVEFORM),
            &[0x00],
        );
        assert_eq!(data_after(&driver.spi.writes, Ssd1677::WRITE_LUT).len(), 105);
        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::GATE_VOLTAGE).len(),
            1
        );
        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::SOURCE_VOLTAGE).len(),
            3
        );
        assert_eq!(
            data_after(&driver.spi.writes, Ssd1677::VCOM_VOLTAGE).len(),
            1
        );
    }

    #[test]
    fn grayscale_refresh_matches_squidscript_update_sequence() {
        let mut driver = driver();

        driver
            .refresh_with_delay(RefreshMode::Grayscale, &MockDelay)
            .unwrap();

        assert_eq!(
            driver.spi.writes,
            [
                Vec::from([Ssd1677::DISPLAY_UPDATE_CTRL1]),
                Vec::from([0x00, 0x00]),
                Vec::from([Ssd1677::DISPLAY_UPDATE_CTRL2]),
                Vec::from([0xC7]),
                Vec::from([Ssd1677::MASTER_ACTIVATION]),
            ],
        );
    }
}
