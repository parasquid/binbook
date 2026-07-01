use crate::header::{read_u16, read_u32};
use crate::{ByteLength, ChunkIndex, FileOffset, FormatError, PageNumber, PlaneSlot};

pub const CHUNK_RECORD_SIZE: usize = crate::index_encode::PAGE_CHUNK_INDEX_RECORD_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageChunk {
    pub page_number: PageNumber,
    pub plane_slot: PlaneSlot,
    pub chunk_index: ChunkIndex,
    pub row_start: u16,
    pub row_count: u16,
    pub offset: FileOffset,
    pub compressed_length: ByteLength,
    pub uncompressed_length: ByteLength,
}

pub(crate) fn parse(
    bytes: &[u8],
    page_count: u32,
    page_data_length: u64,
) -> Result<PageChunk, FormatError> {
    if bytes.len() < CHUNK_RECORD_SIZE {
        return Err(FormatError::InvalidChunk);
    }
    let page = read_u32(bytes, 0)?;
    if page >= page_count {
        return Err(FormatError::InvalidChunk);
    }
    let raw_chunk = bytes[5];
    if raw_chunk >= 32 {
        return Err(FormatError::InvalidChunk);
    }
    let offset = read_u32(bytes, 12)?;
    let compressed = read_u32(bytes, 16)?;
    let end = u64::from(offset)
        .checked_add(u64::from(compressed))
        .ok_or(FormatError::InvalidChunk)?;
    if compressed == 0 || end > page_data_length || read_u16(bytes, 10)? != 0 {
        return Err(FormatError::InvalidChunk);
    }
    Ok(PageChunk {
        page_number: PageNumber::from_validated(page),
        plane_slot: PlaneSlot::try_from(bytes[4])?,
        chunk_index: ChunkIndex::from_validated(raw_chunk),
        row_start: read_u16(bytes, 6)?,
        row_count: read_u16(bytes, 8)?,
        offset: FileOffset::from_validated(u64::from(offset)),
        compressed_length: ByteLength::from_validated(compressed),
        uncompressed_length: ByteLength::from_validated(read_u32(bytes, 20)?),
    })
}
