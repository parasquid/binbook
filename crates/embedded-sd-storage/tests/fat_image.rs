//! Host test: serve a FAT image via a mock `BlockDevice` and verify
//! `SdStorage` enumerates and reads files.

use std::vec::Vec;
use std::{cell::RefCell, rc::Rc, string::String};

use embedded_sd_storage::SdStorage;
use embedded_sdmmc::TimeSource;
use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx};

const FAT_IMAGE: &[u8] = include_bytes!("fixtures/fat16_with_book.img");
const BOOK_A: &[u8] = include_bytes!("../../binbook-storage/tests/fixtures/book_a.binbook");

#[derive(Debug, Default)]
struct SharedBlockState {
    blocks: Vec<Block>,
    read_count: usize,
    write_count: usize,
}

#[derive(Debug, Clone)]
struct SharedBlocks(Rc<RefCell<SharedBlockState>>);

impl SharedBlocks {
    fn from_fixture() -> Self {
        let blocks = FAT_IMAGE
            .chunks_exact(512)
            .map(|c| {
                let mut b = Block::new();
                b.contents.copy_from_slice(c);
                b
            })
            .collect();
        Self(Rc::new(RefCell::new(SharedBlockState {
            blocks,
            read_count: 0,
            write_count: 0,
        })))
    }

    fn reset_read_count(&self) {
        self.0.borrow_mut().read_count = 0;
    }

    fn read_count(&self) -> usize {
        self.0.borrow().read_count
    }
}

impl BlockDevice for SharedBlocks {
    type Error = core::convert::Infallible;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), Self::Error> {
        let mut source = self.0.borrow_mut();
        source.read_count += blocks.len();
        for (i, block) in blocks.iter_mut().enumerate() {
            let idx = (start.0 as usize) + i;
            *block = source.blocks[idx].clone();
        }
        Ok(())
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), Self::Error> {
        let mut target = self.0.borrow_mut();
        target.write_count += blocks.len();
        for (i, block) in blocks.iter().enumerate() {
            let idx = (start.0 as usize) + i;
            target.blocks[idx] = block.clone();
        }
        Ok(())
    }

    fn num_blocks(&self) -> Result<BlockCount, Self::Error> {
        Ok(BlockCount(self.0.borrow().blocks.len() as u32))
    }
}

fn storage_from_fixture() -> (SdStorage<SharedBlocks, FixedTime>, SharedBlocks) {
    let blocks = SharedBlocks::from_fixture();
    (
        SdStorage::from_block_device(blocks.clone(), FixedTime),
        blocks,
    )
}

struct FixedTime;

impl TimeSource for FixedTime {
    fn get_timestamp(&self) -> embedded_sdmmc::Timestamp {
        embedded_sdmmc::Timestamp::from_calendar(2026, 7, 1, 12, 0, 0).expect("valid timestamp")
    }
}

#[test]
fn enumerates_and_reads_file_from_fat_image() {
    let (mut storage, _blocks) = storage_from_fixture();

    storage.create_or_truncate("BOOK_A.BIN").unwrap();
    storage.write_at("BOOK_A.BIN", 0, b"BOOKDATA").unwrap();
    storage.flush("BOOK_A.BIN").unwrap();

    let mut names: Vec<(String, u64)> = Vec::new();
    storage
        .for_each_entry(&mut |name, size| {
            names.push((name.to_string(), size));
        })
        .unwrap();

    assert_eq!(names, vec![("BOOK_A.BIN".to_string(), 8)]);

    let mut buf = [0u8; 8];
    storage
        .read_at("BOOK_A.BIN", 0, &mut buf)
        .expect("read BOOK_A.BIN");
    assert_eq!(&buf, b"BOOKDATA");
}

#[test]
fn short_stem_binbook_is_not_rewritten_to_bbk() {
    let (mut storage, _blocks) = storage_from_fixture();

    storage.create_or_truncate("small.binbook").unwrap();
    storage.write_at("small.binbook", 0, b"BOOKDATA").unwrap();
    storage.flush("small.binbook").unwrap();

    let mut names: Vec<(String, u64)> = Vec::new();
    storage
        .for_each_entry(&mut |name, size| {
            names.push((name.to_string(), size));
        })
        .unwrap();

    assert_eq!(names, vec![("small.binbook".to_string(), 8)]);
    assert!(!names.iter().any(|(name, _)| name.ends_with(".BBK")));
}

