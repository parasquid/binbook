use crate::header::read_le32;
use crate::Error;

pub const PAGE_CHUNK_INDEX_ENTRY_SIZE: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageChunkEntry {
    pub page_number: u32,
    pub plane_slot: u8,
    pub chunk_index: u8,
    pub row_start: u16,
    pub row_count: u16,
    pub page_data_offset: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
}

pub fn parse_page_chunk_entry(bytes: &[u8]) -> Result<PageChunkEntry, Error> {
    if bytes.len() < PAGE_CHUNK_INDEX_ENTRY_SIZE {
        return Err(Error::InvalidSection);
    }
    Ok(PageChunkEntry {
        page_number: read_le32(bytes, 0),
        plane_slot: bytes[4],
        chunk_index: bytes[5],
        row_start: u16::from_le_bytes([bytes[6], bytes[7]]),
        row_count: u16::from_le_bytes([bytes[8], bytes[9]]),
        page_data_offset: read_le32(bytes, 12),
        compressed_size: read_le32(bytes, 16),
        uncompressed_size: read_le32(bytes, 20),
    })
}
