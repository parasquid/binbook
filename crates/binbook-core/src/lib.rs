#![no_std]

mod book;
mod book_io;
mod book_records;
mod chunk;
mod error;
mod header;
mod navigation;
mod page;
mod profile;
mod section;
mod source;
mod strings;
mod transition;
mod types;

pub use book::Book;
pub use chunk::PageChunk;
pub use error::{Error, FormatError};
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
