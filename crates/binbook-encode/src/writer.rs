use std::io::{Seek, SeekFrom, Write};

use binbook_core::{FileHeader, SectionTableEntry, WireEncode, HEADER_SIZE, SECTION_RECORD_SIZE};

use crate::hashing::crc32;
use crate::indices::build_pages;
use crate::model::{
    BookConfig, BookMetadata, CompiledPage, EncodeError, FontPolicy, ModelError, NavigationEntry,
    SourceIdentity, UsedFont, WriteSummary,
};
use crate::policies::build_policies;
use crate::resource_indices::{build_fonts, build_navigation};
use crate::strings::StringTable;

pub const PAGE_DATA_ALIGNMENT: usize = 65_536;
const SECTION_COUNT: usize = 19;

#[derive(Debug, Clone)]
pub struct BookBuilder {
    config: BookConfig,
    metadata: BookMetadata,
    source: SourceIdentity,
    font_policy: FontPolicy,
    pages: Vec<CompiledPage>,
    navigation: Vec<NavigationEntry>,
    fonts: Vec<UsedFont>,
}

impl BookBuilder {
    #[must_use]
    pub fn new(config: BookConfig) -> Self {
        Self {
            config,
            metadata: BookMetadata::default(),
            source: SourceIdentity::default(),
            font_policy: FontPolicy::preserve(),
            pages: Vec::new(),
            navigation: Vec::new(),
            fonts: Vec::new(),
        }
    }

    pub fn set_metadata(&mut self, metadata: BookMetadata) {
        self.metadata = metadata;
    }

    pub fn set_source(&mut self, source: SourceIdentity) {
        self.source = source;
    }

    pub fn set_font_policy(&mut self, policy: FontPolicy) {
        self.font_policy = policy;
    }

    pub fn add_page(&mut self, page: CompiledPage) {
        self.pages.push(page);
    }

    pub fn add_navigation(&mut self, entry: NavigationEntry) {
        self.navigation.push(entry);
    }

    pub fn add_font(&mut self, font: UsedFont) {
        self.fonts.push(font);
    }

    pub fn write_to<W: Write + Seek>(&self, output: &mut W) -> Result<WriteSummary, EncodeError> {
        self.validate_model()?;
        let bytes = self.build_bytes()?;
        output.seek(SeekFrom::Start(0))?;
        output.write_all(&bytes)?;
        output.flush()?;
        Ok(WriteSummary {
            page_count: u32::try_from(self.pages.len()).map_err(|_| ModelError::TooManyRecords)?,
            output_bytes: u64::try_from(bytes.len()).map_err(|_| ModelError::LengthOverflow)?,
        })
    }

    fn build_bytes(&self) -> Result<Vec<u8>, EncodeError> {
        let pages = build_pages(&self.pages)?;
        let mut strings = StringTable::default();
        let fonts = build_fonts(&self.fonts, &mut strings)?;
        let (navigation, chapters) = build_navigation(&self.navigation, &mut strings)?;
        let policies = build_policies(
            &self.config,
            &self.metadata,
            &self.source,
            &self.font_policy,
            &fonts,
            &self.pages,
            &mut strings,
        )?;
        let strings = strings.into_bytes();
        let mut sections = vec![
            SectionData::plain(1, strings),
            SectionData::plain(10, policies.display),
            SectionData::plain(11, policies.layout),
            SectionData::plain(12, policies.requirements),
            SectionData::plain(20, policies.source),
            SectionData::plain(21, policies.metadata),
            SectionData::plain(22, policies.rendition),
            SectionData::plain(30, policies.font),
            SectionData::plain(31, policies.typography),
            SectionData::plain(32, policies.image),
            SectionData::plain(33, policies.compression),
            SectionData::plain(34, policies.chrome),
            SectionData::records(35, fonts, 80)?,
            SectionData::records(40, pages.page_index, 128)?,
            SectionData::records(41, navigation, 48)?,
            SectionData::records(43, chapters, 32)?,
            SectionData::records(44, pages.chunk_index, 24)?,
            SectionData::records(45, pages.transition_index, 24)?,
        ];
        let metadata_start = HEADER_SIZE + SECTION_COUNT * SECTION_RECORD_SIZE;
        let metadata_length = sections
            .iter()
            .map(|section| section.data.len())
            .sum::<usize>();
        let page_data_offset = align_up(metadata_start + metadata_length, PAGE_DATA_ALIGNMENT)?;
        sections.push(SectionData::plain(50, pages.page_data));
        assemble(sections, page_data_offset)
    }

