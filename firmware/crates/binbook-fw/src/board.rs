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
            })
            .and_then(|()| self.bus.flush().map_err(|_| FirmwareError::Spi));
        let deselect = self.chip_select.set_high().map_err(|_| FirmwareError::Gpio);
        result.and(deselect)
    }
}

#[cfg(all(feature = "firmware-bin", target_arch = "riscv32"))]
pub struct DisplayDelay;

#[cfg(all(feature = "firmware-bin", target_arch = "riscv32"))]
impl embedded_hal_async::delay::DelayNs for DisplayDelay {
    async fn delay_ns(&mut self, ns: u32) {
        embassy_time::Timer::after_nanos(u64::from(ns)).await;
    }
}

// Blocking delay — needed by the SD card init sequence (embedded-sdmmc
// uses the blocking DelayNs trait for its init handshake).
#[cfg(all(feature = "firmware-bin", target_arch = "riscv32"))]
impl embedded_hal::delay::DelayNs for DisplayDelay {
    fn delay_ns(&mut self, ns: u32) {
        embassy_time::block_for(embassy_time::Duration::from_nanos(ns as u64));
    }
}

// ---------------------------------------------------------------------------
// Shared SPI2 bus — used by both the display and (optionally) the SD card.
// The bus is owned by a `SharedSpi2` in a `static_cell::StaticCell`. Each
// peripheral wraps it in a `FreqManagedSpiDevice` that reconfigures the bus
// frequency before each transaction, implementing strategy R1 (Task 0).
// ---------------------------------------------------------------------------

#[cfg(all(feature = "firmware-bin", target_arch = "riscv32"))]
pub use shared_spi::{FreqManagedSpiDevice, SharedSpi2};

#[cfg(all(feature = "firmware-bin", target_arch = "riscv32"))]
mod shared_spi {
    use core::cell::RefCell;

    use embedded_hal::{
        digital::OutputPin,
        spi::{ErrorType, Operation, SpiBus, SpiDevice},
    };
    use esp_hal::spi::master::{Config as SpiConfig, Spi};
    use esp_hal::spi::Mode;
    use esp_hal::time::Rate;
    use esp_hal::Blocking;

    use crate::error::FirmwareError;

    /// Shared SPI2 bus, wrapped in a `RefCell` so that both the display and SD
    /// tasks can share the same hardware peripheral without moving it.
    pub struct SharedSpi2 {
        bus: RefCell<Spi<'static, Blocking>>,
    }

    impl SharedSpi2 {
        /// Construct the shared bus. Consumes the SPI2 peripheral + SCK, MOSI,
        /// and MISO pins. The bus starts at 20 MHz (display-friendly); each
        /// `FreqManagedSpiDevice` reconfigures it on acquire.
        pub fn new(
            spi2: esp_hal::peripherals::SPI2<'static>,
            gpio8: esp_hal::peripherals::GPIO8<'static>,
            gpio10: esp_hal::peripherals::GPIO10<'static>,
            gpio7: esp_hal::peripherals::GPIO7<'static>,
        ) -> Self {
            let spi = Spi::new(
                spi2,
                SpiConfig::default()
                    .with_frequency(Rate::from_mhz(20))
                    .with_mode(Mode::_0),
            )
            .expect("SPI2 init")
            .with_sck(gpio8)
            .with_mosi(gpio10)
            .with_miso(gpio7);
            Self {
                bus: RefCell::new(spi),
            }
        }
    }

    /// An `SpiDevice<u8>` that shares a [`SharedSpi2`] bus with other devices.
    ///
    /// On each [`transaction`](SpiDevice::transaction) the bus frequency is
    /// reconfigured to `freq_hz` before the SPI operations run, implementing
    /// strategy R1 (runtime frequency switch). This lets the display (20 MHz)
    /// and SD card (400 kHz init) share one bus without a fixed compromise.
    pub struct FreqManagedSpiDevice<'a, CS: OutputPin> {
        shared: &'a SharedSpi2,
        cs: CS,
        freq_hz: u32,
    }

    impl<'a, CS: OutputPin> FreqManagedSpiDevice<'a, CS> {
        pub fn new(shared: &'a SharedSpi2, cs: CS, freq_hz: u32) -> Self {
            Self { shared, cs, freq_hz }
        }
    }

    impl<'a, CS: OutputPin> ErrorType for FreqManagedSpiDevice<'a, CS> {
        type Error = FirmwareError;
    }

    impl<'a, CS: OutputPin> SpiDevice<u8> for FreqManagedSpiDevice<'a, CS> {
        fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
            let mut bus = self.shared.bus.borrow_mut();
            bus.apply_config(
                &SpiConfig::default()
                    .with_frequency(Rate::from_hz(self.freq_hz))
                    .with_mode(Mode::_0),
            )
            .map_err(|_| FirmwareError::Spi)?;
            self.cs.set_low().map_err(|_| FirmwareError::Gpio)?;
            let result = operations
                .iter_mut()
                .try_for_each(|operation| match operation {
                    Operation::Read(buffer) => SpiBus::read(&mut *bus, buffer).map_err(|_| FirmwareError::Spi),
                    Operation::Write(buffer) => SpiBus::write(&mut *bus, buffer).map_err(|_| FirmwareError::Spi),
                    Operation::Transfer(read, write) => {
                        SpiBus::transfer(&mut *bus, read, write).map_err(|_| FirmwareError::Spi)
                    }
                    Operation::TransferInPlace(buffer) => {
                        SpiBus::transfer_in_place(&mut *bus, buffer).map_err(|_| FirmwareError::Spi)
                    }
                    Operation::DelayNs(_) => Ok(()),
                })
                .and_then(|()| SpiBus::flush(&mut *bus).map_err(|_| FirmwareError::Spi));
            let _ = self.cs.set_high();
            result
        }
    }
}
