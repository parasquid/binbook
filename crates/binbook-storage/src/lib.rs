//! Backend-agnostic BinBook file enumeration and ReadAt.
//!
//! Depends only on `binbook-core` and a `Filesystem` trait defined here, so it
//! works over any backend (SD FAT, internal flash, future USB). The SD adapter
//! lives in `binbook-fw`; `embedded-sd-storage` provides the raw SD+FAT engine.

#![no_std]

extern crate alloc;

pub mod enumerate;
pub mod filesystem;
pub mod read_at;

pub use enumerate::{enumerate_binbooks, enumerate_into, BinbookEntry};
pub use filesystem::{Filesystem, StorageError};
pub use read_at::{FsReadAt, FsReadError};
