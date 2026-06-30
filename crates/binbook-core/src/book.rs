use crate::header::{self, Header};
use crate::section::{self, Section, SectionDirectory};
use crate::{
    ChapterNumber, ChunkRecordNumber, Error, FileOffset, FormatError, NavNumber, PageNumber,
    ReadAt, TransitionNumber,
};

pub struct Book<R: ReadAt> {
    pub(crate) source: R,
    pub(crate) header: Header,
    pub(crate) sections: SectionDirectory,
}

impl<R: ReadAt> Book<R> {
    pub fn open(mut source: R, section_table_scratch: &mut [u8]) -> Result<Self, Error<R::Error>> {
        require_buffer(section_table_scratch.len(), header::HEADER_SIZE)?;
        let source_length = source.len().map_err(Error::Source)?;
        source
            .read_exact_at(0, &mut section_table_scratch[..header::HEADER_SIZE])
            .map_err(Error::Source)?;
        let header = header::parse(&section_table_scratch[..header::HEADER_SIZE], source_length)?;
        let table_length = usize::from(header.section_count)
            .checked_mul(section::ENTRY_SIZE)
            .ok_or(FormatError::InvalidHeader)?;
        require_buffer(section_table_scratch.len(), table_length)?;
        source
            .read_exact_at(
                header.section_table_offset,
                &mut section_table_scratch[..table_length],
            )
            .map_err(Error::Source)?;
        let sections = SectionDirectory::parse(&section_table_scratch[..table_length], header)?;
        let mut book = Self {
            source,
            header,
            sections,
        };
        book.validate_string_references(section_table_scratch)?;
        Ok(book)
    }

    #[must_use]
    pub fn page_count(&self) -> u32 {
        self.sections.get(section::PAGE_INDEX).record_count
    }

    #[must_use]
    pub fn nav_count(&self) -> u32 {
        self.sections.get(section::NAV_INDEX).record_count
    }

    #[must_use]
    pub fn chapter_count(&self) -> u32 {
        self.sections.get(section::CHAPTER_INDEX).record_count
    }

    #[must_use]
    pub fn chunk_count(&self) -> u32 {
        self.sections.get(section::PAGE_CHUNK_INDEX).record_count
    }

    #[must_use]
    pub fn transition_count(&self) -> u32 {
        self.sections
            .get(section::PAGE_TRANSITION_INDEX)
            .record_count
    }

    #[must_use]
    pub const fn page_data_offset(&self) -> FileOffset {
        FileOffset::from_validated(self.header.page_data_offset)
    }

    pub const fn page_number(&self, raw: u32) -> Result<PageNumber, FormatError> {
        if raw < self.page_count_const() {
            Ok(PageNumber::from_validated(raw))
        } else {
            Err(FormatError::PageOutOfRange)
        }
    }

    pub const fn nav_number(&self, raw: u32) -> Result<NavNumber, FormatError> {
        if raw < self.nav_count_const() {
            Ok(NavNumber::from_validated(raw))
        } else {
            Err(FormatError::NavOutOfRange)
        }
    }

    pub const fn chapter_number(&self, raw: u32) -> Result<ChapterNumber, FormatError> {
        if raw < self.chapter_count_const() {
            Ok(ChapterNumber::from_validated(raw))
        } else {
            Err(FormatError::ChapterOutOfRange)
        }
    }

    pub const fn chunk_record_number(&self, raw: u32) -> Result<ChunkRecordNumber, FormatError> {
        if raw < self.chunk_count_const() {
            Ok(ChunkRecordNumber::from_validated(raw))
        } else {
            Err(FormatError::ChunkOutOfRange)
        }
    }

    pub const fn transition_number(&self, raw: u32) -> Result<TransitionNumber, FormatError> {
        if raw < self.transition_count_const() {
            Ok(TransitionNumber::from_validated(raw))
        } else {
            Err(FormatError::TransitionOutOfRange)
        }
    }

    const fn page_count_const(&self) -> u32 {
        self.sections.get(section::PAGE_INDEX).record_count
    }

    const fn nav_count_const(&self) -> u32 {
        self.sections.get(section::NAV_INDEX).record_count
    }

    const fn chapter_count_const(&self) -> u32 {
        self.sections.get(section::CHAPTER_INDEX).record_count
    }

    const fn chunk_count_const(&self) -> u32 {
        self.sections.get(section::PAGE_CHUNK_INDEX).record_count
    }

    const fn transition_count_const(&self) -> u32 {
        self.sections
            .get(section::PAGE_TRANSITION_INDEX)
            .record_count
    }

    pub(crate) const fn string_table(&self) -> Section {
        self.sections.get(section::STRING_TABLE)
    }
}

pub(crate) fn require_buffer<E>(provided: usize, required: usize) -> Result<(), Error<E>> {
    if provided < required {
        Err(Error::BufferTooSmall { required, provided })
    } else {
        Ok(())
    }
}
