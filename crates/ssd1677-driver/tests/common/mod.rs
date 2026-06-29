#![allow(dead_code)]

use core::convert::Infallible;
use embedded_hal::digital::{ErrorType as DigitalErrorType, InputPin, OutputPin};
use embedded_hal::spi::{ErrorType as SpiErrorType, Operation, SpiDevice};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ssd1677_driver::{PanelConfig, Ssd1677};

#[derive(Clone, Default)]
pub struct Trace(pub Rc<RefCell<Vec<Vec<u8>>>>);

#[derive(Default)]
pub struct MockSpi(pub Trace);

impl SpiErrorType for MockSpi {
    type Error = Infallible;
}

impl SpiDevice<u8> for MockSpi {
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        for operation in operations {
            match operation {
                Operation::Write(bytes) => self.0 .0.borrow_mut().push(bytes.to_vec()),
                Operation::Read(bytes) | Operation::Transfer(bytes, _) => bytes.fill(0),
                Operation::TransferInPlace(bytes) => bytes.fill(0),
                Operation::DelayNs(_) => {}
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct Pin;

impl DigitalErrorType for Pin {
    type Error = Infallible;
}

impl OutputPin for Pin {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct Busy(pub Rc<Cell<bool>>);

impl DigitalErrorType for Busy {
    type Error = Infallible;
}

impl InputPin for Busy {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok(self.0.get())
    }

    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok(!self.0.get())
    }
}

#[derive(Default)]
pub struct Delay(pub Vec<u32>);

impl embedded_hal::delay::DelayNs for Delay {
    fn delay_ns(&mut self, ns: u32) {
        self.0.push(ns);
    }
}

impl embedded_hal_async::delay::DelayNs for Delay {
    async fn delay_ns(&mut self, ns: u32) {
        self.0.push(ns);
    }
}

pub fn config(timeout: u32) -> PanelConfig {
    PanelConfig {
        width: 800,
        height: 480,
        driver_output: [0xdf, 0x01, 0x02],
        booster_soft_start: [0xae, 0xc7, 0xc3, 0xc0, 0x80],
        temperature_sensor: 0x80,
        bw_border_waveform: 0x01,
        grayscale_border_waveform: 0x00,
        busy_timeout_ms: timeout,
    }
}

pub fn driver(timeout: u32) -> (Ssd1677<MockSpi, Pin, Pin, Busy>, Trace, Busy) {
    let trace = Trace::default();
    let busy = Busy::default();
    (
        Ssd1677::new(
            MockSpi(trace.clone()),
            Pin,
            Pin,
            busy.clone(),
            config(timeout),
        ),
        trace,
        busy,
    )
}

pub fn data_after(writes: &[Vec<u8>], command: u8) -> &[u8] {
    let index = writes
        .iter()
        .position(|write| write.as_slice() == [command])
        .expect("command must be written");
    writes[index + 1].as_slice()
}