    fn validate_model(&self) -> Result<(), ModelError> {
        if self.pages.is_empty() {
            return Err(ModelError::NoPages);
        }
        let page_count = u32::try_from(self.pages.len()).map_err(|_| ModelError::TooManyRecords)?;
        let nav_count =
            u32::try_from(self.navigation.len()).map_err(|_| ModelError::TooManyRecords)?;
        for entry in &self.navigation {
            if entry.target_page_number >= page_count
                || [entry.parent, entry.first_child, entry.next_sibling]
                    .into_iter()
                    .flatten()
                    .any(|value| value >= nav_count)
            {
                return Err(ModelError::InvalidNavigation);
            }
        }
        Ok(())
    }
}

struct SectionData {
    id: u16,
    data: Vec<u8>,
    entry_size: u32,
    record_count: u32,
}

impl SectionData {
    fn plain(id: u16, data: Vec<u8>) -> Self {
        Self {
            id,
            data,
            entry_size: 0,
            record_count: 0,
        }
    }

    fn records(id: u16, data: Vec<u8>, entry_size: u32) -> Result<Self, ModelError> {
        let size = usize::try_from(entry_size).map_err(|_| ModelError::LengthOverflow)?;
        if !data.len().is_multiple_of(size) {
            return Err(ModelError::LengthOverflow);
        }
        let record_count =
            u32::try_from(data.len() / size).map_err(|_| ModelError::TooManyRecords)?;
        Ok(Self {
            id,
            data,
            entry_size,
            record_count,
        })
    }
}

fn assemble(sections: Vec<SectionData>, page_data_offset: usize) -> Result<Vec<u8>, EncodeError> {
    let table_length = sections.len() * SECTION_RECORD_SIZE;
    let mut cursor = HEADER_SIZE + table_length;
    let mut entries = Vec::with_capacity(table_length);
    let mut metadata = Vec::new();
    let mut page_data = Vec::new();
    for section in sections {
        let is_page_data = section.id == 50;
        let offset = if is_page_data {
            page_data_offset
        } else {
            cursor
        };
        let entry = SectionTableEntry {
            section_id: section.id,
            section_flags: 0,
            offset: u64::try_from(offset).map_err(|_| ModelError::LengthOverflow)?,
            length: u64::try_from(section.data.len()).map_err(|_| ModelError::LengthOverflow)?,
            entry_size: section.entry_size,
            record_count: section.record_count,
            crc32: crc32(&section.data),
        };
        encode(&entry, SECTION_RECORD_SIZE, &mut entries)?;
        if is_page_data {
            page_data = section.data;
        } else {
            cursor += section.data.len();
            metadata.extend(section.data);
        }
    }
    let file_size = page_data_offset
        .checked_add(page_data.len())
        .ok_or(ModelError::LengthOverflow)?;
    let header = FileHeader {
        file_size: u64::try_from(file_size).map_err(|_| ModelError::LengthOverflow)?,
        section_table_offset: HEADER_SIZE as u64,
        section_table_length: u32::try_from(table_length)
            .map_err(|_| ModelError::LengthOverflow)?,
        section_count: u16::try_from(entries.len() / SECTION_RECORD_SIZE)
            .map_err(|_| ModelError::TooManyRecords)?,
        page_data_offset: u64::try_from(page_data_offset)
            .map_err(|_| ModelError::LengthOverflow)?,
        page_data_length: u64::try_from(page_data.len()).map_err(|_| ModelError::LengthOverflow)?,
        file_crc32: 0,
        header_crc32: 0,
        header_flags: 0,
    };
    let mut output = Vec::with_capacity(file_size);
    encode(&header, HEADER_SIZE, &mut output)?;
    output.extend(entries);
    output.extend(metadata);
    output.resize(page_data_offset, 0);
    output.extend(page_data);
    Ok(output)
}

fn encode(record: &impl WireEncode, size: usize, output: &mut Vec<u8>) -> Result<(), EncodeError> {
    let start = output.len();
    output.resize(start + size, 0);
    record.encode_into(&mut output[start..])?;
    Ok(())
}

fn align_up(value: usize, alignment: usize) -> Result<usize, ModelError> {
    value
        .checked_add(alignment - 1)
        .map(|sum| sum / alignment * alignment)
        .ok_or(ModelError::LengthOverflow)
}
