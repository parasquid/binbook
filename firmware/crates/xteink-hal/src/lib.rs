//! Hardware abstraction traits for Xteink devices.
//!
//! This crate defines the trait interfaces that concrete HAL implementations
//! must provide. Library crates depend on these traits, never on concrete
//! implementations.

#![no_std]
#![allow(async_fn_in_trait)]

use embedded_hal::digital::ErrorType as DigitalErrorType;
use embedded_hal::spi::{ErrorType as SpiErrorType, Operation, SpiDevice};

/// Error type for HAL operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HalError {
    /// SPI communication error.
    Spi,
    /// GPIO error.
    Gpio,
    /// Flash storage error.
    Flash,
    /// Timeout waiting for a peripheral.
    Timeout,
    /// Invalid parameter.
    InvalidParam,
}

impl embedded_hal::digital::Error for HalError {
    fn kind(&self) -> embedded_hal::digital::ErrorKind {
        embedded_hal::digital::ErrorKind::Other
    }
}

impl embedded_hal::spi::Error for HalError {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        embedded_hal::spi::ErrorKind::Other
    }
}

/// Result type alias for HAL operations.
pub type HalResult<T> = Result<T, HalError>;

impl From<ssd1677_driver::Error> for HalError {
    fn from(error: ssd1677_driver::Error) -> Self {
        match error {
            ssd1677_driver::Error::Spi => Self::Spi,
            ssd1677_driver::Error::Gpio => Self::Gpio,
            ssd1677_driver::Error::Timeout => Self::Timeout,
            ssd1677_driver::Error::InvalidParameter => Self::InvalidParam,
        }
    }
}

/// SPI bus trait.
pub trait Spi {
    /// Write a command byte followed by data bytes.
    fn write_command(&mut self, cmd: u8, data: &[u8]) -> HalResult<()>;

    /// Write raw bytes.
    fn write(&mut self, data: &[u8]) -> HalResult<()>;

    /// Read bytes.
    fn read(&mut self, buf: &mut [u8]) -> HalResult<()>;
}

/// Digital output pin trait.
pub trait OutputPin {
    /// Set pin high.
    fn set_high(&mut self) -> HalResult<()>;

    /// Set pin low.
    fn set_low(&mut self) -> HalResult<()>;
}

/// Digital input pin trait.
pub trait InputPin {
    /// Read pin state.
    fn is_high(&self) -> HalResult<bool>;

    /// Read pin state as low-active.
    fn is_low(&self) -> HalResult<bool> {
        self.is_high().map(|high| !high)
    }
}

/// ADC pin trait for button reading.
pub trait AdcPin {
    /// Read ADC value (0-4095 for a 12-bit ADC).
    fn read(&self) -> HalResult<u16>;
}

/// Flash storage trait.
pub trait Flash {
    /// Read bytes from flash at the given offset.
    fn read(&self, offset: u32, buf: &mut [u8]) -> HalResult<()>;

    /// Write bytes to flash at the given offset.
    fn write(&mut self, offset: u32, data: &[u8]) -> HalResult<()>;

    /// Erase a sector at the given offset.
    fn erase_sector(&mut self, offset: u32) -> HalResult<()>;

    /// Get total flash size in bytes.
    fn size(&self) -> u32;
}

/// Display refresh mode.
pub use ssd1677_driver::RefreshMode;

/// Display trait for e-ink screens.
pub trait Display {
    /// Initialize display with its init sequence.
    fn init(&mut self) -> HalResult<()>;

    /// Set the RAM address window.
    fn set_window(&mut self, x: u16, y: u16, width: u16, height: u16) -> HalResult<()>;

    /// Write a single GRAY1 row.
    fn write_row(&mut self, row: u16, data: &[u8]) -> HalResult<()>;

    /// Trigger display refresh.
    fn refresh(&mut self, mode: RefreshMode) -> HalResult<()>;

    /// Clear display to white.
    fn clear(&mut self) -> HalResult<()>;

    /// Wait for display ready.
    fn wait_ready(&mut self) -> HalResult<()>;
}

/// Delay trait.
pub trait Delay {
    /// Wait for the specified number of milliseconds.
    fn ms(&self, ms: u32);
}

/// Async delay trait for executor-driven firmware tasks.
pub trait AsyncDelay {
    /// Wait for the specified number of milliseconds.
    async fn ms(&self, ms: u32);
}

pub struct SpiDeviceAdapter<SPI, CS> {
    spi: SPI,
    cs: CS,
}

impl<SPI, CS> SpiDeviceAdapter<SPI, CS> {
    pub fn new(spi: SPI, cs: CS) -> Self {
        Self { spi, cs }
    }
}

impl<SPI, CS> SpiErrorType for SpiDeviceAdapter<SPI, CS>
where
    SPI: Spi,
    CS: OutputPin,
{
    type Error = HalError;
}

impl<SPI, CS> SpiDevice<u8> for SpiDeviceAdapter<SPI, CS>
where
    SPI: Spi,
    CS: OutputPin,
{
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        self.cs.set_low()?;
        let result = operations
            .iter_mut()
            .try_for_each(|operation| match operation {
                Operation::Read(buffer) => self.spi.read(buffer),
                Operation::Write(buffer) => self.spi.write(buffer),
                Operation::Transfer(read, write) => {
                    self.spi.write(write)?;
                    self.spi.read(read)
                }
                Operation::TransferInPlace(buffer) => {
                    self.spi.write(buffer)?;
                    self.spi.read(buffer)
                }
                Operation::DelayNs(_) => Ok(()),
            });
        let deselect = self.cs.set_high();
        result.and(deselect)
    }
}

pub struct OutputPinAdapter<P>(pub P);

impl<P: OutputPin> DigitalErrorType for OutputPinAdapter<P> {
    type Error = HalError;
}

impl<P: OutputPin> embedded_hal::digital::OutputPin for OutputPinAdapter<P> {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.0.set_low()
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.0.set_high()
    }
}

pub struct InputPinAdapter<P>(pub P);

impl<P: InputPin> DigitalErrorType for InputPinAdapter<P> {
    type Error = HalError;
}

impl<P: InputPin> embedded_hal::digital::InputPin for InputPinAdapter<P> {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        self.0.is_high()
    }

    fn is_low(&mut self) -> Result<bool, Self::Error> {
        self.0.is_low()
    }
}

pub struct DelayAdapter<'a>(pub &'a dyn Delay);

impl embedded_hal::delay::DelayNs for DelayAdapter<'_> {
    fn delay_ns(&mut self, ns: u32) {
        self.0.ms(ns.div_ceil(1_000_000));
    }
}

pub struct AsyncDelayAdapter<'a, D: AsyncDelay + ?Sized>(pub &'a D);

impl<D: AsyncDelay + ?Sized> embedded_hal_async::delay::DelayNs for AsyncDelayAdapter<'_, D> {
    async fn delay_ns(&mut self, ns: u32) {
        self.0.ms(ns.div_ceil(1_000_000)).await;
    }
}

/// Button state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Up,
    Down,
    Back,
    Select,
    Power,
}

/// Input handler trait.
pub trait Input {
    /// Poll for button press.
    fn poll(&mut self) -> Option<Button>;
}
