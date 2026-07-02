use std::collections::BTreeMap;

use binbook_storage::{enumerate_binbooks, Filesystem};

const BOOK_A: &[u8] = include_bytes!("fixtures/book_a.binbook");

/// In-memory filesystem used for host-side integration tests.
struct MemoryFs {
    files: BTreeMap<String, Vec<u8>>,
}

impl MemoryFs {
    fn new() -> Self {
        Self {
            files: BTreeMap::new(),
        }
    }
}

impl Filesystem for MemoryFs {
    type Error = ();

    fn for_each_entry(&mut self, visit: &mut dyn FnMut(&str, u64)) -> Result<(), Self::Error> {
        for (name, bytes) in &self.files {
            visit(name, bytes.len() as u64);
        }
        Ok(())
    }

    fn read_at(&mut self, name: &str, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        let bytes = self.files.get(name).ok_or(())?;
        let start = usize::try_from(offset).map_err(|_| ())?;
        let end = start.checked_add(out.len()).ok_or(())?;
        out.copy_from_slice(bytes.get(start..end).ok_or(())?);
        Ok(())
    }

    fn file_size(&mut self, name: &str) -> Result<u64, Self::Error> {
        Ok(self.files.get(name).map(|b| b.len() as u64).unwrap_or(0))
    }
}

#[test]
fn enumerates_only_valid_binbooks() {
    let mut fs = MemoryFs::new();
    fs.files
        .insert("book_a.binbook".to_string(), BOOK_A.to_vec());
    fs.files
        .insert("notes.txt".to_string(), b"not a book".to_vec());
    fs.files
        .insert("corrupt.binbook".to_string(), b"BINBOOK\0garbage".to_vec());

    let entries = enumerate_binbooks(&mut fs).unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "book_a.binbook");
    assert_eq!(entries[0].file_size, BOOK_A.len() as u64);
    assert_eq!(entries[0].page_count, 1);
}

#[test]
fn skips_non_binbook_extension() {
    let mut fs = MemoryFs::new();
    fs.files
        .insert("readme.binbook.bak".to_string(), BOOK_A.to_vec());
    assert!(enumerate_binbooks(&mut fs).unwrap().is_empty());
}
