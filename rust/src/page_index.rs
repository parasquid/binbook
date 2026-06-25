use crate::header::{read_le16, read_le32};
use crate::Error;

pub const PIXEL_FORMAT_GRAY1_PACKED: u16 = 1;
pub const PIXEL_FORMAT_GRAY2_PACKED: u16 = 2;
pub const COMPRESSION_NONE: u16 = 0;
pub const COMPRESSION_RLE_PACKBITS: u16 = 1;
pub const COMPRESSION_LZ4: u16 = 2;
pub const PAGE_INDEX_ENTRY_SIZE: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaneDir {
    pub bitmap: u8,
    pub compression: [u8; 4],
    pub offsets: [u32; 4],
    pub sizes: [u32; 4],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageInfo {
    pub page_number: u32,
    pub page_kind: u16,
    pub pixel_format: u16,
    pub compression_method: u16,
    pub page_flags: u32,
    pub page_crc32: u32,
    pub stored_width: u16,
    pub stored_height: u16,
    pub placement_x: u16,
    pub placement_y: u16,
    pub progress_start_ppm: u32,
    pub progress_end_ppm: u32,
    pub chapter_nav_index: i32,
    pub plane_dir: PlaneDir,
}

pub fn parse_page_info_from_bytes(bytes: &[u8]) -> Result<PageInfo, Error> {
    if bytes.len() < PAGE_INDEX_ENTRY_SIZE {
        return Err(Error::InvalidPageIndex);
    }
    let chapter_raw = read_le32(bytes, 32);
    let plane_bitmap = bytes[44];
    let plane_compression = [bytes[45], bytes[46], bytes[47], bytes[48]];
    let mut plane_offsets = [0u32; 4];
    let mut plane_sizes = [0u32; 4];
    for i in 0..4 {
        plane_offsets[i] = read_le32(bytes, 52 + i * 4);
    }
    for i in 0..4 {
        plane_sizes[i] = read_le32(bytes, 68 + i * 4);
    }
    Ok(PageInfo {
        page_number: read_le32(bytes, 0),
        page_kind: read_le16(bytes, 4),
        pixel_format: read_le16(bytes, 6),
        compression_method: read_le16(bytes, 8),
        page_flags: read_le32(bytes, 12),
        page_crc32: read_le32(bytes, 16),
        stored_width: read_le16(bytes, 20),
        stored_height: read_le16(bytes, 22),
        placement_x: read_le16(bytes, 24),
        placement_y: read_le16(bytes, 26),
        progress_start_ppm: read_le32(bytes, 36),
        progress_end_ppm: read_le32(bytes, 40),
        chapter_nav_index: if chapter_raw == u32::MAX {
            -1
        } else {
            chapter_raw as i32
        },
        plane_dir: PlaneDir {
            bitmap: plane_bitmap,
            compression: plane_compression,
            offsets: plane_offsets,
            sizes: plane_sizes,
        },
    })
}