#[test]
fn long_binbook_filename_roundtrips_as_real_directory_name() {
    let (mut storage, _blocks) = storage_from_fixture();

    storage
        .create_or_truncate("long-book-title.binbook")
        .unwrap();
    storage
        .write_at("long-book-title.binbook", 0, b"BOOKDATA")
        .unwrap();
    storage.flush("long-book-title.binbook").unwrap();

    let mut names: Vec<(String, u64)> = Vec::new();
    storage
        .for_each_entry(&mut |name, size| {
            names.push((name.to_string(), size));
        })
        .unwrap();

    assert_eq!(names, vec![("long-book-title.binbook".to_string(), 8)]);

    let mut buf = [0u8; 8];
    storage
        .read_at("long-book-title.binbook", 0, &mut buf)
        .unwrap();
    assert_eq!(&buf, b"BOOKDATA");

    storage.delete_file("long-book-title.binbook").unwrap();
    names.clear();
    storage
        .for_each_entry(&mut |name, size| {
            names.push((name.to_string(), size));
        })
        .unwrap();
    assert!(names.is_empty(), "expected empty /BOOKS, got {names:?}");
}

#[test]
fn binbook_metadata_enumeration_preserves_long_name_and_page_count() {
    let (mut storage, _blocks) = storage_from_fixture();

    storage
        .create_or_truncate("long-book-title.binbook")
        .unwrap();
    storage
        .write_at("long-book-title.binbook", 0, BOOK_A)
        .unwrap();
    storage.flush("long-book-title.binbook").unwrap();

    let mut entries: Vec<(String, u64, u32)> = Vec::new();
    storage
        .for_each_binbook_entry(&mut |name, size, page_count| {
            entries.push((name.to_string(), size, page_count));
        })
        .unwrap();

    assert_eq!(
        entries,
        vec![(
            "long-book-title.binbook".to_string(),
            BOOK_A.len() as u64,
            1
        )]
    );
}

#[test]
fn streaming_upload_writes_long_filename_in_chunks() {
    let (mut storage, _blocks) = storage_from_fixture();

    storage
        .begin_upload("long-book-title.binbook", BOOK_A.len() as u64)
        .unwrap();
    storage
        .write_upload_chunk("long-book-title.binbook", 0, &BOOK_A[..4096])
        .unwrap();
    storage
        .write_upload_chunk("long-book-title.binbook", 4096, &BOOK_A[4096..])
        .unwrap();
    storage.commit_upload("long-book-title.binbook").unwrap();

    let mut out = vec![0u8; BOOK_A.len()];
    storage
        .read_at("long-book-title.binbook", 0, &mut out)
        .unwrap();
    assert_eq!(out, BOOK_A);
}

#[test]
fn repeated_read_at_reuses_file_extent_mapping() {
    let (mut storage, blocks) = storage_from_fixture();

    storage
        .begin_upload("long-book-title.binbook", BOOK_A.len() as u64)
        .unwrap();
    storage
        .write_upload_chunk("long-book-title.binbook", 0, BOOK_A)
        .unwrap();
    storage.commit_upload("long-book-title.binbook").unwrap();

    let mut out = [0u8; 1024];
    storage
        .read_at("long-book-title.binbook", 0, &mut out)
        .unwrap();
    assert_eq!(&out, &BOOK_A[..1024]);

    blocks.reset_read_count();
    storage
        .read_at("long-book-title.binbook", 1024, &mut out)
        .unwrap();
    assert_eq!(&out, &BOOK_A[1024..2048]);
    assert!(
        blocks.read_count() <= 2,
        "warm repeated read should only touch data blocks, got {} block reads",
        blocks.read_count()
    );
}

#[test]
fn file_size_returns_correct_length() {
    let (mut storage, _blocks) = storage_from_fixture();

    storage.create_or_truncate("BOOK_A.BIN").unwrap();
    storage.write_at("BOOK_A.BIN", 0, b"BOOKDATA").unwrap();
    storage.flush("BOOK_A.BIN").unwrap();

    match storage.file_size("BOOK_A.BIN") {
        Ok(size) => assert_eq!(size, 8),
        Err(e) => panic!("file_size failed: {e:?}"),
    }
}

#[test]
fn delete_removes_file_from_books_directory() {
    let (mut storage, _blocks) = storage_from_fixture();

    storage.create_or_truncate("BOOK_A.BIN").unwrap();
    storage.write_at("BOOK_A.BIN", 0, b"BOOKDATA").unwrap();
    storage.flush("BOOK_A.BIN").unwrap();
    storage.delete_file("BOOK_A.BIN").unwrap();

    let mut names: Vec<String> = Vec::new();
    storage
        .for_each_entry(&mut |name, _size| {
            names.push(name.to_string());
        })
        .unwrap();

    assert!(names.is_empty(), "expected empty /BOOKS, got {names:?}");
}
