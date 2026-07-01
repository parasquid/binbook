use crate::book::Book;
use crate::header::{read_u16, read_u64, HEADER_SIZE};
use crate::record_validation::validate_records;
use crate::section::{self, Section};
use crate::validation_crc::crc_range;
use crate::{Error, FormatError, ReadAt};

const SUPPORTED_READER_FEATURES: u64 = (1 << 11) - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationCode {
    Format,
    Bounds,
    Ordering,
    ReservedBytes,
    SectionCrc,
    PageCrc,
    RequiredFeatures,
    Profile,
    StringReference,
    Plane,
    Chunk,
    Transition,
    Navigation,
    Chapter,
    FontResource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationIssue {
    pub code: ValidationCode,
    pub section_id: Option<u16>,
    pub record_index: Option<u32>,
}

pub trait ValidationVisitor {
    fn visit(&mut self, issue: ValidationIssue);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError<E> {
    Source(E),
    Format(FormatError),
    BufferTooSmall { required: usize, provided: usize },
}

pub fn validate_all<R: ReadAt, V: ValidationVisitor>(
    mut source: R,
    section_scratch: &mut [u8],
    record_scratch: &mut [u8],
    visitor: &mut V,
) -> Result<(), ValidationError<R::Error>> {
    if !preflight(&mut source, section_scratch, visitor)? {
        return Ok(());
    }
    let mut book = match Book::open(source, section_scratch) {
        Ok(book) => book,
        Err(Error::Source(error)) => return Err(ValidationError::Source(error)),
        Err(Error::BufferTooSmall { required, provided }) => {
            return Err(ValidationError::BufferTooSmall { required, provided });
        }
        Err(Error::Format(error)) => {
            visit(visitor, code_for_format(error), None, None);
            return Ok(());
        }
    };
    validate_sections(&mut book, record_scratch, visitor)?;
    validate_requirements(&mut book, record_scratch, visitor)?;
    validate_records(&mut book, record_scratch, visitor)?;
    Ok(())
}

fn preflight<R: ReadAt, V: ValidationVisitor>(
    source: &mut R,
    scratch: &mut [u8],
    visitor: &mut V,
) -> Result<bool, ValidationError<R::Error>> {
    require(scratch, HEADER_SIZE)?;
    let source_len = source.len().map_err(ValidationError::Source)?;
    source
        .read_exact_at(0, &mut scratch[..HEADER_SIZE])
        .map_err(ValidationError::Source)?;
    let header = &scratch[..HEADER_SIZE];
    if header[8..12].iter().any(|byte| *byte != 0) || header[68..].iter().any(|byte| *byte != 0) {
        visit(visitor, ValidationCode::ReservedBytes, None, None);
    }
    let declared = read_u64(header, 16).unwrap_or(u64::MAX);
    if declared > source_len {
        visit(visitor, ValidationCode::Bounds, None, None);
        return Ok(false);
    }
    let table_offset = read_u64(header, 24).unwrap_or(u64::MAX);
    let count = usize::from(read_u16(header, 38).unwrap_or(0));
    let table_len = count.saturating_mul(section::ENTRY_SIZE);
    let table_end = table_offset.saturating_add(table_len as u64);
    if table_end > source_len {
        visit(visitor, ValidationCode::Bounds, None, None);
        return Ok(false);
    }
    require(scratch, table_len)?;
    source
        .read_exact_at(table_offset, &mut scratch[..table_len])
        .map_err(ValidationError::Source)?;
    let mut previous = None;
    for record in scratch[..table_len].chunks_exact(section::ENTRY_SIZE) {
        let id = read_u16(record, 0).unwrap_or(0);
        if previous.is_some_and(|value| id <= value) {
            visit(visitor, ValidationCode::Ordering, Some(id), None);
        }
        if read_u16(record, 2).unwrap_or(1) != 0 || record[32..40].iter().any(|b| *b != 0) {
            visit(visitor, ValidationCode::ReservedBytes, Some(id), None);
        }
        previous = Some(id);
    }
    Ok(true)
}

fn validate_sections<R: ReadAt, V: ValidationVisitor>(
    book: &mut Book<R>,
    scratch: &mut [u8],
    visitor: &mut V,
) -> Result<(), ValidationError<R::Error>> {
    for section in book.sections.entries() {
        if section.crc32 == 0 {
            continue;
        }
        let actual = crc_range(&mut book.source, section.offset, section.length, scratch)?;
        if actual != section.crc32 {
            visit(visitor, ValidationCode::SectionCrc, Some(section.id), None);
        }
    }
    Ok(())
}

fn validate_requirements<R: ReadAt, V: ValidationVisitor>(
    book: &mut Book<R>,
    scratch: &mut [u8],
    visitor: &mut V,
) -> Result<(), ValidationError<R::Error>> {
    let section = book.sections.get(section::READER_REQUIREMENTS);
    if read_section_prefix(book, section, 16, scratch)?
        .get(8..16)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_le_bytes)
        .unwrap_or(u64::MAX)
        & !SUPPORTED_READER_FEATURES
        != 0
    {
        visit(
            visitor,
            ValidationCode::RequiredFeatures,
            Some(section.id),
            None,
        );
    }
    Ok(())
}

fn read_section_prefix<'a, R: ReadAt>(
    book: &mut Book<R>,
    section: Section,
    required: usize,
    scratch: &'a mut [u8],
) -> Result<&'a [u8], ValidationError<R::Error>> {
    require(scratch, required)?;
    book.source
        .read_exact_at(section.offset, &mut scratch[..required])
        .map_err(ValidationError::Source)?;
    Ok(&scratch[..required])
}

fn code_for_format(error: FormatError) -> ValidationCode {
    match error {
        FormatError::FileOutOfBounds => ValidationCode::Bounds,
        FormatError::InvalidDisplayProfile | FormatError::UnsupportedWaveformHint(_) => {
            ValidationCode::Profile
        }
        FormatError::InvalidStringRef => ValidationCode::StringReference,
        FormatError::InvalidNavigation | FormatError::NavOutOfRange => ValidationCode::Navigation,
        FormatError::InvalidChapter | FormatError::ChapterOutOfRange => ValidationCode::Chapter,
        FormatError::InvalidChunk | FormatError::ChunkOutOfRange => ValidationCode::Chunk,
        FormatError::InvalidTransition | FormatError::TransitionOutOfRange => {
            ValidationCode::Transition
        }
        FormatError::InvalidFontResource => ValidationCode::FontResource,
        FormatError::InvalidPage
        | FormatError::PlaneSlotOutOfRange
        | FormatError::UnsupportedPixelFormat(_)
        | FormatError::UnsupportedCompression(_) => ValidationCode::Plane,
        _ => ValidationCode::Format,
    }
}

pub(crate) fn visit<V: ValidationVisitor>(
    visitor: &mut V,
    code: ValidationCode,
    section_id: Option<u16>,
    record_index: Option<u32>,
) {
    visitor.visit(ValidationIssue {
        code,
        section_id,
        record_index,
    });
}
pub(crate) fn require<E>(scratch: &[u8], required: usize) -> Result<(), ValidationError<E>> {
    if scratch.len() < required {
        Err(ValidationError::BufferTooSmall {
            required,
            provided: scratch.len(),
        })
    } else {
        Ok(())
    }
}
pub(crate) fn map_book_error<E>(error: Error<E>) -> ValidationError<E> {
    match error {
        Error::Source(error) => ValidationError::Source(error),
        Error::BufferTooSmall { required, provided } => {
            ValidationError::BufferTooSmall { required, provided }
        }
        Error::Format(error) => ValidationError::Format(error),
    }
}
