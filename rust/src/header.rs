use crate::Error;

pub(crate) const HEADER_SIZE: usize = 256;
const MAGIC: &[u8] = b"BINBOOK\0";

#[derive(Debug)]
pub struct Header {
    pub section_table_offset: u64,
    pub section_table_length: u32,
    pub section_table_entry_size: u16,
    pub section_count: u16,
    pub page_index_entry_size: u16,
    pub nav_index_entry_size: u16,
    pub page_data_offset: u64,
    pub page_data_length: u64,
}

pub(crate) fn read_le16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

pub(crate) fn read_le32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}

pub(crate) fn read_le64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
        data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7],
    ])
}

pub(crate) fn parse_header(data: &[u8]) -> Result<Header, Error> {
    if data.len() < HEADER_SIZE {
        return Err(Error::InvalidHeader);
    }
    if &data[0..8] != MAGIC {
        return Err(Error::InvalidMagic);
    }
    let header_size = read_le16(data, 12);
    if header_size as usize != HEADER_SIZE {
        return Err(Error::UnsupportedVersion);
    }
    let file_size = read_le64(data, 16);
    let h = Header {
        section_table_offset: read_le64(data, 24),
        section_table_length: read_le32(data, 32),
        section_table_entry_size: read_le16(data, 36),
        section_count: read_le16(data, 38),
        page_index_entry_size: read_le16(data, 40),
        nav_index_entry_size: read_le16(data, 42),
        page_data_offset: read_le64(data, 44),
        page_data_length: read_le64(data, 52),
    };
    if file_size < HEADER_SIZE as u64
        || h.section_table_entry_size != 40
        || h.page_index_entry_size != 128
        || h.nav_index_entry_size != 48
        || h.section_table_offset < HEADER_SIZE as u64
        || h.section_count == 0
        || h.section_table_length < h.section_count as u32 * 40
    {
        return Err(Error::UnsupportedVersion);
    }
    Ok(h)
}
