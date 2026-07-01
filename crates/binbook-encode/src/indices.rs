use binbook_core::{
    PageChunkIndexRecord, PageIndexRecord, PageTransitionIndexRecord, PlaneDirectoryRecord,
    WireEncode, PAGE_CHUNK_INDEX_RECORD_SIZE, PAGE_INDEX_RECORD_SIZE,
    PAGE_TRANSITION_INDEX_RECORD_SIZE,
};

use crate::hashing::{crc32, sha256};
use crate::model::{CompiledPage, ModelError};

pub(crate) struct PageSections {
    pub page_index: Vec<u8>,
    pub chunk_index: Vec<u8>,
    pub transition_index: Vec<u8>,
    pub page_data: Vec<u8>,
}

pub(crate) fn build_pages(pages: &[CompiledPage]) -> Result<PageSections, ModelError> {
    if pages.is_empty() {
        return Err(ModelError::NoPages);
    }
    let mut page_index = Vec::with_capacity(pages.len() * PAGE_INDEX_RECORD_SIZE);
    let mut chunk_index = Vec::new();
    let mut page_data = Vec::new();
    for (page_number, page) in pages.iter().enumerate() {
        validate_page(page)?;
        let mut directory = PlaneDirectoryRecord::default();
        let mut page_crc_bytes = Vec::new();
        for plane in &page.planes {
            align(&mut page_data, 4);
            let offset = u32::try_from(page_data.len()).map_err(|_| ModelError::LengthOverflow)?;
            let plane_start = page_data.len();
            for (chunk_number, chunk) in plane.chunks.iter().enumerate() {
                let chunk_offset =
                    u32::try_from(page_data.len()).map_err(|_| ModelError::LengthOverflow)?;
                page_data.extend_from_slice(&chunk.compressed);
                page_crc_bytes.extend_from_slice(&chunk.compressed);
                let record = PageChunkIndexRecord {
                    page_number: u32::try_from(page_number)
                        .map_err(|_| ModelError::TooManyRecords)?,
                    plane_slot: plane.slot,
                    chunk_index: u8::try_from(chunk_number)
                        .map_err(|_| ModelError::TooManyRecords)?,
                    row_start: chunk.row_start,
                    row_count: chunk.row_count,
                    page_data_offset: chunk_offset,
                    compressed_size: u32::try_from(chunk.compressed.len())
                        .map_err(|_| ModelError::LengthOverflow)?,
                    uncompressed_size: chunk.uncompressed_size,
                };
                encode_record(&record, PAGE_CHUNK_INDEX_RECORD_SIZE, &mut chunk_index)?;
            }
            let size = u32::try_from(page_data.len() - plane_start)
                .map_err(|_| ModelError::LengthOverflow)?;
            let slot = usize::from(plane.slot);
            directory.bitmap |= 1 << plane.slot;
            directory.compression[slot] = compression_code(plane.compression);
            directory.offsets[slot] = offset;
            directory.sizes[slot] = size;
        }
        let total = pages.len() as u64;
        let index = page_number as u64;
        let record = PageIndexRecord {
            page_number: u32::try_from(page_number).map_err(|_| ModelError::TooManyRecords)?,
            page_kind: page.page_kind,
            pixel_format: page.pixel_format,
            compression_method: compression_code(page.planes[0].compression).into(),
            update_hint: 0,
            page_flags: 1,
            page_crc32: crc32(&page_crc_bytes),
            stored_width: page.stored_width,
            stored_height: page.stored_height,
            placement_x: 0,
            placement_y: 0,
            source_spine_index: page.source_spine_index,
            chapter_nav_index: page.chapter_nav_index,
            progress_start_ppm: u32::try_from(index * 1_000_000 / total).unwrap_or(0),
            progress_end_ppm: u32::try_from((index + 1) * 1_000_000 / total).unwrap_or(1_000_000),
            plane_directory: directory,
        };
        encode_record(&record, PAGE_INDEX_RECORD_SIZE, &mut page_index)?;
    }
    let transition_index = build_transitions(pages)?;
    Ok(PageSections {
        page_index,
        chunk_index,
        transition_index,
        page_data,
    })
}

fn build_transitions(pages: &[CompiledPage]) -> Result<Vec<u8>, ModelError> {
    let mut output = Vec::new();
    for index in 0..pages.len().saturating_sub(1) {
        for (from, to) in [(index, index + 1), (index + 1, index)] {
            let (mask, first, count) = changed_chunks(&pages[from], &pages[to]);
            let record = PageTransitionIndexRecord {
                from_page_number: u32::try_from(from).map_err(|_| ModelError::TooManyRecords)?,
                to_page_number: u32::try_from(to).map_err(|_| ModelError::TooManyRecords)?,
                changed_chunk_mask: mask,
                first_changed_chunk: first,
                changed_chunk_count: count,
                flags: 0,
            };
            encode_record(&record, PAGE_TRANSITION_INDEX_RECORD_SIZE, &mut output)?;
        }
    }
    Ok(output)
}

fn changed_chunks(from: &CompiledPage, to: &CompiledPage) -> (u32, u16, u16) {
    let from_chunks = from
        .planes
        .iter()
        .find(|plane| plane.slot == 2)
        .map(|plane| plane.chunks.as_slice())
        .unwrap_or(&[]);
    let to_chunks = to
        .planes
        .iter()
        .find(|plane| plane.slot == 2)
        .map(|plane| plane.chunks.as_slice())
        .unwrap_or(&[]);
    let mut mask = 0_u32;
    for index in 0..from_chunks.len().min(to_chunks.len()).min(32) {
        if sha256(&from_chunks[index].compressed) != sha256(&to_chunks[index].compressed) {
            mask |= 1 << index;
        }
    }
    if mask == 0 {
        return (0, 0, 0);
    }
    let first = mask.trailing_zeros() as u16;
    let last = (31 - mask.leading_zeros()) as u16;
    (mask, first, last - first + 1)
}

fn validate_page(page: &CompiledPage) -> Result<(), ModelError> {
    if page.planes.is_empty() {
        return Err(ModelError::EmptyPlane);
    }
    let mut bitmap = 0_u8;
    for plane in &page.planes {
        if plane.slot >= 4 {
            return Err(ModelError::InvalidPlaneSlot);
        }
        if bitmap & (1 << plane.slot) != 0 {
            return Err(ModelError::DuplicatePlaneSlot);
        }
        if plane.chunks.is_empty() {
            return Err(ModelError::EmptyPlane);
        }
        if plane.chunks.iter().any(|chunk| chunk.compressed.is_empty()) {
            return Err(ModelError::EmptyChunk);
        }
        bitmap |= 1 << plane.slot;
    }
    Ok(())
}

fn compression_code(value: binbook_core::CompressionMethod) -> u8 {
    match value {
        binbook_core::CompressionMethod::None => 0,
        binbook_core::CompressionMethod::RlePackBits => 1,
        binbook_core::CompressionMethod::Lz4 => 2,
    }
}

fn encode_record(
    record: &impl WireEncode,
    size: usize,
    output: &mut Vec<u8>,
) -> Result<(), ModelError> {
    let start = output.len();
    output.resize(start + size, 0);
    record
        .encode_into(&mut output[start..])
        .map_err(|_| ModelError::LengthOverflow)
}

fn align(output: &mut Vec<u8>, alignment: usize) {
    output.resize(output.len().div_ceil(alignment) * alignment, 0);
}
