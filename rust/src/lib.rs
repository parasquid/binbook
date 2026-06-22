#![cfg_attr(not(feature = "std"), no_std)]

pub mod chapter_index;
pub mod decompress;
pub mod error;
mod header;
#[cfg(feature = "lz4")]
mod lz4_decompress;
pub mod nav_index;
pub mod page_index;
pub mod reader;
mod rle;
pub mod section;
pub mod string_table;

pub use decompress::decompress_page;
pub use error::Error;
pub use nav_index::NavEntry;
pub use page_index::PageInfo;
pub use reader::Reader;
pub use chapter_index::ChapterEntry;

pub struct Info<'a> {
    pub title: &'a [u8],
    pub subtitle: &'a [u8],
    pub author: &'a [u8],
    pub publisher: &'a [u8],
    pub language: &'a [u8],
    pub series_name: &'a [u8],
    pub series_index_milli: u32,
    pub page_count: u32,
    pub nav_count: u32,
    pub chapter_count: u32,
    pub logical_width: u16,
    pub logical_height: u16,
    pub physical_width: u16,
    pub physical_height: u16,
    pub logical_to_physical_rotation: u16,
    pub default_pixel_format: u16,
    pub compression: u16,
}

pub struct PageRef<'a> {
    pub info: PageInfo,
    compressed_data: &'a [u8],
    pub uncompressed_size: usize,
}

pub struct BinBook<R: Reader, S: AsRef<[u8]> + AsMut<[u8]>> {
    reader: R,
    scratch: S,
    string_table_offset: u64,
    string_table_length: u32,
    page_index_offset: u64,
    page_index_entry_size: u16,
    nav_index_offset: u64,
    nav_index_entry_size: u16,
    chapter_index_offset: u64,
    chapter_index_entry_size: u16,
    page_data_offset: u64,
    page_count: u32,
    nav_count: u32,
    chapter_count: u32,
}

pub struct OpenInfo {
    pub string_table_offset: u64,
    pub string_table_length: u32,
    pub page_index_offset: u64,
    pub page_index_entry_size: u16,
    pub nav_index_offset: u64,
    pub nav_index_entry_size: u16,
    pub chapter_index_offset: u64,
    pub chapter_index_entry_size: u16,
    pub page_data_offset: u64,
    pub page_count: u32,
    pub nav_count: u32,
    pub chapter_count: u32,
}

const MIN_SCRATCH: usize = 256;

impl<R: Reader, S: AsRef<[u8]> + AsMut<[u8]>> BinBook<R, S> {
    pub fn open(mut reader: R, mut scratch: S) -> Result<Self, Error> {
        if scratch.as_ref().len() < MIN_SCRATCH {
            return Err(Error::InvalidHeader);
        }
        reader.read_at(0, &mut scratch.as_mut()[..256])?;
        let hdr = header::parse_header(scratch.as_ref())?;

        let table_bytes = hdr.section_count as usize * section::SECTION_ENTRY_SIZE;
        if scratch.as_ref().len() < table_bytes {
            return Err(Error::InvalidHeader);
        }
        reader.read_at(hdr.section_table_offset, &mut scratch.as_mut()[..table_bytes])?;
        let sections = section::parse_sections_from_table(
            scratch.as_ref(),
            hdr.section_count,
            hdr.page_data_offset,
            hdr.page_data_length,
        )?;

        Ok(Self {
            reader,
            scratch,
            string_table_offset: sections.string_table.offset,
            string_table_length: sections.string_table.length as u32,
            page_index_offset: sections.page_index.offset,
            page_index_entry_size: sections.page_index.entry_size as u16,
            nav_index_offset: sections.nav_index.offset,
            nav_index_entry_size: sections.nav_index.entry_size as u16,
            chapter_index_offset: sections.chapter_index.offset,
            chapter_index_entry_size: sections.chapter_index.entry_size as u16,
            page_data_offset: sections.page_data.offset,
            page_count: sections.page_index.record_count,
            nav_count: sections.nav_index.record_count,
            chapter_count: sections.chapter_index.record_count,
        })
    }

    pub fn page_count(&self) -> u32 {
        self.page_count
    }

    pub fn nav_count(&self) -> u32 {
        self.nav_count
    }

    pub fn chapter_count(&self) -> u32 {
        self.chapter_count
    }

