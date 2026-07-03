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

use binbook_storage::filesystem::{Filesystem, WritableFilesystem};
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

impl<D> From<embedded_sd_storage::sd_filesystem::StorageError<D>> for SdError
where
    D: embedded_sdmmc::BlockDevice,
    D::Error: core::fmt::Debug,
{
    fn from(e: embedded_sd_storage::sd_filesystem::StorageError<D>) -> Self {
        match e {
            embedded_sd_storage::sd_filesystem::StorageError::Device(_)
            | embedded_sd_storage::sd_filesystem::StorageError::Fat(_)
            | embedded_sd_storage::sd_filesystem::StorageError::NameTooLong
            | embedded_sd_storage::sd_filesystem::StorageError::UploadInProgress
            | embedded_sd_storage::sd_filesystem::StorageError::NoUploadInProgress
            | embedded_sd_storage::sd_filesystem::StorageError::UploadNameMismatch
            | embedded_sd_storage::sd_filesystem::StorageError::UploadTooFragmented
            | embedded_sd_storage::sd_filesystem::StorageError::FileTooFragmented
            | embedded_sd_storage::sd_filesystem::StorageError::BadUploadOffset => SdError::Sdmmc,
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
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] for_each_entry begin");
        let result = self.inner.for_each_entry(visit).map_err(SdError::from);
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] for_each_entry end: {:?}", result);
        result
    }

    fn read_at(&mut self, name: &str, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        #[cfg(feature = "debug-log")]
        esp_println::println!(
            "[SD] read_at begin name={} offset={} len={}",
            name,
            offset,
            out.len()
        );
        let result = self.inner.read_at(name, offset, out).map_err(SdError::from);
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] read_at end: {:?}", result);
        result
    }

    fn file_size(&mut self, name: &str) -> Result<u64, Self::Error> {
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] file_size begin name={}", name);
        let result = self.inner.file_size(name).map_err(SdError::from);
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] file_size end: {:?}", result);
        result
    }

    fn for_each_binbook_entry(
        &mut self,
        visit: &mut dyn FnMut(&str, u64, u32),
    ) -> Result<(), Self::Error> {
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] for_each_binbook_entry begin");
        let result = self.inner.for_each_entry(&mut |name, size| {
            if name.ends_with(".binbook") {
                visit(name, size, 0);
            }
        });
        let result = result.map_err(SdError::from);
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] for_each_binbook_entry end: {:?}", result);
        result
    }
}

impl<SPI, DELAY, TIME> WritableFilesystem for SdFilesystem<SPI, DELAY, TIME>
where
    SPI: SpiDevice<u8>,
    DELAY: DelayNs,
    TIME: TimeSource,
{
    fn create_or_truncate(&mut self, name: &str) -> Result<(), Self::Error> {
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] create_or_truncate begin name={}", name);
        let result = self.inner.create_or_truncate(name).map_err(SdError::from);
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] create_or_truncate end: {:?}", result);
        result
    }

    fn write_at(&mut self, name: &str, offset: u64, data: &[u8]) -> Result<(), Self::Error> {
        #[cfg(feature = "debug-log")]
        esp_println::println!(
            "[SD] write_at begin name={} offset={} len={}",
            name,
            offset,
            data.len()
        );
        let result = self
            .inner
            .write_at(name, offset, data)
            .map_err(SdError::from);
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] write_at end: {:?}", result);
        result
    }

    fn flush(&mut self, name: &str) -> Result<(), Self::Error> {
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] flush begin name={}", name);
        let result = self.inner.flush(name).map_err(SdError::from);
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] flush end: {:?}", result);
        result
    }

    fn delete_file(&mut self, name: &str) -> Result<(), Self::Error> {
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] delete_file begin name={}", name);
        let result = self.inner.delete_file(name).map_err(SdError::from);
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] delete_file end: {:?}", result);
        result
    }

    fn begin_upload(&mut self, name: &str, expected_size: u64) -> Result<(), Self::Error> {
        #[cfg(feature = "debug-log")]
        esp_println::println!(
            "[SD] begin_upload begin name={} expected_size={}",
            name,
            expected_size
        );
        let result = self
            .inner
            .begin_upload(name, expected_size)
            .map_err(SdError::from);
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] begin_upload end: {:?}", result);
        result
    }

    fn write_upload_chunk(
        &mut self,
        name: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        #[cfg(feature = "debug-log")]
        esp_println::println!(
            "[SD] write_upload_chunk begin name={} offset={} len={}",
            name,
            offset,
            data.len()
        );
        let result = self
            .inner
            .write_upload_chunk(name, offset, data)
            .map_err(SdError::from);
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] write_upload_chunk end: {:?}", result);
        result
    }

    fn commit_upload(&mut self, name: &str) -> Result<(), Self::Error> {
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] commit_upload begin name={}", name);
        let result = self.inner.commit_upload(name).map_err(SdError::from);
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] commit_upload end: {:?}", result);
        result
    }

    fn abort_upload(&mut self, name: &str) -> Result<(), Self::Error> {
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] abort_upload begin name={}", name);
        let result = self.inner.abort_upload(name).map_err(SdError::from);
        #[cfg(feature = "debug-log")]
        esp_println::println!("[SD] abort_upload end: {:?}", result);
        result
    }
}
