use std::collections::BTreeMap;

use binbook_core::{Book, ReadAt, SliceSource};
use binbook_storage::{read_at::FsReadAt, Filesystem};

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

    fn for_each_entry(
        &mut self,
        visit: &mut dyn FnMut(&str, u64),
    ) -> Result<(), Self::Error> {
        for (name, bytes) in &self.files {
            visit(name, bytes.len() as u64);
        }
        Ok(())
    }

    fn read_at(
        &mut self,
        name: &str,
        offset: u64,
        out: &mut [u8],
    ) -> Result<(), Self::Error> {
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
fn reads_book_through_filesystem() {
    let mut fs = MemoryFs::new();
    fs.files
        .insert("book_a.binbook".to_string(), BOOK_A.to_vec());

    let source = FsReadAt::new(&mut fs, "book_a.binbook");
    let mut scratch = [0u8; 1024];
    let book = Book::open(source, &mut scratch).expect("open via FsReadAt");
    assert_eq!(book.page_count(), 1);
}

#[test]
fn read_exact_at_matches_slice_source() {
    let mut fs = MemoryFs::new();
    fs.files
        .insert("book_a.binbook".to_string(), BOOK_A.to_vec());

    let mut via_fs = [0u8; 16];
    FsReadAt::new(&mut fs, "book_a.binbook")
        .read_exact_at(4, &mut via_fs)
        .unwrap();

    let mut via_slice = [0u8; 16];
    SliceSource::new(BOOK_A)
        .read_exact_at(4, &mut via_slice)
        .unwrap();
    assert_eq!(via_fs, via_slice);
}
