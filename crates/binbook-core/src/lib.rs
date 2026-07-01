#![no_std]

mod book;
mod book_io;
mod book_records;
mod chunk;
mod encode;
mod error;
mod font_resource;
mod header;
mod index_encode;
mod link_validation;
mod navigation;
mod page;
mod profile;
mod record_validation;
mod section;
mod source;
mod strings;
mod transition;
mod types;
mod validate;
mod validation_crc;

pub use book::Book;
pub use chunk::PageChunk;
pub use encode::{
    EncodeError, FileHeader, SectionTableEntry, WireEncode, HEADER_SIZE, SECTION_RECORD_SIZE,
};
pub use error::{Error, FormatError};
pub use font_resource::{
    FontResourceIndexEntry, FontSourceKind, FontStyle, FONT_RESOURCE_RECORD_SIZE,
};
pub use index_encode::{
    ChapterIndexRecord, NavIndexRecord, PageChunkIndexRecord, PageIndexRecord,
    PageTransitionIndexRecord, PlaneDirectoryRecord, CHAPTER_INDEX_RECORD_SIZE,
    NAV_INDEX_RECORD_SIZE, PAGE_CHUNK_INDEX_RECORD_SIZE, PAGE_INDEX_RECORD_SIZE,
    PAGE_TRANSITION_INDEX_RECORD_SIZE,
};
pub use navigation::{ChapterEntry, NavEntry};
pub use page::{PageInfo, PlaneDescriptor, PlaneDirectory, PAGE_RECORD_SIZE};
pub use profile::{BookMetadata, DisplayProfile};
pub use profile::{
    WAVEFORM_SSD1677_ABSOLUTE_GRAY2, WAVEFORM_SSD1677_STAGED_GRAY2, WAVEFORM_UNKNOWN,
};
pub use source::{ReadAt, SliceReadError, SliceSource};
pub use strings::StringRef;
pub use transition::PageTransition;
pub use types::{
    ByteLength, ChapterNumber, ChunkIndex, ChunkRecordNumber, CompressionMethod, FileOffset,
    NavNumber, PageNumber, PixelFormat, PlaneSlot, TransitionNumber,
};
pub use validate::{
    validate_all, ValidationCode, ValidationError, ValidationIssue, ValidationVisitor,
};