    pub fn open_info(&self) -> OpenInfo {
        OpenInfo {
            string_table_offset: self.string_table_offset,
            string_table_length: self.string_table_length,
            page_index_offset: self.page_index_offset,
            page_index_entry_size: self.page_index_entry_size,
            nav_index_offset: self.nav_index_offset,
            nav_index_entry_size: self.nav_index_entry_size,
            chapter_index_offset: self.chapter_index_offset,
            chapter_index_entry_size: self.chapter_index_entry_size,
            page_data_offset: self.page_data_offset,
            page_count: self.page_count,
            nav_count: self.nav_count,
            chapter_count: self.chapter_count,
        }
    }

    pub fn info(&self) -> Result<Info<'_>, Error> {
        Ok(Info {
            title: b"",
            subtitle: b"",
            author: b"",
            publisher: b"",
            language: b"",
            series_name: b"",
            series_index_milli: 0,
            page_count: self.page_count,
            nav_count: self.nav_count,
            chapter_count: self.chapter_count,
            logical_width: 0,
            logical_height: 0,
            physical_width: 0,
            physical_height: 0,
            logical_to_physical_rotation: 0,
            default_pixel_format: 0,
            compression: 0,
        })
    }

    pub fn page_info(&mut self, index: u32) -> Result<PageInfo, Error> {
        if index >= self.page_count {
            return Err(Error::PageOutOfRange);
        }
        let buf = self.scratch.as_mut();
        if buf.len() < page_index::PAGE_INDEX_ENTRY_SIZE {
            return Err(Error::InvalidPageIndex);
        }
        let off = self.page_index_offset + index as u64 * self.page_index_entry_size as u64;
        self.reader.read_at(off, &mut buf[..page_index::PAGE_INDEX_ENTRY_SIZE])?;
        page_index::parse_page_info_from_bytes(buf, self.page_data_offset)
    }

    pub fn page(&mut self, index: u32) -> Result<PageRef<'_>, Error> {
        let info = self.page_info(index)?;
        let uncompressed_size = info.uncompressed_size as usize;
        let buf = self.scratch.as_mut();
        let compressed_size = info.compressed_size as usize;
        if buf.len() < compressed_size {
            return Err(Error::OutputBufferTooSmall);
        }
        self.reader.read_at(info.blob_offset, &mut buf[..compressed_size])?;
        Ok(PageRef {
            info,
            compressed_data: &buf[..compressed_size],
            uncompressed_size,
        })
    }

    pub fn decompress_page(&mut self, index: u32, out: &mut [u8]) -> Result<(), Error> {
        let info = self.page_info(index)?;
        let uncompressed_size = info.uncompressed_size as usize;
        if out.len() < uncompressed_size {
            return Err(Error::OutputBufferTooSmall);
        }
        let buf = self.scratch.as_mut();
        let compressed_size = info.compressed_size as usize;
        if buf.len() < compressed_size {
            return Err(Error::OutputBufferTooSmall);
        }
        self.reader.read_at(info.blob_offset, &mut buf[..compressed_size])?;
        decompress::decompress_bytes(info.compression_method, &buf[..compressed_size], out, uncompressed_size)
    }

    pub fn nav_entry(&mut self, index: u32) -> Result<NavEntry<'_>, Error> {
        if index >= self.nav_count {
            return Err(Error::NavOutOfRange);
        }
        let buf = self.scratch.as_mut();
        if buf.len() < section::NAV_INDEX_ENTRY_SIZE {
            return Err(Error::InvalidNavIndex);
        }
        let off = self.nav_index_offset + index as u64 * self.nav_index_entry_size as u64;
        self.reader.read_at(off, &mut buf[..section::NAV_INDEX_ENTRY_SIZE])?;
        let title_offset = header::read_le32(buf, 8);
        let title_len = header::read_le32(buf, 12);
        Ok(nav_index::parse_nav_entry_from_bytes(buf, title_offset, title_len))
    }

    pub fn chapter(&mut self, index: u32) -> Result<ChapterEntry<'_>, Error> {
        if index >= self.chapter_count {
            return Err(Error::ChapterOutOfRange);
        }
        let buf = self.scratch.as_mut();
        if buf.len() < section::CHAPTER_INDEX_ENTRY_SIZE {
            return Err(Error::InvalidChapterIndex);
        }
        let off = self.chapter_index_offset + index as u64 * self.chapter_index_entry_size as u64;
        self.reader.read_at(off, &mut buf[..section::CHAPTER_INDEX_ENTRY_SIZE])?;
        let title_offset = header::read_le32(buf, 8);
        let title_len = header::read_le32(buf, 12);
        Ok(chapter_index::parse_chapter_entry_from_bytes(
            buf,
            index,
            title_offset,
            title_len,
            self.page_count,
        )?)
    }
}
