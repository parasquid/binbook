use crate::header::{read_u16, read_u32, read_u64, Header};
use crate::FormatError;

pub(crate) const ENTRY_SIZE: usize = 40;
pub(crate) const STRING_TABLE: u16 = 1;
pub(crate) const DISPLAY_PROFILE: u16 = 10;
pub(crate) const LAYOUT_PROFILE: u16 = 11;
pub(crate) const READER_REQUIREMENTS: u16 = 12;
pub(crate) const SOURCE_IDENTITY: u16 = 20;
pub(crate) const BOOK_METADATA: u16 = 21;
pub(crate) const RENDITION_IDENTITY: u16 = 22;
pub(crate) const FONT_POLICY: u16 = 30;
pub(crate) const TYPOGRAPHY_POLICY: u16 = 31;
pub(crate) const IMAGE_POLICY: u16 = 32;
pub(crate) const COMPRESSION_POLICY: u16 = 33;
pub(crate) const CHROME_POLICY: u16 = 34;
pub(crate) const FONT_RESOURCE_INDEX: u16 = 35;
pub(crate) const PAGE_INDEX: u16 = 40;
pub(crate) const NAV_INDEX: u16 = 41;
pub(crate) const CHAPTER_INDEX: u16 = 43;
pub(crate) const PAGE_CHUNK_INDEX: u16 = 44;
pub(crate) const PAGE_TRANSITION_INDEX: u16 = 45;
pub(crate) const PAGE_DATA: u16 = 50;

const REQUIRED: [u16; 19] = [
    STRING_TABLE,
    DISPLAY_PROFILE,
    LAYOUT_PROFILE,
    READER_REQUIREMENTS,
    SOURCE_IDENTITY,
    BOOK_METADATA,
    RENDITION_IDENTITY,
    FONT_POLICY,
    TYPOGRAPHY_POLICY,
    IMAGE_POLICY,
    COMPRESSION_POLICY,
    CHROME_POLICY,
    FONT_RESOURCE_INDEX,
    PAGE_INDEX,
    NAV_INDEX,
    CHAPTER_INDEX,
    PAGE_CHUNK_INDEX,
    PAGE_TRANSITION_INDEX,
    PAGE_DATA,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Section {
    pub id: u16,
    pub offset: u64,
    pub length: u64,
    pub entry_size: u32,
    pub record_count: u32,
}

impl Section {
    const EMPTY: Self = Self {
        id: 0,
        offset: 0,
        length: 0,
        entry_size: 0,
        record_count: 0,
    };
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SectionDirectory {
    entries: [Section; 19],
}

impl SectionDirectory {
    pub(crate) fn parse(table: &[u8], header: Header) -> Result<Self, FormatError> {
        let mut entries = [Section::EMPTY; 19];
        for index in 0..usize::from(header.section_count) {
            let start = index
                .checked_mul(ENTRY_SIZE)
                .ok_or(FormatError::InvalidSection)?;
            let bytes = table
                .get(start..start + ENTRY_SIZE)
                .ok_or(FormatError::InvalidSection)?;
            let section = Section {
                id: read_u16(bytes, 0)?,
                offset: read_u64(bytes, 4)?,
                length: read_u64(bytes, 12)?,
                entry_size: read_u32(bytes, 20)?,
                record_count: read_u32(bytes, 24)?,
            };
            let end = section
                .offset
                .checked_add(section.length)
                .ok_or(FormatError::FileOutOfBounds)?;
            if end > header.file_length {
                return Err(FormatError::FileOutOfBounds);
            }
            if section.entry_size != 0 {
                let records_length = u64::from(section.entry_size)
                    .checked_mul(u64::from(section.record_count))
                    .ok_or(FormatError::InvalidSection)?;
                if records_length > section.length {
                    return Err(FormatError::InvalidSection);
                }
            }
            if let Some(slot) = required_slot(section.id) {
                if entries[slot].id != 0 {
                    return Err(FormatError::DuplicateSection(section.id));
                }
                entries[slot] = section;
            }
        }
        for (slot, required) in REQUIRED.iter().copied().enumerate() {
            if entries[slot].id != required {
                return Err(FormatError::MissingSection(required));
            }
        }
        validate_record_section(entries[12], 80)?;
        validate_record_section(entries[13], 128)?;
        validate_record_section(entries[14], 48)?;
        validate_record_section(entries[15], 32)?;
        validate_record_section(entries[16], 24)?;
        validate_record_section(entries[17], 24)?;
        let page_data = entries[18];
        if page_data.entry_size != 0
            || page_data.record_count != 0
            || page_data.offset != header.page_data_offset
            || page_data.length != header.page_data_length
        {
            return Err(FormatError::InvalidSection);
        }
        if entries[13].record_count == 0 {
            return Err(FormatError::InvalidPage);
        }
        Ok(Self { entries })
    }

    pub(crate) const fn get(self, id: u16) -> Section {
        match required_slot(id) {
            Some(slot) => self.entries[slot],
            None => Section::EMPTY,
        }
    }
}

fn validate_record_section(section: Section, expected: u32) -> Result<(), FormatError> {
    if section.entry_size == expected {
        Ok(())
    } else {
        Err(FormatError::InvalidSection)
    }
}

const fn required_slot(id: u16) -> Option<usize> {
    match id {
        STRING_TABLE => Some(0),
        DISPLAY_PROFILE => Some(1),
        LAYOUT_PROFILE => Some(2),
        READER_REQUIREMENTS => Some(3),
        SOURCE_IDENTITY => Some(4),
        BOOK_METADATA => Some(5),
        RENDITION_IDENTITY => Some(6),
        FONT_POLICY => Some(7),
        TYPOGRAPHY_POLICY => Some(8),
        IMAGE_POLICY => Some(9),
        COMPRESSION_POLICY => Some(10),
        CHROME_POLICY => Some(11),
        FONT_RESOURCE_INDEX => Some(12),
        PAGE_INDEX => Some(13),
        NAV_INDEX => Some(14),
        CHAPTER_INDEX => Some(15),
        PAGE_CHUNK_INDEX => Some(16),
        PAGE_TRANSITION_INDEX => Some(17),
        PAGE_DATA => Some(18),
        _ => None,
    }
}
