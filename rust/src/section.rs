use crate::Error;
use crate::header::{read_le16, read_le32, read_le64};

pub const SECTION_STRING_TABLE: u16 = 1;
pub const SECTION_PAGE_INDEX: u16 = 40;
pub const SECTION_NAV_INDEX: u16 = 41;
pub const SECTION_CHAPTER_INDEX: u16 = 43;
pub const SECTION_PAGE_DATA: u16 = 50;

pub const SECTION_ENTRY_SIZE: usize = 40;
pub const PAGE_INDEX_ENTRY_SIZE: usize = 128;
pub const NAV_INDEX_ENTRY_SIZE: usize = 48;
pub const CHAPTER_INDEX_ENTRY_SIZE: usize = 32;

#[derive(Debug, Clone, Default)]
pub struct Section {
    pub section_id: u16,
    pub offset: u64,
    pub length: u64,
    pub entry_size: u32,
    pub record_count: u32,
}

pub(crate) fn parse_section(bytes: &[u8]) -> Section {
    Section {
        section_id: read_le16(bytes, 0),
        offset: read_le64(bytes, 4),
        length: read_le64(bytes, 12),
        entry_size: read_le32(bytes, 20),
        record_count: read_le32(bytes, 24),
    }
}

pub(crate) struct RequiredSections {
    pub string_table: Section,
    pub page_index: Section,
    pub nav_index: Section,
    pub chapter_index: Section,
    pub page_data: Section,
}

pub(crate) fn parse_sections_from_table(
    table_bytes: &[u8],
    section_count: u16,
    page_data_offset: u64,
    page_data_length: u64,
) -> Result<RequiredSections, Error> {
    let mut string_table = Section::default();
    let mut page_index = Section::default();
    let mut nav_index = Section::default();
    let mut chapter_index = Section::default();
    let mut page_data = Section::default();

    for i in 0..section_count {
        let off = i as usize * SECTION_ENTRY_SIZE;
        let end = off + SECTION_ENTRY_SIZE;
        if end > table_bytes.len() {
            return Err(Error::InvalidSection);
        }
        let section = parse_section(&table_bytes[off..end]);
        match section.section_id {
            SECTION_STRING_TABLE => string_table = section,
            SECTION_PAGE_INDEX => page_index = section,
            SECTION_NAV_INDEX => nav_index = section,
            SECTION_CHAPTER_INDEX => chapter_index = section,
            SECTION_PAGE_DATA => page_data = section,
            _ => {}
        }
    }

    if string_table.section_id != SECTION_STRING_TABLE {
        return Err(Error::MissingSection(SECTION_STRING_TABLE));
    }
    if page_index.section_id != SECTION_PAGE_INDEX {
        return Err(Error::MissingSection(SECTION_PAGE_INDEX));
    }
    if page_index.entry_size as usize != PAGE_INDEX_ENTRY_SIZE {
        return Err(Error::InvalidPageIndex);
    }
    if nav_index.section_id != SECTION_NAV_INDEX {
        return Err(Error::MissingSection(SECTION_NAV_INDEX));
    }
    if nav_index.entry_size as usize != NAV_INDEX_ENTRY_SIZE {
        return Err(Error::InvalidNavIndex);
    }
    if chapter_index.section_id != SECTION_CHAPTER_INDEX {
        return Err(Error::MissingSection(SECTION_CHAPTER_INDEX));
    }
    if chapter_index.entry_size as usize != CHAPTER_INDEX_ENTRY_SIZE {
        return Err(Error::InvalidChapterIndex);
    }
    if page_data.section_id != SECTION_PAGE_DATA {
        return Err(Error::MissingSection(SECTION_PAGE_DATA));
    }
    if page_index.record_count == 0 {
        return Err(Error::InvalidPageIndex);
    }
    if page_data.offset != page_data_offset || page_data.length != page_data_length {
        return Err(Error::InvalidSection);
    }

    Ok(RequiredSections {
        string_table,
        page_index,
        nav_index,
        chapter_index,
        page_data,
    })
}
