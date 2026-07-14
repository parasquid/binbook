use core::cmp::Ordering;

use heapless::Vec;

use crate::crc32;

pub const MAX_PROGRESS_RECORDS: usize = 200;
pub const STATE_HEADER_SIZE: usize = 72;
pub const STATE_RECORD_SIZE: usize = 44;
pub const MAX_STATE_BYTES: usize = STATE_HEADER_SIZE + MAX_PROGRESS_RECORDS * STATE_RECORD_SIZE;

const MAGIC: &[u8; 4] = b"BBST";
const VERSION: u16 = 1;
const CRC_OFFSET: usize = 68;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SavedMode {
    Menu = 0,
    Reading = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressRecord {
    pub filename_hash: u64,
    pub rendition_hash: [u8; 32],
    pub page: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderStateError {
    BufferTooSmall,
    Magic,
    Version,
    HeaderSize,
    Reserved,
    Mode,
    Length,
    Crc,
    TooManyRecords,
    Duplicate,
    ZeroRenditionHash,
    InvalidModeState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderState {
    pub generation: u32,
    pub saved_mode: SavedMode,
    pub selected_filename_hash: u64,
    pub active_filename_hash: u64,
    pub active_rendition_hash: [u8; 32],
    records: Vec<ProgressRecord, MAX_PROGRESS_RECORDS>,
}

impl ReaderState {
    pub const fn empty() -> Self {
        Self {
            generation: 0,
            saved_mode: SavedMode::Menu,
            selected_filename_hash: 0,
            active_filename_hash: 0,
            active_rendition_hash: [0; 32],
            records: Vec::new(),
        }
    }

    pub fn records(&self) -> &[ProgressRecord] {
        &self.records
    }

    pub fn lookup_page(&self, filename_hash: u64, rendition_hash: &[u8; 32]) -> Option<u32> {
        self.records
            .iter()
            .find(|record| {
                record.filename_hash == filename_hash && &record.rendition_hash == rendition_hash
            })
            .map(|record| record.page)
    }

    pub fn set_progress(&mut self, record: ProgressRecord) -> Result<(), ReaderStateError> {
        if record.rendition_hash.iter().all(|byte| *byte == 0) {
            return Err(ReaderStateError::ZeroRenditionHash);
        }
        if let Some(existing) = self.records.iter_mut().find(|existing| {
            existing.filename_hash == record.filename_hash
                && existing.rendition_hash == record.rendition_hash
        }) {
            *existing = record;
            return Ok(());
        }
        self.records
            .push(record)
            .map_err(|_| ReaderStateError::TooManyRecords)
    }

    /// Publishes a page only after its visible render has succeeded.
    ///
    /// This keeps the selected identity, per-rendition progress, and saved
    /// reading mode in one operation for runtime callers.
    pub fn commit_visible_page(
        &mut self,
        filename_hash: u64,
        rendition_hash: [u8; 32],
        page: u32,
    ) -> Result<(), ReaderStateError> {
        if filename_hash == 0 || rendition_hash.iter().all(|byte| *byte == 0) {
            return Err(ReaderStateError::InvalidModeState);
        }
        self.records.retain(|record| {
            record.filename_hash != filename_hash || record.rendition_hash == rendition_hash
        });
        self.set_progress(ProgressRecord {
            filename_hash,
            rendition_hash,
            page,
        })?;
        self.selected_filename_hash = filename_hash;
        self.set_reading(filename_hash, rendition_hash)
    }

    pub fn set_reading(
        &mut self,
        filename_hash: u64,
        rendition_hash: [u8; 32],
    ) -> Result<(), ReaderStateError> {
        if filename_hash == 0
            || rendition_hash.iter().all(|byte| *byte == 0)
            || self.lookup_page(filename_hash, &rendition_hash).is_none()
        {
            return Err(ReaderStateError::InvalidModeState);
        }
        self.saved_mode = SavedMode::Reading;
        self.active_filename_hash = filename_hash;
        self.active_rendition_hash = rendition_hash;
        Ok(())
    }

    pub fn set_menu(&mut self) {
        self.saved_mode = SavedMode::Menu;
        self.active_filename_hash = 0;
        self.active_rendition_hash = [0; 32];
    }

    pub fn prune(&mut self, mut keep_filename: impl FnMut(u64) -> bool) {
        let keep_selected =
            self.selected_filename_hash == 0 || keep_filename(self.selected_filename_hash);
        let keep_active =
            self.saved_mode != SavedMode::Reading || keep_filename(self.active_filename_hash);
        self.records
            .retain(|record| keep_filename(record.filename_hash));
        if !keep_selected {
            self.selected_filename_hash = 0;
        }
        if !keep_active {
            self.set_menu();
        }
    }

    #[cfg(feature = "sd-storage")]
    pub(crate) fn retain_record_mask(&mut self, keep: &[bool]) -> usize {
        let before = self.records.len();
        let mut index = 0usize;
        self.records.retain(|_| {
            let retain = keep.get(index).copied().unwrap_or(false);
            index += 1;
            retain
        });
        before - self.records.len()
    }

    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ReaderStateError> {
        self.encode_with_generation(self.generation, out)
    }

    pub(crate) fn encode_with_generation(
        &self,
        generation: u32,
        out: &mut [u8],
    ) -> Result<usize, ReaderStateError> {
        self.validate_mode_state()?;
        let used = STATE_HEADER_SIZE + self.records.len() * STATE_RECORD_SIZE;
        if out.len() < used {
            return Err(ReaderStateError::BufferTooSmall);
        }
        out[..used].fill(0);
        out[0..4].copy_from_slice(MAGIC);
        out[4..6].copy_from_slice(&VERSION.to_le_bytes());
        out[6..8].copy_from_slice(&(STATE_HEADER_SIZE as u16).to_le_bytes());
        out[8..12].copy_from_slice(&generation.to_le_bytes());
        out[12] = self.saved_mode as u8;
        out[16..24].copy_from_slice(&self.selected_filename_hash.to_le_bytes());
        out[24..32].copy_from_slice(&self.active_filename_hash.to_le_bytes());
        out[32..64].copy_from_slice(&self.active_rendition_hash);
        out[64..66].copy_from_slice(&(self.records.len() as u16).to_le_bytes());

        let mut sorted = self.records.clone();
        sorted.as_mut_slice().sort_unstable_by(record_order);
        for (index, record) in sorted.iter().enumerate() {
            let start = STATE_HEADER_SIZE + index * STATE_RECORD_SIZE;
            out[start..start + 8].copy_from_slice(&record.filename_hash.to_le_bytes());
            out[start + 8..start + 40].copy_from_slice(&record.rendition_hash);
            out[start + 40..start + 44].copy_from_slice(&record.page.to_le_bytes());
        }
        rewrite_crc(&mut out[..used]);
        Ok(used)
    }

    pub(crate) fn matches_except_generation(&self, other: &Self) -> bool {
        self.saved_mode == other.saved_mode
            && self.selected_filename_hash == other.selected_filename_hash
            && self.active_filename_hash == other.active_filename_hash
            && self.active_rendition_hash == other.active_rendition_hash
            && self.records.len() == other.records.len()
            && self
                .records
                .iter()
                .all(|record| other.records.iter().any(|candidate| candidate == record))
    }

    pub fn decode(data: &[u8]) -> Result<Self, ReaderStateError> {
        if data.len() < STATE_HEADER_SIZE {
            return Err(ReaderStateError::Length);
        }
        if &data[0..4] != MAGIC {
            return Err(ReaderStateError::Magic);
        }
        if read_u16(data, 4) != VERSION {
            return Err(ReaderStateError::Version);
        }
        if read_u16(data, 6) as usize != STATE_HEADER_SIZE {
            return Err(ReaderStateError::HeaderSize);
        }
        if data[13..16] != [0; 3] || data[66..68] != [0; 2] {
            return Err(ReaderStateError::Reserved);
        }
        let count = read_u16(data, 64) as usize;
        if count > MAX_PROGRESS_RECORDS {
            return Err(ReaderStateError::TooManyRecords);
        }
        if data.len() != STATE_HEADER_SIZE + count * STATE_RECORD_SIZE {
            return Err(ReaderStateError::Length);
        }
        let expected_crc = read_u32(data, CRC_OFFSET);
        let mut state = u32::MAX;
        for (index, byte) in data.iter().copied().enumerate() {
            let byte = if (CRC_OFFSET..CRC_OFFSET + 4).contains(&index) {
                0
            } else {
                byte
            };
            state = crc32::update_byte(state, byte);
        }
        if !state != expected_crc {
            return Err(ReaderStateError::Crc);
        }
        let saved_mode = match data[12] {
            0 => SavedMode::Menu,
            1 => SavedMode::Reading,
            _ => return Err(ReaderStateError::Mode),
        };
        let mut decoded = Self {
            generation: read_u32(data, 8),
            saved_mode,
            selected_filename_hash: read_u64(data, 16),
            active_filename_hash: read_u64(data, 24),
            active_rendition_hash: data[32..64].try_into().unwrap(),
            records: Vec::new(),
        };
        for index in 0..count {
            let start = STATE_HEADER_SIZE + index * STATE_RECORD_SIZE;
            let record = ProgressRecord {
                filename_hash: read_u64(data, start),
                rendition_hash: data[start + 8..start + 40].try_into().unwrap(),
                page: read_u32(data, start + 40),
            };
            if record.rendition_hash.iter().all(|byte| *byte == 0) {
                return Err(ReaderStateError::ZeroRenditionHash);
            }
            if decoded
                .lookup_page(record.filename_hash, &record.rendition_hash)
                .is_some()
            {
                return Err(ReaderStateError::Duplicate);
            }
            decoded
                .records
                .push(record)
                .map_err(|_| ReaderStateError::TooManyRecords)?;
        }
        decoded.validate_mode_state()?;
        Ok(decoded)
    }

    fn validate_mode_state(&self) -> Result<(), ReaderStateError> {
        match self.saved_mode {
            SavedMode::Menu
                if self.active_filename_hash == 0 && self.active_rendition_hash == [0; 32] =>
            {
                Ok(())
            }
            SavedMode::Reading
                if self.active_filename_hash != 0
                    && self.active_rendition_hash != [0; 32]
                    && self
                        .lookup_page(self.active_filename_hash, &self.active_rendition_hash)
                        .is_some() =>
            {
                Ok(())
            }
            _ => Err(ReaderStateError::InvalidModeState),
        }
    }
}

fn record_order(left: &ProgressRecord, right: &ProgressRecord) -> Ordering {
    left.filename_hash
        .cmp(&right.filename_hash)
        .then_with(|| left.rendition_hash.cmp(&right.rendition_hash))
}

pub fn filename_hash(filename: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in filename {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

pub fn select_newer(a: Option<&ReaderState>, b: Option<&ReaderState>) -> Option<usize> {
    match (a, b) {
        (None, None) => None,
        (Some(_), None) => Some(0),
        (None, Some(_)) => Some(1),
        (Some(a), Some(b)) => {
            if b.generation.wrapping_sub(a.generation) < 0x8000_0000 {
                Some(1)
            } else {
                Some(0)
            }
        }
    }
}

#[doc(hidden)]
pub fn rewrite_crc(data: &mut [u8]) {
    data[CRC_OFFSET..CRC_OFFSET + 4].fill(0);
    let crc = crc32::iso_hdlc(data);
    data[CRC_OFFSET..CRC_OFFSET + 4].copy_from_slice(&crc.to_le_bytes());
}

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap())
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn read_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_ignores_progress_record_insertion_order() {
        let first = ProgressRecord {
            filename_hash: 1,
            rendition_hash: [1; 32],
            page: 3,
        };
        let second = ProgressRecord {
            filename_hash: 2,
            rendition_hash: [2; 32],
            page: 7,
        };
        let mut inserted = ReaderState::empty();
        inserted.set_progress(second).unwrap();
        inserted.set_progress(first).unwrap();
        let mut canonical = ReaderState::empty();
        canonical.set_progress(first).unwrap();
        canonical.set_progress(second).unwrap();

        assert!(inserted.matches_except_generation(&canonical));
    }
}
