//! Generic SPI-mode SD + FAT16/32 storage over `embedded-hal`.
//!
//! Wraps `embedded-sdmmc`. Takes an `SpiDevice<u8>` (CS-managed, shareable via
//! `embedded-hal-bus`) + a `DelayNs`. Knows nothing about X4 pins or BinBook.

#![no_std]

mod block_stream;
mod fat_directory;
pub mod sd_filesystem;

pub use block_stream::{BlockDeviceStream, StreamError};
pub use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx, TimeSource, Timestamp};
pub use sd_filesystem::{
    MountedDirectoryScan, MountedDirectorySession, MountedFileAccess, MountedFileScanBatch,
    MountedFileSession, MountedNamedFile, MountedScanBatch, MountedScanEntry, MountedScanFile,
    MountedStorage, MountedWriteSession, SdStorage,
};
