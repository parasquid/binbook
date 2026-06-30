use embedded_hal::{
    digital::OutputPin,
    spi::{ErrorType, Operation, SpiBus, SpiDevice},
};

use crate::error::FirmwareError;

pub struct BoardSpiDevice<BUS, CS> {
    bus: BUS,
    chip_select: CS,
}

impl<BUS, CS> BoardSpiDevice<BUS, CS> {
    pub fn new(bus: BUS, chip_select: CS) -> Self {
        Self { bus, chip_select }
    }
}

impl<BUS, CS> ErrorType for BoardSpiDevice<BUS, CS>
where
    BUS: SpiBus<u8>,
    CS: OutputPin,
{
    type Error = FirmwareError;
}

impl<BUS, CS> SpiDevice<u8> for BoardSpiDevice<BUS, CS>
where
    BUS: SpiBus<u8>,
    CS: OutputPin,
{
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        self.chip_select
            .set_low()
            .map_err(|_| FirmwareError::Gpio)?;
        let result = operations
            .iter_mut()
            .try_for_each(|operation| match operation {
                Operation::Read(buffer) => self.bus.read(buffer).map_err(|_| FirmwareError::Spi),
                Operation::Write(buffer) => self.bus.write(buffer).map_err(|_| FirmwareError::Spi),
                Operation::Transfer(read, write) => self
                    .bus
                    .transfer(read, write)
                    .map_err(|_| FirmwareError::Spi),
                Operation::TransferInPlace(buffer) => self
                    .bus
                    .transfer_in_place(buffer)
                    .map_err(|_| FirmwareError::Spi),
                Operation::DelayNs(_) => Ok(()),
            });
        let deselect = self.chip_select.set_high().map_err(|_| FirmwareError::Gpio);
        result.and(deselect)
    }
}

#[cfg(feature = "firmware-bin")]
pub struct DisplayDelay;

#[cfg(feature = "firmware-bin")]
impl embedded_hal_async::delay::DelayNs for DisplayDelay {
    async fn delay_ns(&mut self, ns: u32) {
        embassy_time::Timer::after_nanos(u64::from(ns)).await;
    }
}
