//! Enumerate and validate `.binbook` files over a `Filesystem`.
//!
//! Each file is validated by opening it with `binbook_core::Book::open`, which
//! checks the `BINBOOK\0` magic + section table. Non-`.binbook` names and invalid
//! files are silently skipped.

use alloc::vec::Vec;

use crate::filesystem::{Filesystem, StorageError};
use crate::read_at::FsReadAt;
use binbook_core::Book;

/// A discovered BinBook file (header-only facts: name, size, page count).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinbookEntry {
    pub name: heapless::String<64>,
    pub file_size: u64,
    pub page_count: u32,
}

/// Maximum length of a BinBook filename (including `.binbook` extension).
const NAME_CAP: usize = 64;

/// List `.binbook` files in the filesystem, validating each by opening it.
///
/// Files whose name lacks the `.binbook` extension, exceeds `NAME_CAP`, or fails
/// to open as a BinBook are silently skipped.
pub fn enumerate_binbooks<F: Filesystem>(
    fs: &mut F,
) -> Result<Vec<BinbookEntry>, StorageError<F::Error>> {
    let mut out = Vec::new();
    enumerate_into(fs, &mut out)?;
    Ok(out)
}

/// Same as `enumerate_binbooks` but appends into a caller-owned buffer,
/// avoiding an allocation when the caller already has a buffer.
pub fn enumerate_into<F: Filesystem>(
    fs: &mut F,
    out: &mut Vec<BinbookEntry>,
) -> Result<(), StorageError<F::Error>> {
    let mut scratch = [0u8; 1024];
    let mut collected: Vec<(heapless::String<NAME_CAP>, u64)> = Vec::new();

    fs.for_each_entry(&mut |name, size| {
        if !name.ends_with(".binbook") {
            return;
        }
        if let Ok(name_buf) = heapless::String::<NAME_CAP>::try_from(name) {
            collected.push((name_buf, size));
        }
    })
    .map_err(StorageError::Backend)?;

    for (name, size) in collected {
        // Clone `name` to break the borrow: FsReadAt references the temporary
        // clone (stack-copy, no alloc), freeing `name` for the move below.
        let page_count = match Book::open(
            FsReadAt::new(fs, name.clone().as_str()),
            &mut scratch,
        ) {
            Ok(book) => book.page_count(),
            Err(_) => continue,
        };
        out.push(BinbookEntry {
            name,
            file_size: size,
            page_count,
        });
    }

    Ok(())
}
