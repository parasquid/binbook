use crate::header::read_le32;
use crate::Error;

pub const PAGE_TRANSITION_INDEX_ENTRY_SIZE: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageTransitionEntry {
    pub from_page_number: u32,
    pub to_page_number: u32,
    pub changed_chunk_mask: u32,
    pub first_changed_chunk: u16,
    pub changed_chunk_count: u16,
    pub flags: u16,
}

pub fn parse_page_transition_entry(bytes: &[u8]) -> Result<PageTransitionEntry, Error> {
    if bytes.len() < PAGE_TRANSITION_INDEX_ENTRY_SIZE {
        return Err(Error::InvalidSection);
    }
    Ok(PageTransitionEntry {
        from_page_number: read_le32(bytes, 0),
        to_page_number: read_le32(bytes, 4),
        changed_chunk_mask: read_le32(bytes, 8),
        first_changed_chunk: u16::from_le_bytes([bytes[12], bytes[13]]),
        changed_chunk_count: u16::from_le_bytes([bytes[14], bytes[15]]),
        flags: u16::from_le_bytes([bytes[16], bytes[17]]),
    })
}
