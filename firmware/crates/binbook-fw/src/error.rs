#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareError {
    Spi,
    Gpio,
    Storage,
    Timeout,
    InvalidParameter,
}

pub type FirmwareResult<T> = Result<T, FirmwareError>;

impl embedded_hal::spi::Error for FirmwareError {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        embedded_hal::spi::ErrorKind::Other
    }
}

impl embedded_hal::digital::Error for FirmwareError {
    fn kind(&self) -> embedded_hal::digital::ErrorKind {
        embedded_hal::digital::ErrorKind::Other
    }
}

impl embedded_storage::nor_flash::NorFlashError for FirmwareError {
    fn kind(&self) -> embedded_storage::nor_flash::NorFlashErrorKind {
        embedded_storage::nor_flash::NorFlashErrorKind::Other
    }
}
