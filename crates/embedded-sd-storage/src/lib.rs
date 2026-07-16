//! Generic SPI-mode SD + FAT16/32 storage over `embedded-hal`.
//!
//! Wraps `embedded-sdmmc`. Takes an `SpiDevice<u8>` (CS-managed, shareable via
//! `embedded-hal-bus`) + a `DelayNs`. Knows nothing about X4 pins or BinBook.

#![no_std]

mod block_stream;
mod fat_directory;
mod fat_sector_map;
mod measured_block_device;
pub mod sd_filesystem;
mod upload_journal;

pub use block_stream::{BlockDeviceStream, StreamError};
pub use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx, TimeSource, Timestamp};
pub use fat_sector_map::{FatGeometry, FatGeometryError, SectorPurpose, SemanticSectorMap};
pub use measured_block_device::{
    BlockIoEvent, BlockIoObserver, BlockIoOperation, BlockIoPhase, MeasuredBlockDevice,
    NoopBlockIoObserver,
};
pub use sd_filesystem::{
    MountedDirectoryScan, MountedDirectorySession, MountedFileAccess, MountedFileScanBatch,
    MountedFileSession, MountedNamedFile, MountedScanBatch, MountedScanEntry, MountedScanFile,
    MountedScratch, MountedStorage, MountedWriteSession, SdStorage,
};
pub use upload_journal::{
    decode_upload_journal, encode_upload_journal, is_upload_journal_sector, UploadJournalError,
    UploadJournalPhase, UploadJournalRecord, UPLOAD_JOURNAL_BYTES,
};
