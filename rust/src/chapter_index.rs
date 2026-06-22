use crate::Error;
use crate::header::{read_le16, read_le32};
use crate::section::CHAPTER_INDEX_ENTRY_SIZE;

#[derive(Debug)]
pub struct ChapterEntry<'a> {
    pub index: u32,
    pub title: &'a [u8],
    pub page_index: u32,
    pub level: u16,
    pub entry_type: u16,
}

pub(crate) fn parse_chapter_entry_from_bytes(
    bytes: &[u8],
    index: u32,
    _title_offset: u32,
    _title_len: u32,
    page_count: u32,
) -> Result<ChapterEntry<'_>, Error> {
    if bytes.len() < CHAPTER_INDEX_ENTRY_SIZE {
        return Err(Error::InvalidChapterIndex);
    }
    let chapter_index_raw = read_le32(bytes, 0);
    if chapter_index_raw != index {
        return Err(Error::InvalidChapterIndex);
    }
    let pi = read_le32(bytes, 16);
    if pi >= page_count {
        return Err(Error::InvalidChapterIndex);
    }
    let entry_type = read_le16(bytes, 22);
    if entry_type != 3 && entry_type != 4 {
        return Err(Error::InvalidChapterIndex);
    }
    Ok(ChapterEntry {
        index: chapter_index_raw,
        title: b"",
        page_index: pi,
        level: read_le16(bytes, 20),
        entry_type,
    })
}
