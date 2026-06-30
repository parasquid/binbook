use core::convert::Infallible;
use embedded_hal::{
    digital::{ErrorType as DigitalErrorType, OutputPin},
    spi::{ErrorType as SpiErrorType, Operation, SpiBus, SpiDevice},
};
use std::{cell::RefCell, rc::Rc};

use binbook_fw::board::BoardSpiDevice;

#[derive(Clone, Default)]
struct Trace(Rc<RefCell<Vec<&'static str>>>);

struct Bus(Trace);
impl SpiErrorType for Bus {
    type Error = Infallible;
}
impl SpiBus<u8> for Bus {
    fn read(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.0 .0.borrow_mut().push("read");
        words.fill(0x5a);
        Ok(())
    }
    fn write(&mut self, _: &[u8]) -> Result<(), Self::Error> {
        self.0 .0.borrow_mut().push("write");
        Ok(())
    }
    fn transfer(&mut self, read: &mut [u8], _: &[u8]) -> Result<(), Self::Error> {
        self.0 .0.borrow_mut().push("transfer");
        read.fill(0xa5);
        Ok(())
    }
    fn transfer_in_place(&mut self, _: &mut [u8]) -> Result<(), Self::Error> {
        self.0 .0.borrow_mut().push("transfer-in-place");
        Ok(())
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        self.0 .0.borrow_mut().push("flush");
        Ok(())
    }
}

struct ChipSelect(Trace);
impl DigitalErrorType for ChipSelect {
    type Error = Infallible;
}
impl OutputPin for ChipSelect {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.0 .0.borrow_mut().push("select");
        Ok(())
    }
    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.0 .0.borrow_mut().push("deselect");
        Ok(())
    }
}

#[test]
fn board_spi_adapter_owns_chip_select_for_the_whole_transaction() {
    let trace = Trace::default();
    let mut device = BoardSpiDevice::new(Bus(trace.clone()), ChipSelect(trace.clone()));
    let mut read = [0_u8; 2];
    device
        .transaction(&mut [Operation::Write(&[1, 2]), Operation::Read(&mut read)])
        .unwrap();
    assert_eq!(
        &*trace.0.borrow(),
        &["select", "write", "read", "flush", "deselect"]
    );
    assert_eq!(read, [0x5a; 2]);
}
