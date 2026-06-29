use crate::header::{read_u16, read_u32};
use crate::{FormatError, PageNumber};

pub const TRANSITION_RECORD_SIZE: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageTransition {
    pub from: PageNumber,
    pub to: PageNumber,
    pub changed_chunk_mask: u32,
    pub first_changed_chunk: u16,
    pub changed_chunk_count: u16,
    pub flags: u16,
}

pub(crate) fn parse(bytes: &[u8], page_count: u32) -> Result<PageTransition, FormatError> {
    if bytes.len() < TRANSITION_RECORD_SIZE {
        return Err(FormatError::InvalidTransition);
    }
    let from = read_u32(bytes, 0)?;
    let to = read_u32(bytes, 4)?;
    let mask = read_u32(bytes, 8)?;
    let first = read_u16(bytes, 12)?;
    let count = read_u16(bytes, 14)?;
    let flags = read_u16(bytes, 16)?;
    if from >= page_count
        || to >= page_count
        || flags != 0
        || read_u16(bytes, 18)? != 0
        || read_u32(bytes, 20)? != 0
        || (mask == 0 && (first != 0 || count != 0))
    {
        return Err(FormatError::InvalidTransition);
    }
    Ok(PageTransition {
        from: PageNumber::from_validated(from),
        to: PageNumber::from_validated(to),
        changed_chunk_mask: mask,
        first_changed_chunk: first,
        changed_chunk_count: count,
        flags,
    })
}
