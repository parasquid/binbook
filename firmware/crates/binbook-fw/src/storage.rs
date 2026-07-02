//! SD card `Filesystem` adapter.
//!
//! Wraps `embedded_sd_storage::SdStorage` as a
//! `binbook_storage::filesystem::Filesystem` so BinBook enumeration and
//! ReadAt work over the hardware SD card.
//!
//! Gated behind `#[cfg(feature = "sd-storage")]`.

use embedded_hal::delay::DelayNs;
use embedded_hal::spi::SpiDevice;
use embedded_sdmmc::filesystem::TimeSource;
use embedded_sdmmc::SdCard;

use binbook_storage::filesystem::Filesystem;
use embedded_sd_storage::SdStorage;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors from SD filesystem operations.
#[derive(Debug)]
pub enum SdError {
    /// The SD card or FAT filesystem returned an error.
    Sdmmc,
    /// The named file was not found.
    NotFound,
}

impl<D: embedded_sdmmc::BlockDevice> From<embedded_sd_storage::sd_filesystem::StorageError<D>>
    for SdError
{
    fn from(e: embedded_sd_storage::sd_filesystem::StorageError<D>) -> Self {
        match e {
            embedded_sd_storage::sd_filesystem::StorageError::Sdmmc(_) => SdError::Sdmmc,
            embedded_sd_storage::sd_filesystem::StorageError::NotFound => SdError::NotFound,
        }
    }
}

// ---------------------------------------------------------------------------
// Fixed timestamp — firmware does not track wall-clock time
// ---------------------------------------------------------------------------

/// Time source that always returns a fixed timestamp.
///
/// FAT filesystems need timestamps for directory entries; this provides a
/// compile-time constant so embedded-sdmmc operations don't panic.
pub struct FixedTime;

impl TimeSource for FixedTime {
    fn get_timestamp(&self) -> embedded_sdmmc::Timestamp {
        // 2026-07-01 12:00:00 — arbitrary but valid.
        // FAT date/time fields: see FAT32 specification.
        let year_off = (2026u16 - 1980) & 0x7f;
        let date = (year_off << 9) | (7 << 5) | 1; // year=46, month=7, day=1
        let time = (12 << 11) | (0 << 5) | (0); // hour=12, min=0, sec=0
        embedded_sdmmc::Timestamp::from_fat(date, time)
    }
}

// ---------------------------------------------------------------------------
// SdFilesystem — Filesystem impl over SdStorage
// ---------------------------------------------------------------------------

/// Wraps an `SdStorage<SdCard<SPI, DELAY>, TIME>` as a `binbook_storage::Filesystem`.
pub struct SdFilesystem<SPI, DELAY, TIME>
where
    SPI: SpiDevice<u8>,
    DELAY: DelayNs,
    TIME: TimeSource,
{
    inner: SdStorage<SdCard<SPI, DELAY>, TIME>,
}

impl<SPI, DELAY, TIME> SdFilesystem<SPI, DELAY, TIME>
where
    SPI: SpiDevice<u8>,
    DELAY: DelayNs,
    TIME: TimeSource,
{
    /// Construct from an SPI device, a delay implementation, and a time source.
    pub fn new(spi: SPI, delay: DELAY, time: TIME) -> Self {
        Self {
            inner: SdStorage::new(spi, delay, time),
        }
    }
}

impl<SPI, DELAY, TIME> Filesystem for SdFilesystem<SPI, DELAY, TIME>
where
    SPI: SpiDevice<u8>,
    DELAY: DelayNs,
    TIME: TimeSource,
{
    type Error = SdError;

    fn for_each_entry(&mut self, visit: &mut dyn FnMut(&str, u64)) -> Result<(), Self::Error> {
        self.inner.for_each_entry(visit).map_err(SdError::from)
    }

    fn read_at(&mut self, name: &str, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        self.inner.read_at(name, offset, out).map_err(SdError::from)
    }

    fn file_size(&mut self, name: &str) -> Result<u64, Self::Error> {
        self.inner.file_size(name).map_err(SdError::from)
    }
}
