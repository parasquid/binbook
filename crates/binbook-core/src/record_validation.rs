use crate::book::Book;
use crate::header::read_u32;
use crate::link_validation::validate_links;
use crate::section;
use crate::validate::{map_book_error, require, visit};
use crate::validation_crc::Crc32;
use crate::{
    ChunkRecordNumber, PageNumber, PixelFormat, PlaneSlot, ReadAt, TransitionNumber,
    ValidationCode, ValidationError, ValidationVisitor,
};

pub(crate) fn validate_records<R: ReadAt, V: ValidationVisitor>(
    book: &mut Book<R>,
    scratch: &mut [u8],
    visitor: &mut V,
) -> Result<(), ValidationError<R::Error>> {
    require(scratch, 128)?;
    validate_pages(book, scratch, visitor)?;
    validate_chunks(book, scratch, visitor)?;
    validate_transitions(book, scratch, visitor)?;
    validate_links(book, scratch, visitor)
}

fn validate_pages<R: ReadAt, V: ValidationVisitor>(
    book: &mut Book<R>,
    scratch: &mut [u8],
    visitor: &mut V,
) -> Result<(), ValidationError<R::Error>> {
    let pages = book.sections.get(section::PAGE_INDEX);
    let mut previous_progress = 0;
    for raw in 0..book.page_count() {
        book.read_record(pages, raw, scratch)
            .map_err(map_book_error)?;
        let reserved = scratch[49..52]
            .iter()
            .chain(&scratch[84..128])
            .any(|byte| *byte != 0)
            || read_u32(scratch, 12).unwrap_or(u32::MAX) & !1 != 0;
        if reserved {
            visit(
                visitor,
                ValidationCode::ReservedBytes,
                Some(pages.id),
                Some(raw),
            );
        }
        let page = match book.page(PageNumber::from_validated(raw), scratch) {
            Ok(page) => page,
            Err(_) => {
                visit(visitor, ValidationCode::Plane, Some(pages.id), Some(raw));
                continue;
            }
        };
        if page.progress_start_ppm < previous_progress || page.page_kind == 3 {
            visit(visitor, ValidationCode::Ordering, Some(pages.id), Some(raw));
        }
        previous_progress = page.progress_end_ppm;
        if invalid_plane_directory(page.pixel_format, page.planes) {
            visit(visitor, ValidationCode::Plane, Some(pages.id), Some(raw));
        }
        validate_page_crc(book, page, raw, scratch, visitor)?;
    }
    Ok(())
}

fn invalid_plane_directory(format: PixelFormat, planes: crate::PlaneDirectory) -> bool {
    let expected = match format {
        PixelFormat::Gray1Packed => 0x04,
        PixelFormat::Gray2Packed => 0x07,
        PixelFormat::Gray4Packed => 0x0f,
        PixelFormat::Rgb565 | PixelFormat::Rgb888 | PixelFormat::Rgba8888 => 0x01,
    };
    if planes.bitmap() != expected || planes.bitmap() & 0x08 != 0 {
        return true;
    }
    let mut ranges = [(0_u64, 0_u64); 4];
    for raw_slot in 0..4 {
        let slot = PlaneSlot::try_from(raw_slot).expect("wire plane slot");
        if let Some(plane) = planes.get(slot) {
            let start = plane.offset.get();
            let end = start + u64::from(plane.length.get());
            if start % 4 != 0 {
                return true;
            }
            for prior in &ranges[..usize::from(raw_slot)] {
                if prior.1 != 0 && start < prior.1 && prior.0 < end {
                    return true;
                }
            }
            ranges[usize::from(raw_slot)] = (start, end);
        }
    }
    false
}

fn validate_page_crc<R: ReadAt, V: ValidationVisitor>(
    book: &mut Book<R>,
    page: crate::PageInfo,
    raw: u32,
    scratch: &mut [u8],
    visitor: &mut V,
) -> Result<(), ValidationError<R::Error>> {
    if page.page_crc32 == 0 {
        return Ok(());
    }
    let mut crc = Crc32::new();
    for raw_slot in 0..4 {
        let slot = PlaneSlot::try_from(raw_slot).expect("wire plane slot");
        if let Some(plane) = page.planes.get(slot) {
            crc.update_range(
                &mut book.source,
                book.header.page_data_offset + plane.offset.get(),
                u64::from(plane.length.get()),
                scratch,
            )?;
        }
    }
    if crc.finish() != page.page_crc32 {
        visit(
            visitor,
            ValidationCode::PageCrc,
            Some(section::PAGE_INDEX),
            Some(raw),
        );
    }
    Ok(())
}

fn validate_chunks<R: ReadAt, V: ValidationVisitor>(
    book: &mut Book<R>,
    scratch: &mut [u8],
    visitor: &mut V,
) -> Result<(), ValidationError<R::Error>> {
    let section_info = book.sections.get(section::PAGE_CHUNK_INDEX);
    let mut previous = None;
    for raw in 0..book.chunk_count() {
        book.read_record(section_info, raw, scratch)
            .map_err(map_book_error)?;
        let key = (
            read_u32(scratch, 0).unwrap_or(u32::MAX),
            scratch[4],
            scratch[5],
        );
        let parsed = book.chunk(ChunkRecordNumber::from_validated(raw), scratch);
        let mut invalid = parsed.is_err() || previous.is_some_and(|value| key <= value);
        if let Ok(chunk) = parsed {
            let Ok(page) = book.page(chunk.page_number, scratch) else {
                visit(
                    visitor,
                    ValidationCode::Chunk,
                    Some(section_info.id),
                    Some(raw),
                );
                previous = Some(key);
                continue;
            };
            let parent = page.planes.get(chunk.plane_slot);
            let chunk_end = chunk.offset.get() + u64::from(chunk.compressed_length.get());
            invalid |= parent.is_none_or(|plane| {
                chunk.offset.get() < plane.offset.get()
                    || chunk_end > plane.offset.get() + u64::from(plane.length.get())
            });
            if page.stored_width == 800
                && page.stored_height == 480
                && page.pixel_format == PixelFormat::Gray2Packed
            {
                invalid |= chunk.row_count != 16
                    || chunk.uncompressed_length.get() != 1_600
                    || chunk.chunk_index.get() >= 30;
            }
        }
        if invalid {
            visit(
                visitor,
                ValidationCode::Chunk,
                Some(section_info.id),
                Some(raw),
            );
        }
        previous = Some(key);
    }
    Ok(())
}

fn validate_transitions<R: ReadAt, V: ValidationVisitor>(
    book: &mut Book<R>,
    scratch: &mut [u8],
    visitor: &mut V,
) -> Result<(), ValidationError<R::Error>> {
    let transitions = book.sections.get(section::PAGE_TRANSITION_INDEX);
    for raw in 0..book.transition_count() {
        let parsed = book.transition(TransitionNumber::from_validated(raw), scratch);
        let mut invalid = parsed.is_err();
        if let Ok(value) = parsed {
            let mask = value.changed_chunk_mask;
            if mask != 0 {
                let first = mask.trailing_zeros() as u16;
                let last = (31 - mask.leading_zeros()) as u16;
                invalid |= value.first_changed_chunk != first
                    || value.changed_chunk_count != last - first + 1;
            }
        }
        if invalid {
            visit(
                visitor,
                ValidationCode::Transition,
                Some(transitions.id),
                Some(raw),
            );
        }
    }
    Ok(())
}
