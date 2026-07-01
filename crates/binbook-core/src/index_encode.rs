use crate::encode::{put_u16, put_u32, require, EncodeError, WireEncode};
use crate::StringRef;

pub const PAGE_INDEX_RECORD_SIZE: usize = 128;
pub const NAV_INDEX_RECORD_SIZE: usize = 48;
pub const CHAPTER_INDEX_RECORD_SIZE: usize = 32;
pub const PAGE_CHUNK_INDEX_RECORD_SIZE: usize = 24;
pub const PAGE_TRANSITION_INDEX_RECORD_SIZE: usize = 24;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PlaneDirectoryRecord {
    pub bitmap: u8,
    pub compression: [u8; 4],
    pub offsets: [u32; 4],
    pub sizes: [u32; 4],
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PageIndexRecord {
    pub page_number: u32,
    pub page_kind: u16,
    pub pixel_format: u16,
    pub compression_method: u16,
    pub update_hint: u16,
    pub page_flags: u32,
    pub page_crc32: u32,
    pub stored_width: u16,
    pub stored_height: u16,
    pub placement_x: u16,
    pub placement_y: u16,
    pub source_spine_index: u32,
    pub chapter_nav_index: u32,
    pub progress_start_ppm: u32,
    pub progress_end_ppm: u32,
    pub plane_directory: PlaneDirectoryRecord,
}

impl WireEncode for PageIndexRecord {
    fn encode_into(&self, output: &mut [u8]) -> Result<(), EncodeError> {
        let record = require(output, PAGE_INDEX_RECORD_SIZE)?;
        record.fill(0);
        put_u32(record, 0, self.page_number);
        for (offset, value) in [
            (4, self.page_kind),
            (6, self.pixel_format),
            (8, self.compression_method),
            (10, self.update_hint),
            (20, self.stored_width),
            (22, self.stored_height),
            (24, self.placement_x),
            (26, self.placement_y),
        ] {
            put_u16(record, offset, value);
        }
        for (offset, value) in [
            (12, self.page_flags),
            (16, self.page_crc32),
            (28, self.source_spine_index),
            (32, self.chapter_nav_index),
            (36, self.progress_start_ppm),
            (40, self.progress_end_ppm),
        ] {
            put_u32(record, offset, value);
        }
        record[44] = self.plane_directory.bitmap;
        record[45..49].copy_from_slice(&self.plane_directory.compression);
        for (index, value) in self.plane_directory.offsets.iter().enumerate() {
            put_u32(record, 52 + index * 4, *value);
        }
        for (index, value) in self.plane_directory.sizes.iter().enumerate() {
            put_u32(record, 68 + index * 4, *value);
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NavIndexRecord {
    pub nav_index: u32,
    pub nav_type: u16,
    pub level: u16,
    pub title: StringRef,
    pub source_href: StringRef,
    pub source_spine_index: u32,
    pub target_page_number: u32,
    pub parent_nav_index: u32,
    pub first_child_nav_index: u32,
    pub next_sibling_nav_index: u32,
    pub nav_flags: u32,
}

impl WireEncode for NavIndexRecord {
    fn encode_into(&self, output: &mut [u8]) -> Result<(), EncodeError> {
        let record = require(output, NAV_INDEX_RECORD_SIZE)?;
        record.fill(0);
        put_u32(record, 0, self.nav_index);
        put_u16(record, 4, self.nav_type);
        put_u16(record, 6, self.level);
        put_string_ref(record, 8, self.title);
        put_string_ref(record, 16, self.source_href);
        for (offset, value) in [
            (24, self.source_spine_index),
            (28, self.target_page_number),
            (32, self.parent_nav_index),
            (36, self.first_child_nav_index),
            (40, self.next_sibling_nav_index),
            (44, self.nav_flags),
        ] {
            put_u32(record, offset, value);
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ChapterIndexRecord {
    pub chapter_index: u32,
    pub nav_index: u32,
    pub title: StringRef,
    pub target_page_number: u32,
    pub level: u16,
    pub nav_type: u16,
    pub source_spine_index: u32,
    pub chapter_flags: u32,
}

impl WireEncode for ChapterIndexRecord {
    fn encode_into(&self, output: &mut [u8]) -> Result<(), EncodeError> {
        let record = require(output, CHAPTER_INDEX_RECORD_SIZE)?;
        record.fill(0);
        put_u32(record, 0, self.chapter_index);
        put_u32(record, 4, self.nav_index);
        put_string_ref(record, 8, self.title);
        put_u32(record, 16, self.target_page_number);
        put_u16(record, 20, self.level);
        put_u16(record, 22, self.nav_type);
        put_u32(record, 24, self.source_spine_index);
        put_u32(record, 28, self.chapter_flags);
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PageChunkIndexRecord {
    pub page_number: u32,
    pub plane_slot: u8,
    pub chunk_index: u8,
    pub row_start: u16,
    pub row_count: u16,
    pub page_data_offset: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
}

impl WireEncode for PageChunkIndexRecord {
    fn encode_into(&self, output: &mut [u8]) -> Result<(), EncodeError> {
        let record = require(output, PAGE_CHUNK_INDEX_RECORD_SIZE)?;
        record.fill(0);
        put_u32(record, 0, self.page_number);
        record[4] = self.plane_slot;
        record[5] = self.chunk_index;
        put_u16(record, 6, self.row_start);
        put_u16(record, 8, self.row_count);
        put_u32(record, 12, self.page_data_offset);
        put_u32(record, 16, self.compressed_size);
        put_u32(record, 20, self.uncompressed_size);
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PageTransitionIndexRecord {
    pub from_page_number: u32,
    pub to_page_number: u32,
    pub changed_chunk_mask: u32,
    pub first_changed_chunk: u16,
    pub changed_chunk_count: u16,
    pub flags: u16,
}

impl WireEncode for PageTransitionIndexRecord {
    fn encode_into(&self, output: &mut [u8]) -> Result<(), EncodeError> {
        let record = require(output, PAGE_TRANSITION_INDEX_RECORD_SIZE)?;
        record.fill(0);
        put_u32(record, 0, self.from_page_number);
        put_u32(record, 4, self.to_page_number);
        put_u32(record, 8, self.changed_chunk_mask);
        put_u16(record, 12, self.first_changed_chunk);
        put_u16(record, 14, self.changed_chunk_count);
        put_u16(record, 16, self.flags);
        Ok(())
    }
}

fn put_string_ref(output: &mut [u8], offset: usize, value: StringRef) {
    put_u32(output, offset, value.offset);
    put_u32(output, offset + 4, value.length);
}
