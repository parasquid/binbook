//! Hardware abstraction traits for Xteink devices.
//!
//! This crate defines the trait interfaces that concrete HAL implementations
//! must provide. Library crates depend on these traits, never on concrete
//! implementations.

#![no_std]
#![allow(async_fn_in_trait)]

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

/// Result type alias for HAL operations.
pub type HalResult<T> = Result<T, HalError>;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshMode {
    /// Full display refresh.
    Full,
    /// Partial display refresh.
    Partial,
    /// Four-level grayscale display refresh.
    Grayscale,
}

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
