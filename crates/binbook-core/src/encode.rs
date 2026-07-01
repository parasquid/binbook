pub const HEADER_SIZE: usize = 256;
pub const SECTION_RECORD_SIZE: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    BufferTooSmall { required: usize, provided: usize },
}

pub trait WireEncode {
    fn encode_into(&self, output: &mut [u8]) -> Result<(), EncodeError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileHeader {
    pub file_size: u64,
    pub section_table_offset: u64,
    pub section_table_length: u32,
    pub section_count: u16,
    pub page_data_offset: u64,
    pub page_data_length: u64,
    pub file_crc32: u32,
    pub header_crc32: u32,
    pub header_flags: u16,
}

impl Default for FileHeader {
    fn default() -> Self {
        Self {
            file_size: 0,
            section_table_offset: HEADER_SIZE as u64,
            section_table_length: 0,
            section_count: 0,
            page_data_offset: 0,
            page_data_length: 0,
            file_crc32: 0,
            header_crc32: 0,
            header_flags: 0,
        }
    }
}

impl WireEncode for FileHeader {
    fn encode_into(&self, output: &mut [u8]) -> Result<(), EncodeError> {
        let record = require(output, HEADER_SIZE)?;
        record.fill(0);
        record[..8].copy_from_slice(b"BINBOOK\0");
        put_u16(record, 12, HEADER_SIZE as u16);
        put_u16(record, 14, self.header_flags);
        put_u64(record, 16, self.file_size);
        put_u64(record, 24, self.section_table_offset);
        put_u32(record, 32, self.section_table_length);
        put_u16(record, 36, SECTION_RECORD_SIZE as u16);
        put_u16(record, 38, self.section_count);
        put_u16(record, 40, 128);
        put_u16(record, 42, 48);
        put_u64(record, 44, self.page_data_offset);
        put_u64(record, 52, self.page_data_length);
        put_u32(record, 60, self.file_crc32);
        put_u32(record, 64, self.header_crc32);
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SectionTableEntry {
    pub section_id: u16,
    pub section_flags: u16,
    pub offset: u64,
    pub length: u64,
    pub entry_size: u32,
    pub record_count: u32,
    pub crc32: u32,
}

impl WireEncode for SectionTableEntry {
    fn encode_into(&self, output: &mut [u8]) -> Result<(), EncodeError> {
        let record = require(output, SECTION_RECORD_SIZE)?;
        record.fill(0);
        put_u16(record, 0, self.section_id);
        put_u16(record, 2, self.section_flags);
        put_u64(record, 4, self.offset);
        put_u64(record, 12, self.length);
        put_u32(record, 20, self.entry_size);
        put_u32(record, 24, self.record_count);
        put_u32(record, 28, self.crc32);
        Ok(())
    }
}

pub(crate) fn require(output: &mut [u8], required: usize) -> Result<&mut [u8], EncodeError> {
    let provided = output.len();
    output
        .get_mut(..required)
        .ok_or(EncodeError::BufferTooSmall { required, provided })
}

pub(crate) fn put_u16(output: &mut [u8], offset: usize, value: u16) {
    output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_u32(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_u64(output: &mut [u8], offset: usize, value: u64) {
    output[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
