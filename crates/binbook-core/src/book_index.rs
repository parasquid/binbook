use crate::{
    chunk, header, page, profile, rendition_identity, section, DisplayProfile, FormatError,
    PageChunk, PageInfo, RenditionIdentity,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadRange {
    offset: u64,
    length: usize,
}

impl ReadRange {
    #[must_use]
    pub const fn from_parts(offset: u64, length: usize) -> Self {
        Self { offset, length }
    }

    #[must_use]
    pub const fn offset(self) -> u64 {
        self.offset
    }

    #[must_use]
    pub const fn length(self) -> usize {
        self.length
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BookIndexHeader {
    header: header::Header,
}

impl BookIndexHeader {
    #[must_use]
    pub fn section_table_range(self) -> ReadRange {
        ReadRange {
            offset: self.header.section_table_offset,
            length: usize::from(self.header.section_count) * section::ENTRY_SIZE,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BookIndex {
    header: header::Header,
    sections: section::SectionDirectory,
}

impl BookIndex {
    pub fn parse_header(bytes: &[u8], source_length: u64) -> Result<BookIndexHeader, FormatError> {
        header::parse(bytes, source_length).map(|header| BookIndexHeader { header })
    }

    pub fn parse(header: BookIndexHeader, table: &[u8]) -> Result<Self, FormatError> {
        let expected = header.section_table_range().length;
        if table.len() < expected {
            return Err(FormatError::InvalidSection);
        }
        let sections = section::SectionDirectory::parse(&table[..expected], header.header)?;
        Ok(Self {
            header: header.header,
            sections,
        })
    }

    #[must_use]
    pub const fn page_count(self) -> u32 {
        self.sections.get(section::PAGE_INDEX).record_count
    }

    #[must_use]
    pub const fn chunk_count(self) -> u32 {
        self.sections.get(section::PAGE_CHUNK_INDEX).record_count
    }

    pub fn page_record_range(self, page: u32) -> Result<ReadRange, FormatError> {
        self.record_range(section::PAGE_INDEX, page, FormatError::PageOutOfRange)
    }

    pub fn chunk_record_range(self, chunk: u32) -> Result<ReadRange, FormatError> {
        self.record_range(
            section::PAGE_CHUNK_INDEX,
            chunk,
            FormatError::ChunkOutOfRange,
        )
    }

    #[must_use]
    pub fn rendition_identity_range(self) -> ReadRange {
        let rendition = self.sections.get(section::RENDITION_IDENTITY);
        ReadRange {
            offset: rendition.offset,
            length: rendition_identity::RENDITION_HASH_LENGTH,
        }
    }

    #[must_use]
    pub fn display_profile_range(self) -> ReadRange {
        let profile = self.sections.get(section::DISPLAY_PROFILE);
        ReadRange {
            offset: profile.offset,
            length: profile::DISPLAY_PROFILE_MIN,
        }
    }

    pub fn parse_display_profile(self, bytes: &[u8]) -> Result<DisplayProfile, FormatError> {
        if bytes.len() < profile::DISPLAY_PROFILE_MIN {
            return Err(FormatError::InvalidDisplayProfile);
        }
        DisplayProfile::parse(
            &bytes[..profile::DISPLAY_PROFILE_MIN],
            self.sections.get(section::STRING_TABLE).length,
        )
    }

    pub fn parse_page(self, expected: u32, bytes: &[u8]) -> Result<PageInfo, FormatError> {
        if expected >= self.page_count() {
            return Err(FormatError::PageOutOfRange);
        }
        page::parse(
            bytes,
            crate::PageNumber::from_validated(expected),
            self.header.page_data_length,
        )
    }

    pub fn parse_chunk(self, bytes: &[u8]) -> Result<PageChunk, FormatError> {
        chunk::parse(bytes, self.page_count(), self.header.page_data_length)
    }

    pub fn page_data_range(
        self,
        relative_offset: u64,
        length: u32,
    ) -> Result<ReadRange, FormatError> {
        let end = relative_offset
            .checked_add(u64::from(length))
            .ok_or(FormatError::InvalidPage)?;
        if end > self.header.page_data_length {
            return Err(FormatError::InvalidPage);
        }
        Ok(ReadRange {
            offset: self
                .header
                .page_data_offset
                .checked_add(relative_offset)
                .ok_or(FormatError::InvalidPage)?,
            length: usize::try_from(length).map_err(|_| FormatError::InvalidPage)?,
        })
    }

    pub fn parse_rendition_identity(self, bytes: &[u8]) -> Result<RenditionIdentity, FormatError> {
        let section = self.sections.get(section::RENDITION_IDENTITY);
        if section.length != rendition_identity::RENDITION_IDENTITY_LENGTH
            || bytes.len() < rendition_identity::RENDITION_HASH_LENGTH
        {
            return Err(FormatError::InvalidSection);
        }
        rendition_identity::parse(&bytes[..rendition_identity::RENDITION_HASH_LENGTH])
    }

    fn record_range(
        self,
        section_id: u16,
        index: u32,
        out_of_range: FormatError,
    ) -> Result<ReadRange, FormatError> {
        let section = self.sections.get(section_id);
        if index >= section.record_count {
            return Err(out_of_range);
        }
        let relative = u64::from(index)
            .checked_mul(u64::from(section.entry_size))
            .ok_or(FormatError::InvalidSection)?;
        let offset = section
            .offset
            .checked_add(relative)
            .ok_or(FormatError::InvalidSection)?;
        let length =
            usize::try_from(section.entry_size).map_err(|_| FormatError::InvalidSection)?;
        Ok(ReadRange { offset, length })
    }
}
