use crate::header::{read_le16, read_le32};

pub const NAV_TYPE_CHAPTER: u16 = 3;
pub const NAV_TYPE_SECTION: u16 = 4;

#[derive(Debug)]
pub struct NavEntry<'a> {
    pub nav_index: u32,
    pub nav_type: u16,
    pub level: u16,
    pub title: &'a [u8],
    pub rendered_page_number: u32,
    pub parent_nav_index: i32,
    pub first_child_nav_index: i32,
    pub next_sibling_nav_index: i32,
}

pub(crate) fn parse_nav_entry_from_bytes(
    bytes: &[u8],
    _title_offset: u32,
    _title_len: u32,
) -> NavEntry<'_> {
    let parent_raw = read_le32(bytes, 32);
    let first_child_raw = read_le32(bytes, 36);
    let next_sibling_raw = read_le32(bytes, 40);
    NavEntry {
        nav_index: read_le32(bytes, 0),
        nav_type: read_le16(bytes, 4),
        level: read_le16(bytes, 6),
        title: b"",
        rendered_page_number: read_le32(bytes, 28),
        parent_nav_index: if parent_raw == u32::MAX { -1 } else { parent_raw as i32 },
        first_child_nav_index: if first_child_raw == u32::MAX { -1 } else { first_child_raw as i32 },
        next_sibling_nav_index: if next_sibling_raw == u32::MAX { -1 } else { next_sibling_raw as i32 },
    }
}
