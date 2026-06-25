use xteink_hal::{Flash, HalResult};

pub const STORAGE_OFFSET: u32 = 0x00FB_0000;
pub const STORAGE_SIZE: u32 = 192 * 1024;
pub const MAX_FILE_SIZE: u32 = 180 * 1024;
pub const MAX_FILES: usize = 8;
pub const FILE_ENTRY_SIZE: usize = 44;

const FILE_TABLE_OFFSET: u32 = 0;
const VALID_ENTRY: u8 = 0x00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileInfo {
    pub offset: u32,
    pub size: u32,
}

pub struct FlashStorage<F> {
    flash: F,
}

impl<F: Flash> FlashStorage<F> {
    pub fn new(flash: F) -> Self {
        Self { flash }
    }

    pub fn find(&mut self, name: &str) -> HalResult<Option<FileInfo>> {
        let mut entry = [0u8; FILE_ENTRY_SIZE];

        for index in 0..MAX_FILES {
            let offset = FILE_TABLE_OFFSET + (index * FILE_ENTRY_SIZE) as u32;
            self.flash.read(offset, &mut entry)?;

            if entry[40] != VALID_ENTRY {
                continue;
            }

            if entry_name_matches(&entry[..32], name.as_bytes()) {
                return Ok(Some(FileInfo {
                    offset: u32::from_le_bytes([entry[32], entry[33], entry[34], entry[35]]),
                    size: u32::from_le_bytes([entry[36], entry[37], entry[38], entry[39]]),
                }));
            }
        }

        Ok(None)
    }

    pub fn read_file(&self, info: &FileInfo, offset: u32, buf: &mut [u8]) -> HalResult<()> {
        let end = offset.saturating_add(buf.len() as u32);
        if end > info.size {
            return Err(xteink_hal::HalError::InvalidParam);
        }

        self.flash.read(info.offset + offset, buf)
    }
}

fn entry_name_matches(stored: &[u8], requested: &[u8]) -> bool {
    let stored_len = stored
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(stored.len());
    &stored[..stored_len] == requested
}
