//! Resume state management for firmware.
//!
//! Stores the last book, page, and menu state in internal flash for quick
//! resume on boot. Uses a reserved region outside the crash sector and
//! existing FlashStorage table.

use crate::error::{FirmwareError, FirmwareResult};
use embedded_storage::nor_flash::ReadNorFlash;
use embedded_storage::nor_flash::NorFlash;

/// Magic bytes to identify valid resume records.
const RESUME_MAGIC: &[u8; 4] = b"BRSM";

/// Current resume state format version.
const RESUME_VERSION: u8 = 1;

/// Resume record layout: 81 bytes total.
///
/// ```text
/// [0..4]   magic: u32 (0x4252534D = "BRSM")
/// [4]     version: u8
/// [5..8]  reserved: [u8; 3]
/// [8..72] last_book_name: [u8; 64] (UTF-8, null-terminated)
/// [72..76] last_page: u32 (page number)
/// [76..80] menu_top: u32 (menu viewport top)
/// [80]    menu_selected: u8 (menu viewport selected)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResumeRecord {
    pub last_book_name: [u8; 64],
    pub last_page: u32,
    pub menu_top: u32,
    pub menu_selected: u8,
}

impl ResumeRecord {
    /// Record size in bytes.
    pub const SIZE: usize = 81;

    /// Default empty resume record.
    pub const fn empty() -> Self {
        Self {
            last_book_name: [0; 64],
            last_page: 0,
            menu_top: 0,
            menu_selected: 0,
        }
    }

    /// Encode resume record into byte buffer.
    pub fn encode(&self, out: &mut [u8]) -> FirmwareResult<()> {
        if out.len() < Self::SIZE {
            return Err(FirmwareError::InvalidParameter);
        }

        out[0..4].copy_from_slice(RESUME_MAGIC);
        out[4] = RESUME_VERSION;
        out[5..8].copy_from_slice(&[0u8; 3]);
        out[8..72].copy_from_slice(&self.last_book_name);
        out[72..76].copy_from_slice(&self.last_page.to_le_bytes());
        out[76..80].copy_from_slice(&self.menu_top.to_le_bytes());
        out[80] = self.menu_selected;

        Ok(())
    }

    /// Decode resume record from byte buffer.
    pub fn decode(data: &[u8]) -> FirmwareResult<Self> {
        if data.len() < Self::SIZE {
            return Err(FirmwareError::InvalidParameter);
        }

        if &data[0..4] != RESUME_MAGIC {
            return Err(FirmwareError::Storage);
        }

        let last_book_name: [u8; 64] = data[8..72]
            .try_into()
            .map_err(|_| FirmwareError::Storage)?;
        let last_page = u32::from_le_bytes([data[72], data[73], data[74], data[75]]);
        let menu_top = u32::from_le_bytes([data[76], data[77], data[78], data[79]]);
        let menu_selected = data[80];

        Ok(Self {
            last_book_name,
            last_page,
            menu_top,
            menu_selected,
        })
    }

    /// Check if resume record is populated (has a non-empty book name).
    pub fn is_empty(&self) -> bool {
        self.last_book_name[0] == 0
    }
}

/// Resume state manager using internal flash.
pub struct ResumeStorage<F> {
    flash: F,
    offset: u32,
}

impl<F: ReadNorFlash + NorFlash> ResumeStorage<F> {
    /// Create a new resume storage manager.
    ///
    /// The offset must be aligned to a flash sector boundary and outside the
    /// crash sector region.
    pub fn new(flash: F, offset: u32) -> Self {
        Self { flash, offset }
    }

    /// Read resume record from flash.
    pub fn read(&mut self) -> FirmwareResult<Option<ResumeRecord>> {
        let mut data = [0u8; ResumeRecord::SIZE];
        self.flash
            .read(self.offset, &mut data)
            .map_err(|_| FirmwareError::Storage)?;

        ResumeRecord::decode(&data).map(Some)
    }

    /// Write resume record to flash.
    ///
    /// Only writes if the record differs from the current flash contents to
    /// minimize wear. Returns `true` if a write was performed.
    pub fn write(&mut self, record: &ResumeRecord) -> FirmwareResult<bool> {
        let mut data = [0u8; ResumeRecord::SIZE];
        record.encode(&mut data)?;

        // Read current contents to compare
        let mut current = [0u8; ResumeRecord::SIZE];
        self.flash
            .read(self.offset, &mut current)
            .map_err(|_| FirmwareError::Storage)?;

        // Skip write if unchanged
        if data == current {
            return Ok(false);
        }

        // Erase and write
        let sector_size = self.flash.capacity() as u32;
        self.flash
            .erase(self.offset, sector_size)
            .map_err(|_| FirmwareError::Storage)?;
        self.flash
            .write(self.offset, &data)
            .map_err(|_| FirmwareError::Storage)?;

        Ok(true)
    }

    /// Clear the resume record.
    pub fn clear(&mut self) -> FirmwareResult<()> {
        let empty = ResumeRecord::empty();
        self.write(&empty).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_resume_record() {
        let record = ResumeRecord::empty();
        assert!(record.is_empty());
        assert_eq!(record.last_page, 0);
        assert_eq!(record.menu_top, 0);
        assert_eq!(record.menu_selected, 0);
    }

    #[test]
    fn encode_decode_round_trip() {
        let original = ResumeRecord {
            last_book_name: {
                let mut name = [0u8; 64];
                name[..12].copy_from_slice(b"test.binbook");
                name
            },
            last_page: 42,
            menu_top: 5,
            menu_selected: 3,
        };

        let mut data = [0u8; ResumeRecord::SIZE];
        original.encode(&mut data).unwrap();
        let decoded = ResumeRecord::decode(&data).unwrap();

        assert_eq!(decoded, original);
        assert!(!decoded.is_empty());
    }

    #[test]
    fn decode_rejects_invalid_magic() {
        let mut data = [0u8; ResumeRecord::SIZE];
        data[0..4].copy_from_slice(b"XXXX");
        data[4] = RESUME_VERSION;

        assert!(ResumeRecord::decode(&data).is_err());
    }

    #[test]
    fn decode_rejects_short_buffer() {
        let data = [0u8; 10];
        assert!(ResumeRecord::decode(&data).is_err());
    }
}