use xteink_hal::{Flash, HalError, HalResult};

use crate::diag_log::{CrashSummary, CrashSummaryError, CRASH_RECORD_BYTES};
use crate::flash::CRASH_SECTOR_OFFSET;

pub struct CrashStore<F> {
    flash: F,
}

impl<F: Flash> CrashStore<F> {
    pub fn new(flash: F) -> Self {
        Self { flash }
    }

    /// Read and validate the crash summary from flash.
    ///
    /// Returns `Ok(None)` when the sector is erased (all `0xFF`), `Ok(Some(summary))` for a
    /// valid summary, and `Err(InternalError)` for a corrupt sector or flash failure.
    pub fn read(&mut self) -> HalResult<Option<CrashSummary>> {
        let mut buf = [0u8; CRASH_RECORD_BYTES];
        self.flash.read(CRASH_SECTOR_OFFSET, &mut buf)?;
        match CrashSummary::decode(&buf) {
            Ok(summary) => Ok(summary),
            Err(CrashSummaryError::BadMagic) => {
                // Erased flash reads as 0xFF which decode already returns Ok(None) for.
                // A real BadMagic here means a genuine corruption.
                Err(HalError::Flash)
            }
            Err(CrashSummaryError::BadCrc) => Err(HalError::Flash),
        }
    }

    /// Erase the crash sector and write a new summary.
    ///
    /// The sector is erased first; `write_fatal` returns `Err` if the erase or write fails.
    pub fn write_fatal(&mut self, summary: &CrashSummary) -> HalResult<()> {
        self.flash.erase_sector(CRASH_SECTOR_OFFSET)?;
        let mut buf = [0u8; CRASH_RECORD_BYTES];
        summary.encode(&mut buf);
        self.flash.write(CRASH_SECTOR_OFFSET, &buf)?;
        // Verify the write by reading back and checking CRC.
        let mut verify = [0u8; CRASH_RECORD_BYTES];
        self.flash.read(CRASH_SECTOR_OFFSET, &mut verify)?;
        if verify != buf {
            return Err(HalError::Flash);
        }
        Ok(())
    }

    /// Erase the crash sector so it reads as empty.
    pub fn clear(&mut self) -> HalResult<()> {
        self.flash.erase_sector(CRASH_SECTOR_OFFSET)?;
        // Verify the erase by reading back and confirming all 0xFF.
        let mut verify = [0u8; CRASH_RECORD_BYTES];
        self.flash.read(CRASH_SECTOR_OFFSET, &mut verify)?;
        if !verify.iter().all(|&b| b == 0xFF) {
            return Err(HalError::Flash);
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
