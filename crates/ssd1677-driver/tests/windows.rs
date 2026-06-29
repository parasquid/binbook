mod common;

use common::{data_after, driver};
use ssd1677_driver::{Command, Error};

#[test]
fn windows_and_counters_are_little_endian_physical_pixels() {
    let (mut driver, trace, _) = driver(4);
    driver.set_window(0, 0x20, 800, 240).unwrap();
    driver.write_row(0x0123, &[0xaa, 0x55]).unwrap();

    let writes = trace.0.borrow();
    assert_eq!(
        data_after(&writes, Command::SET_RAM_X_ADDR),
        [0x00, 0x00, 0x1f, 0x03]
    );
    assert_eq!(
        data_after(&writes, Command::SET_RAM_Y_ADDR),
        [0x20, 0x00, 0x0f, 0x01]
    );
    let last_y = writes
        .iter()
        .rposition(|write| write.as_slice() == [Command::SET_RAM_Y_COUNTER])
        .unwrap();
    assert_eq!(writes[last_y + 1], [0x23, 0x01]);
    assert_eq!(
        data_after(&writes[last_y..], Command::WRITE_RAM_BW),
        [0xaa, 0x55]
    );
}

#[test]
fn invalid_or_overflowing_windows_are_rejected() {
    let (mut driver, _, _) = driver(4);
    assert_eq!(driver.set_window(0, 0, 0, 1), Err(Error::InvalidParameter));
    assert_eq!(
        driver.set_window(u16::MAX, 0, 2, 1),
        Err(Error::InvalidParameter)
    );
}
