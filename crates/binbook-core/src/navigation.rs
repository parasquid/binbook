use crate::header::{read_u16, read_u32};
use crate::{ChapterNumber, FormatError, NavNumber, PageNumber, StringRef};

pub const NAV_RECORD_SIZE: usize = 48;
pub const CHAPTER_RECORD_SIZE: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavEntry {
    pub number: NavNumber,
    pub nav_type: u16,
    pub level: u16,
    pub title: StringRef,
    pub source_href: StringRef,
    pub page: PageNumber,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChapterEntry {
    pub number: ChapterNumber,
    pub nav: NavNumber,
    pub title: StringRef,
    pub page: PageNumber,
    pub level: u16,
    pub nav_type: u16,
}

pub(crate) fn parse_nav(
    bytes: &[u8],
    expected: NavNumber,
    page_count: u32,
    string_length: u64,
) -> Result<NavEntry, FormatError> {
    if bytes.len() < NAV_RECORD_SIZE || read_u32(bytes, 0)? != expected.get() {
        return Err(FormatError::InvalidNavigation);
    }
    let page = read_u32(bytes, 28)?;
    if page >= page_count {
        return Err(FormatError::InvalidNavigation);
    }
    let title = StringRef::parse(bytes, 8)?;
    let source_href = StringRef::parse(bytes, 16)?;
    title.validate(string_length)?;
    source_href.validate(string_length)?;
    Ok(NavEntry {
        number: expected,
        nav_type: read_u16(bytes, 4)?,
        level: read_u16(bytes, 6)?,
        title,
        source_href,
        page: PageNumber::from_validated(page),
    })
}

pub(crate) fn parse_chapter(
    bytes: &[u8],
    expected: ChapterNumber,
    nav_count: u32,
    page_count: u32,
    string_length: u64,
) -> Result<ChapterEntry, FormatError> {
    if bytes.len() < CHAPTER_RECORD_SIZE || read_u32(bytes, 0)? != expected.get() {
        return Err(FormatError::InvalidChapter);
    }
    let nav = read_u32(bytes, 4)?;
    let page = read_u32(bytes, 16)?;
    let nav_type = read_u16(bytes, 22)?;
    if nav >= nav_count || page >= page_count || !matches!(nav_type, 3 | 4) {
        return Err(FormatError::InvalidChapter);
    }
    let title = StringRef::parse(bytes, 8)?;
    title.validate(string_length)?;
    Ok(ChapterEntry {
        number: expected,
        nav: NavNumber::from_validated(nav),
        title,
        page: PageNumber::from_validated(page),
        level: read_u16(bytes, 20)?,
        nav_type,
    })
}
