use core::cmp::Ordering;

use crate::book::Book;
use crate::font_resource::FontResourceIndexEntry;
use crate::header::{read_u16, read_u32};
use crate::section;
use crate::validate::{map_book_error, visit};
use crate::{ReadAt, StringRef, ValidationCode, ValidationError, ValidationVisitor};

pub(crate) fn validate_links<R: ReadAt, V: ValidationVisitor>(
    book: &mut Book<R>,
    scratch: &mut [u8],
    visitor: &mut V,
) -> Result<(), ValidationError<R::Error>> {
    validate_navigation(book, scratch, visitor)?;
    validate_chapters(book, scratch, visitor)?;
    validate_fonts(book, scratch, visitor)
}

fn validate_navigation<R: ReadAt, V: ValidationVisitor>(
    book: &mut Book<R>,
    scratch: &mut [u8],
    visitor: &mut V,
) -> Result<(), ValidationError<R::Error>> {
    let nav = book.sections.get(section::NAV_INDEX);
    for raw in 0..book.nav_count() {
        book.read_record(nav, raw, scratch)
            .map_err(map_book_error)?;
        let invalid_link = [32, 36, 40]
            .into_iter()
            .map(|offset| read_u32(scratch, offset).unwrap_or(u32::MAX))
            .any(|value| value != u32::MAX && value >= book.nav_count());
        if invalid_link || read_u32(scratch, 44).unwrap_or(1) != 0 {
            visit(visitor, ValidationCode::Navigation, Some(nav.id), Some(raw));
        }
    }
    Ok(())
}

fn validate_chapters<R: ReadAt, V: ValidationVisitor>(
    book: &mut Book<R>,
    scratch: &mut [u8],
    visitor: &mut V,
) -> Result<(), ValidationError<R::Error>> {
    let chapters = book.sections.get(section::CHAPTER_INDEX);
    let nav = book.sections.get(section::NAV_INDEX);
    for raw in 0..book.chapter_count() {
        book.read_record(chapters, raw, scratch)
            .map_err(map_book_error)?;
        let nav_index = read_u32(scratch, 4).unwrap_or(u32::MAX);
        let chapter = [
            read_u32(scratch, 8).unwrap_or(u32::MAX),
            read_u32(scratch, 12).unwrap_or(u32::MAX),
            read_u32(scratch, 16).unwrap_or(u32::MAX),
            u32::from(read_u16(scratch, 20).unwrap_or(u16::MAX)),
            u32::from(read_u16(scratch, 22).unwrap_or(u16::MAX)),
        ];
        if nav_index >= book.nav_count() {
            continue;
        }
        book.read_record(nav, nav_index, scratch)
            .map_err(map_book_error)?;
        let nav_value = [
            read_u32(scratch, 8).unwrap_or(u32::MAX),
            read_u32(scratch, 12).unwrap_or(u32::MAX),
            read_u32(scratch, 28).unwrap_or(u32::MAX),
            u32::from(read_u16(scratch, 6).unwrap_or(u16::MAX)),
            u32::from(read_u16(scratch, 4).unwrap_or(u16::MAX)),
        ];
        if chapter != nav_value {
            visit(
                visitor,
                ValidationCode::Chapter,
                Some(chapters.id),
                Some(raw),
            );
        }
    }
    Ok(())
}

fn validate_fonts<R: ReadAt, V: ValidationVisitor>(
    book: &mut Book<R>,
    scratch: &mut [u8],
    visitor: &mut V,
) -> Result<(), ValidationError<R::Error>> {
    let fonts = book.sections.get(section::FONT_RESOURCE_INDEX);
    let mut previous: Option<(StringRef, u32)> = None;
    for raw in 0..fonts.record_count {
        book.read_record(fonts, raw, scratch)
            .map_err(map_book_error)?;
        match FontResourceIndexEntry::parse(scratch, raw, book.string_table().length) {
            Ok(font) => {
                if let Some((path, face)) = previous {
                    let order = compare_strings(book, path, font.source_path)?;
                    if order == Ordering::Greater
                        || (order == Ordering::Equal && face >= font.face_index)
                    {
                        visit(visitor, ValidationCode::Ordering, Some(fonts.id), Some(raw));
                    }
                }
                previous = Some((font.source_path, font.face_index));
            }
            Err(_) => visit(
                visitor,
                ValidationCode::FontResource,
                Some(fonts.id),
                Some(raw),
            ),
        }
    }
    Ok(())
}

fn compare_strings<R: ReadAt>(
    book: &mut Book<R>,
    left: StringRef,
    right: StringRef,
) -> Result<Ordering, ValidationError<R::Error>> {
    let table = book.string_table();
    let mut left_byte = [0_u8; 1];
    let mut right_byte = [0_u8; 1];
    for index in 0..left.length.min(right.length) {
        book.source
            .read_exact_at(
                table.offset + u64::from(left.offset + index),
                &mut left_byte,
            )
            .map_err(ValidationError::Source)?;
        book.source
            .read_exact_at(
                table.offset + u64::from(right.offset + index),
                &mut right_byte,
            )
            .map_err(ValidationError::Source)?;
        match left_byte.cmp(&right_byte) {
            Ordering::Equal => {}
            order => return Ok(order),
        }
    }
    Ok(left.length.cmp(&right.length))
}
