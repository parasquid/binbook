use embedded_storage::nor_flash::NorFlash;

use crate::error::{FirmwareError, FirmwareResult};

use crate::diag_log::{CrashSummary, CrashSummaryError, CRASH_RECORD_BYTES};
use crate::flash::CRASH_SECTOR_OFFSET;

pub struct CrashStore<F> {
    flash: F,
}

impl<F: NorFlash> CrashStore<F> {
    pub fn new(flash: F) -> Self {
        Self { flash }
    }

    /// Read and validate the crash summary from flash.
    ///
    /// Returns `Ok(None)` when the sector is erased (all `0xFF`), `Ok(Some(summary))` for a
    /// valid summary, and `Err(InternalError)` for a corrupt sector or flash failure.
    pub fn read(&mut self) -> FirmwareResult<Option<CrashSummary>> {
        let mut buf = [0u8; CRASH_RECORD_BYTES];
        self.flash
            .read(CRASH_SECTOR_OFFSET, &mut buf)
            .map_err(|_| FirmwareError::Storage)?;
        match CrashSummary::decode(&buf) {
            Ok(summary) => Ok(summary),
            Err(CrashSummaryError::BadMagic) => {
                // Erased flash reads as 0xFF which decode already returns Ok(None) for.
                // A real BadMagic here means a genuine corruption.
                Err(FirmwareError::Storage)
            }
            Err(CrashSummaryError::BadCrc) => Err(FirmwareError::Storage),
        }
    }

    /// Erase the crash sector and write a new summary.
    ///
    /// The sector is erased first; `write_fatal` returns `Err` if the erase or write fails.
    pub fn write_fatal(&mut self, summary: &CrashSummary) -> FirmwareResult<()> {
        self.flash
            .erase(
                CRASH_SECTOR_OFFSET,
                CRASH_SECTOR_OFFSET + crate::flash::CRASH_SECTOR_SIZE,
            )
            .map_err(|_| FirmwareError::Storage)?;
        let mut buf = [0u8; CRASH_RECORD_BYTES];
        summary.encode(&mut buf);
        self.flash
            .write(CRASH_SECTOR_OFFSET, &buf)
            .map_err(|_| FirmwareError::Storage)?;
        // Verify the write by reading back and checking CRC.
        let mut verify = [0u8; CRASH_RECORD_BYTES];
        self.flash
            .read(CRASH_SECTOR_OFFSET, &mut verify)
            .map_err(|_| FirmwareError::Storage)?;
        if verify != buf {
            return Err(FirmwareError::Storage);
        }
        Ok(())
    }

    /// Erase the crash sector so it reads as empty.
    pub fn clear(&mut self) -> FirmwareResult<()> {
        self.flash
            .erase(
                CRASH_SECTOR_OFFSET,
                CRASH_SECTOR_OFFSET + crate::flash::CRASH_SECTOR_SIZE,
            )
            .map_err(|_| FirmwareError::Storage)?;
        // Verify the erase by reading back and confirming all 0xFF.
        let mut verify = [0u8; CRASH_RECORD_BYTES];
        self.flash
            .read(CRASH_SECTOR_OFFSET, &mut verify)
            .map_err(|_| FirmwareError::Storage)?;
        if !verify.iter().all(|&b| b == 0xFF) {
            return Err(FirmwareError::Storage);
        }
        Ok(())
    }

    /// Access the underlying flash reference (for testing / sector validation).
    pub fn flash(&self) -> &F {
        &self.flash
    }

    pub fn flash_mut(&mut self) -> &mut F {
        &mut self.flash
    }
}
