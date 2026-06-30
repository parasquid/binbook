mod common;

use common::{data_after, driver, Delay};
use ssd1677_driver::{Command, ControllerState, Waveform};

#[test]
fn reset_and_bw_initialization_match_panel_configuration() {
    let (mut driver, trace, _) = driver(4);
    let mut delay = Delay::default();

    driver.init_with_delay(&mut delay).unwrap();

    assert_eq!(delay.0, [20_000_000, 20_000_000, 200_000_000]);
    let writes = trace.0.borrow();
    assert_eq!(
        data_after(&writes, Command::BOOSTER_SOFT_START),
        [0xae, 0xc7, 0xc3, 0xc0, 0x80]
    );
    assert_eq!(data_after(&writes, Command::BORDER_WAVEFORM), [0x01]);
    assert_eq!(driver.state(), ControllerState::Powered);
}

#[test]
fn grayscale_initialization_and_waveform_are_explicit_operations() {
    let (mut driver, trace, _) = driver(4);
    let mut delay = Delay::default();
    let waveform = Waveform {
        lut: &[0x11, 0x22],
        gate_voltage: 0x17,
        source_voltage: [0x41, 0xa8, 0x32],
        vcom_voltage: 0x30,
    };

    driver.init_grayscale_with_delay(&mut delay).unwrap();
    driver.load_waveform(&waveform).unwrap();

    let writes = trace.0.borrow();
    assert_eq!(data_after(&writes, Command::BORDER_WAVEFORM), [0x00]);
    assert_eq!(data_after(&writes, Command::WRITE_LUT), [0x11, 0x22]);
    assert_eq!(
        data_after(&writes, Command::SOURCE_VOLTAGE),
        [0x41, 0xa8, 0x32]
    );
}

#[test]
fn grayscale_initialization_preserves_controller_command_order() {
    let (mut driver, trace, _) = driver(4);
    let mut delay = Delay::default();

    driver.init_grayscale_with_delay(&mut delay).unwrap();

    let expected = [
        vec![Command::BOOSTER_SOFT_START],
        vec![0xae, 0xc7, 0xc3, 0xc0, 0x80],
        vec![Command::DRIVER_OUTPUT_CTRL],
        vec![0xdf, 0x01, 0x02],
        vec![Command::DATA_ENTRY_MODE],
        vec![0x03],
        vec![Command::BORDER_WAVEFORM],
        vec![0x00],
        vec![Command::TEMP_SENSOR_CTRL],
        vec![0x80],
    ];
    assert!(trace
        .0
        .borrow()
        .windows(expected.len())
        .any(|window| window == expected));
}
