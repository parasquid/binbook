use crate::book::Book;
use crate::chunk;
use crate::navigation;
use crate::page;
use crate::profile;
use crate::rendition_identity;
use crate::section;
use crate::transition;
use crate::{
    BookMetadata, ChapterEntry, ChapterNumber, ChunkRecordNumber, DisplayProfile, Error, NavEntry,
    NavNumber, PageChunk, PageInfo, PageNumber, PageTransition, ReadAt, RenditionIdentity,
    TransitionNumber,
};

impl<R: ReadAt> Book<R> {
    pub fn rendition_identity(
        &mut self,
        record: &mut [u8],
    ) -> Result<RenditionIdentity, Error<R::Error>> {
        let section = self.sections.get(section::RENDITION_IDENTITY);
        if section.length != rendition_identity::RENDITION_IDENTITY_LENGTH {
            return Err(crate::FormatError::InvalidSection.into());
        }
        self.read_prefix(section, rendition_identity::RENDITION_HASH_LENGTH, record)?;
        Ok(rendition_identity::parse(
            &record[..rendition_identity::RENDITION_HASH_LENGTH],
        )?)
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
}
