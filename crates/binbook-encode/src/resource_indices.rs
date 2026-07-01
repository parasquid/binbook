use binbook_core::{
    ChapterIndexRecord, FontResourceIndexEntry, NavIndexRecord, WireEncode,
    CHAPTER_INDEX_RECORD_SIZE, FONT_RESOURCE_RECORD_SIZE, NAV_INDEX_RECORD_SIZE,
};

use crate::model::{ModelError, NavigationEntry, UsedFont};
use crate::strings::StringTable;

pub(crate) fn build_navigation(
    entries: &[NavigationEntry],
    strings: &mut StringTable,
) -> Result<(Vec<u8>, Vec<u8>), ModelError> {
    let mut nav = Vec::new();
    let mut chapters = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let number = u32::try_from(index).map_err(|_| ModelError::TooManyRecords)?;
        let title = strings.add(&entry.title)?;
        let record = NavIndexRecord {
            nav_index: number,
            nav_type: entry.nav_type,
            level: entry.level,
            title,
            source_href: strings.add(&entry.source_href)?,
            source_spine_index: entry.source_spine_index,
            target_page_number: entry.target_page_number,
            parent_nav_index: entry.parent.unwrap_or(u32::MAX),
            first_child_nav_index: entry.first_child.unwrap_or(u32::MAX),
            next_sibling_nav_index: entry.next_sibling.unwrap_or(u32::MAX),
            nav_flags: 0,
        };
        encode_record(&record, NAV_INDEX_RECORD_SIZE, &mut nav)?;
        if matches!(entry.nav_type, 3 | 4) {
            let chapter = ChapterIndexRecord {
                chapter_index: u32::try_from(chapters.len() / CHAPTER_INDEX_RECORD_SIZE)
                    .map_err(|_| ModelError::TooManyRecords)?,
                nav_index: number,
                title,
                target_page_number: entry.target_page_number,
                level: entry.level,
                nav_type: entry.nav_type,
                source_spine_index: entry.source_spine_index,
                chapter_flags: 0,
            };
            encode_record(&chapter, CHAPTER_INDEX_RECORD_SIZE, &mut chapters)?;
        }
    }
    Ok((nav, chapters))
}

pub(crate) fn build_fonts(
    fonts: &[UsedFont],
    strings: &mut StringTable,
) -> Result<Vec<u8>, ModelError> {
    let mut sorted: Vec<&UsedFont> = fonts.iter().collect();
    sorted.sort_by(|left, right| {
        left.source_path
            .cmp(&right.source_path)
            .then(left.face_index.cmp(&right.face_index))
    });
    let mut output = Vec::with_capacity(sorted.len() * FONT_RESOURCE_RECORD_SIZE);
    for (index, font) in sorted.into_iter().enumerate() {
        if font.source_path.is_empty() || font.family.is_empty() {
            return Err(ModelError::InvalidFont);
        }
        let record = FontResourceIndexEntry {
            font_index: u32::try_from(index).map_err(|_| ModelError::TooManyRecords)?,
            source_kind: font.source_kind,
            flags: font.flags,
            weight: font.weight,
            stretch_milli: font.stretch_milli,
            style: font.style,
            family: strings.add(&font.family)?,
            source_path: strings.add(&font.source_path)?,
            sha256: font.sha256,
            face_index: font.face_index,
        };
        encode_record(&record, FONT_RESOURCE_RECORD_SIZE, &mut output)?;
    }
    Ok(output)
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
