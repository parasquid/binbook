use crate::Error;
use crate::header::{read_le16, read_le32, read_le64};

pub const PIXEL_FORMAT_GRAY1_PACKED: u16 = 1;
pub const PIXEL_FORMAT_GRAY2_PACKED: u16 = 2;
pub const COMPRESSION_NONE: u16 = 0;
pub const COMPRESSION_RLE_PACKBITS: u16 = 1;
pub const COMPRESSION_LZ4: u16 = 2;
pub const PAGE_INDEX_ENTRY_SIZE: usize = 76;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageInfo {
    pub page_number: u32,
    pub page_kind: u16,
    pub pixel_format: u16,
    pub compression_method: u16,
    pub stored_width: u16,
    pub stored_height: u16,
    pub placement_x: u16,
    pub placement_y: u16,
    pub progress_start_ppm: u32,
    pub progress_end_ppm: u32,
    pub chapter_nav_index: i32,
    pub blob_offset: u64,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
}

pub fn parse_page_info_from_bytes(
    bytes: &[u8],
    page_data_offset: u64,
) -> Result<PageInfo, Error> {
    if bytes.len() < PAGE_INDEX_ENTRY_SIZE {
        return Err(Error::InvalidPageIndex);
    }
    let chapter_raw = read_le32(bytes, 48);
    Ok(PageInfo {
        page_number: read_le32(bytes, 0),
        page_kind: read_le16(bytes, 4),
        pixel_format: read_le16(bytes, 6),
        compression_method: read_le16(bytes, 8),
        stored_width: read_le16(bytes, 36),
        stored_height: read_le16(bytes, 38),
        placement_x: read_le16(bytes, 40),
        placement_y: read_le16(bytes, 42),
        progress_start_ppm: read_le32(bytes, 52),
        progress_end_ppm: read_le32(bytes, 56),
        chapter_nav_index: if chapter_raw == u32::MAX { -1 } else { chapter_raw as i32 },
        blob_offset: page_data_offset + read_le64(bytes, 16),
        compressed_size: read_le32(bytes, 24),
        uncompressed_size: read_le32(bytes, 28),
    })
}
