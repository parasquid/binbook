use crate::chunk::{self, PageChunk};
use crate::header::{self, Header};
use crate::navigation::{self, ChapterEntry, NavEntry};
use crate::page::{self, PageInfo, PlaneDescriptor};
use crate::profile::{self, BookMetadata, DisplayProfile};
use crate::section::{self, Section, SectionDirectory};
use crate::transition::{self, PageTransition};
use crate::{
    ChapterNumber, ChunkRecordNumber, Error, FileOffset, FormatError, NavNumber, PageNumber,
    ReadAt, StringRef, TransitionNumber,
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

    pub fn display_profile(
        &mut self,
        record: &mut [u8],
    ) -> Result<DisplayProfile, Error<R::Error>> {
        let section = self.sections.get(section::DISPLAY_PROFILE);
        self.read_prefix(section, profile::DISPLAY_PROFILE_MIN, record)?;
        Ok(DisplayProfile::parse(
            &record[..profile::DISPLAY_PROFILE_MIN],
            self.string_table().length,
        )?)
    }

    pub fn book_metadata(&mut self, record: &mut [u8]) -> Result<BookMetadata, Error<R::Error>> {
        let section = self.sections.get(section::BOOK_METADATA);
        self.read_prefix(section, profile::BOOK_METADATA_MIN, record)?;
        Ok(BookMetadata::parse(
            &record[..profile::BOOK_METADATA_MIN],
            self.string_table().length,
        )?)
    }

    pub fn page(
        &mut self,
        number: PageNumber,
        record: &mut [u8],
    ) -> Result<PageInfo, Error<R::Error>> {
        let section = self.sections.get(section::PAGE_INDEX);
        self.read_record(section, number.get(), record)?;
        Ok(page::parse(
            &record[..page::PAGE_RECORD_SIZE],
            number,
            self.header.page_data_length,
        )?)
    }

    pub fn nav(
        &mut self,
        number: NavNumber,
        record: &mut [u8],
    ) -> Result<NavEntry, Error<R::Error>> {
        let section = self.sections.get(section::NAV_INDEX);
        self.read_record(section, number.get(), record)?;
        Ok(navigation::parse_nav(
            &record[..navigation::NAV_RECORD_SIZE],
            number,
            self.page_count(),
            self.string_table().length,
        )?)
    }

    pub fn chapter(
        &mut self,
        number: ChapterNumber,
        record: &mut [u8],
    ) -> Result<ChapterEntry, Error<R::Error>> {
        let section = self.sections.get(section::CHAPTER_INDEX);
        self.read_record(section, number.get(), record)?;
        Ok(navigation::parse_chapter(
            &record[..navigation::CHAPTER_RECORD_SIZE],
            number,
            self.nav_count(),
            self.page_count(),
            self.string_table().length,
        )?)
    }

    pub fn chunk(
        &mut self,
        number: ChunkRecordNumber,
        record: &mut [u8],
    ) -> Result<PageChunk, Error<R::Error>> {
        let section = self.sections.get(section::PAGE_CHUNK_INDEX);
        self.read_record(section, number.get(), record)?;
        Ok(chunk::parse(
            &record[..chunk::CHUNK_RECORD_SIZE],
            self.page_count(),
            self.header.page_data_length,
        )?)
    }

    pub fn transition(
        &mut self,
        number: TransitionNumber,
        record: &mut [u8],
    ) -> Result<PageTransition, Error<R::Error>> {
        let section = self.sections.get(section::PAGE_TRANSITION_INDEX);
        self.read_record(section, number.get(), record)?;
        Ok(transition::parse(
            &record[..transition::TRANSITION_RECORD_SIZE],
            self.page_count(),
        )?)
    }

    pub fn read_string<'a>(
        &mut self,
        reference: StringRef,
        out: &'a mut [u8],
    ) -> Result<&'a [u8], Error<R::Error>> {
        let table = self.string_table();
        reference.validate(table.length)?;
        let required =
            usize::try_from(reference.length).map_err(|_| FormatError::InvalidStringRef)?;
        require_buffer(out.len(), required)?;
        let offset = table
            .offset
            .checked_add(u64::from(reference.offset))
            .ok_or(FormatError::InvalidStringRef)?;
        self.source
            .read_exact_at(offset, &mut out[..required])
            .map_err(Error::Source)?;
        Ok(&out[..required])
    }

    pub fn read_plane(
        &mut self,
        plane: PlaneDescriptor,
        out: &mut [u8],
    ) -> Result<(), Error<R::Error>> {
        self.read_page_data(plane.offset.get(), plane.length.get(), out)
    }

    pub fn read_plane_range(
        &mut self,
        plane: PlaneDescriptor,
        offset: u32,
        out: &mut [u8],
    ) -> Result<(), Error<R::Error>> {
        let requested = u32::try_from(out.len()).map_err(|_| FormatError::InvalidPage)?;
        let end = offset
            .checked_add(requested)
            .ok_or(FormatError::InvalidPage)?;
        if end > plane.length.get() {
            return Err(FormatError::InvalidPage.into());
        }
        let relative = plane
            .offset
            .get()
            .checked_add(u64::from(offset))
            .ok_or(FormatError::InvalidPage)?;
        self.read_page_data(relative, requested, out)
    }

    pub fn read_chunk(&mut self, chunk: PageChunk, out: &mut [u8]) -> Result<(), Error<R::Error>> {
        self.read_page_data(chunk.offset.get(), chunk.compressed_length.get(), out)
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

    const fn string_table(&self) -> Section {
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
