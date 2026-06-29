use crate::book::{require_buffer, Book};
use crate::section::{self, Section};
use crate::{ChapterNumber, Error, FormatError, NavNumber, ReadAt};

impl<R: ReadAt> Book<R> {
    pub(crate) fn read_prefix(
        &mut self,
        section: Section,
        required: usize,
        out: &mut [u8],
    ) -> Result<(), Error<R::Error>> {
        require_buffer(out.len(), required)?;
        if section.length < u64::try_from(required).map_err(|_| FormatError::InvalidSection)? {
            return Err(FormatError::InvalidSection.into());
        }
        self.source
            .read_exact_at(section.offset, &mut out[..required])
            .map_err(Error::Source)
    }

    pub(crate) fn read_record(
        &mut self,
        section: Section,
        index: u32,
        out: &mut [u8],
    ) -> Result<(), Error<R::Error>> {
        let required =
            usize::try_from(section.entry_size).map_err(|_| FormatError::InvalidSection)?;
        require_buffer(out.len(), required)?;
        let relative = u64::from(index)
            .checked_mul(u64::from(section.entry_size))
            .ok_or(FormatError::InvalidSection)?;
        let offset = section
            .offset
            .checked_add(relative)
            .ok_or(FormatError::InvalidSection)?;
        self.source
            .read_exact_at(offset, &mut out[..required])
            .map_err(Error::Source)
    }

    pub(crate) fn read_page_data(
        &mut self,
        relative_offset: u64,
        length: u32,
        out: &mut [u8],
    ) -> Result<(), Error<R::Error>> {
        let required = usize::try_from(length).map_err(|_| FormatError::InvalidSection)?;
        require_buffer(out.len(), required)?;
        let page_data = self.sections.get(section::PAGE_DATA);
        let offset = page_data
            .offset
            .checked_add(relative_offset)
            .ok_or(FormatError::InvalidSection)?;
        self.source
            .read_exact_at(offset, &mut out[..required])
            .map_err(Error::Source)
    }

    pub(crate) fn validate_string_references(
        &mut self,
        scratch: &mut [u8],
    ) -> Result<(), Error<R::Error>> {
        self.display_profile(scratch)?;
        self.book_metadata(scratch)?;
        for raw in 0..self.nav_count() {
            self.nav(NavNumber::from_validated(raw), scratch)?;
        }
        for raw in 0..self.chapter_count() {
            self.chapter(ChapterNumber::from_validated(raw), scratch)?;
        }
        Ok(())
    }
}
