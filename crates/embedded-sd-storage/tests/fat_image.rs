//! Host test: serve a FAT image via a mock `BlockDevice` and verify
//! `SdStorage` enumerates and reads files.

use std::vec::Vec;
use std::{cell::RefCell, rc::Rc, string::String};

use embedded_sd_storage::{
    MountedDirectoryScan, MountedFileAccess, SdStorage, UploadJournalPhase, UploadJournalRecord,
};
use embedded_sdmmc::TimeSource;
use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx};

const FAT_IMAGE: &[u8] = include_bytes!("fixtures/fat16_with_book.img");
const BOOK_A: &[u8] = include_bytes!("../../binbook-storage/tests/fixtures/book_a.binbook");

#[derive(Debug, Default)]
struct SharedBlockState {
    blocks: Vec<Block>,
    read_count: usize,
    read_calls: usize,
    write_count: usize,
    write_calls: usize,
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
            read_calls: 0,
            write_count: 0,
            write_calls: 0,
        })))
    }

    fn reset_read_count(&self) {
        self.0.borrow_mut().read_count = 0;
    }

    fn read_count(&self) -> usize {
        self.0.borrow().read_count
    }

    fn reset_read_calls(&self) {
        self.0.borrow_mut().read_calls = 0;
    }

    fn read_calls(&self) -> usize {
        self.0.borrow().read_calls
    }

    fn reset_write_counts(&self) {
        let mut state = self.0.borrow_mut();
        state.write_count = 0;
        state.write_calls = 0;
    }

    fn write_count(&self) -> usize {
        self.0.borrow().write_count
    }

    fn write_calls(&self) -> usize {
        self.0.borrow().write_calls
    }
}

impl BlockDevice for SharedBlocks {
    type Error = core::convert::Infallible;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), Self::Error> {
        let mut source = self.0.borrow_mut();
        source.read_calls += 1;
        source.read_count += blocks.len();
        for (i, block) in blocks.iter_mut().enumerate() {
            let idx = (start.0 as usize) + i;
            *block = source.blocks[idx].clone();
        }
        Ok(())
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), Self::Error> {
        let mut target = self.0.borrow_mut();
        target.write_calls += 1;
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

#[test]
fn squidscript_style_begin_upload_writes_a_full_file_before_payload_overwrite() {
    const NAME: &str = "ACCOUNT.BIN";
    const SIZE: usize = 3_584;
    const FILE_BLOCKS: usize = SIZE / 512;

    let (mut storage, blocks) = storage_from_fixture();
    blocks.reset_write_counts();
    storage.begin_upload(NAME, SIZE as u64).unwrap();
    let preparation_blocks = blocks.write_count();
    let preparation_calls = blocks.write_calls();
    assert!(
        preparation_blocks >= FILE_BLOCKS,
        "zero-fill preparation wrote {preparation_blocks} blocks, below the {FILE_BLOCKS}-block file payload"
    );
    assert!(preparation_calls > 0);

    blocks.reset_write_counts();
    storage.write_upload_chunk(NAME, 0, &[0xA5; SIZE]).unwrap();
    assert_eq!(blocks.write_count(), FILE_BLOCKS);
    assert_eq!(blocks.write_calls(), FILE_BLOCKS);
    assert!(
        preparation_blocks + blocks.write_count() >= FILE_BLOCKS * 2,
        "preparation plus payload must write the complete file at least twice"
    );
}

fn storage_from_fixture() -> (SdStorage<SharedBlocks, FixedTime>, SharedBlocks) {
    let blocks = SharedBlocks::from_fixture();
    (
        SdStorage::from_block_device(blocks.clone(), FixedTime),
        blocks,
    )
}

fn mounted_scratch() -> &'static mut embedded_sd_storage::MountedScratch {
    Box::leak(Box::new(core::array::from_fn(|_| Block::new())))
}

#[test]
fn mounted_book_upload_is_hidden_until_publish_and_recovery_discards_receiving_stage() {
    let (storage, _blocks) = storage_from_fixture();
    let mounted = storage.into_owned_mount(mounted_scratch()).unwrap();
    let record = UploadJournalRecord::new(
        UploadJournalPhase::Receiving,
        mounted.next_upload_generation().unwrap(),
        7,
        0x1234_5678,
        "recover.binbook",
    )
    .unwrap();
    let mut upload = mounted.begin_book_upload(&record).unwrap();
    upload.write_at(0, b"partial").unwrap();
    upload.flush().unwrap();
    drop(upload);

    assert!(mounted.file_size("recover.binbook").is_err());
    mounted
        .discard_book_upload_stage(record.generation)
        .unwrap();
    mounted.recover_book_upload().unwrap();
    assert!(mounted.file_size("recover.binbook").is_err());

    let next = UploadJournalRecord::new(
        UploadJournalPhase::Receiving,
        mounted.next_upload_generation().unwrap(),
        3,
        0x0102_0304,
        "recover.binbook",
    )
    .unwrap();
    let mut replacement = mounted.begin_book_upload(&next).unwrap();
    replacement.write_at(0, b"new").unwrap();
    replacement.flush().unwrap();
    drop(replacement);
    mounted.publish_book_upload(&next).unwrap();
    let mut bytes = [0; 3];
    mounted.read_at("recover.binbook", 0, &mut bytes).unwrap();
    assert_eq!(&bytes, b"new");
}

#[test]
fn mounted_book_publish_replaces_only_after_complete_stage_and_removes_abort_stage() {
    let (mut storage, _blocks) = storage_from_fixture();
    storage.create_or_truncate("replace.binbook").unwrap();
    storage.write_at("replace.binbook", 0, b"old").unwrap();
    let mounted = storage.into_owned_mount(mounted_scratch()).unwrap();
    let record = UploadJournalRecord::new(
        UploadJournalPhase::Receiving,
        mounted.next_upload_generation().unwrap(),
        3,
        0xaabb_ccdd,
        "replace.binbook",
    )
    .unwrap();
    let mut upload = mounted.begin_book_upload(&record).unwrap();
    upload.write_at(0, b"new").unwrap();
    upload.flush().unwrap();

    let mut old = [0; 3];
    mounted.read_at("replace.binbook", 0, &mut old).unwrap();
    assert_eq!(&old, b"old");
    drop(upload);
    mounted.publish_book_upload(&record).unwrap();
    mounted.read_at("replace.binbook", 0, &mut old).unwrap();
    assert_eq!(&old, b"new");

    let aborted = UploadJournalRecord::new(
        UploadJournalPhase::Receiving,
        mounted.next_upload_generation().unwrap(),
        4,
        1,
        "aborted.binbook",
    )
    .unwrap();
    let mut upload = mounted.begin_book_upload(&aborted).unwrap();
    upload.write_at(0, b"nope").unwrap();
    drop(upload);
    mounted.abort_book_upload(&aborted).unwrap();
    assert!(mounted.file_size("aborted.binbook").is_err());
}

#[test]
fn mounted_storage_opens_metadata_from_a_dedicated_directory() {
    let (storage, _blocks) = storage_from_fixture();
    let mounted = storage.into_owned_mount(mounted_scratch()).unwrap();

    mounted
        .create_or_truncate_in_dir("BINBOOK", "LIBRARY.IDX")
        .unwrap();
    mounted
        .write_at_in_dir("BINBOOK", "LIBRARY.IDX", 0, b"cached")
        .unwrap();

    let mut file = mounted.open_file_in_dir("BINBOOK", "LIBRARY.IDX").unwrap();
    assert_eq!(file.size().unwrap(), 6);
    let mut bytes = [0; 6];
    file.read_at(0, &mut bytes).unwrap();
    assert_eq!(&bytes, b"cached");
}

#[test]
fn mounted_write_session_keeps_one_file_open_across_bounded_chunks() {
    let (storage, _blocks) = storage_from_fixture();
    let mounted = storage.into_owned_mount(mounted_scratch()).unwrap();
    let mut upload = mounted
        .create_file_session_in_dir("BINBOOK", "LIBRARY.IDX")
        .unwrap();
    upload.write_at(0, b"header").unwrap();
    upload.write_at(6, b"records").unwrap();
    assert_eq!(upload.size().unwrap(), 13);
    upload.flush().unwrap();
    drop(upload);

    let mut read = mounted.open_file_in_dir("BINBOOK", "LIBRARY.IDX").unwrap();
    let mut bytes = [0; 13];
    read.read_at(0, &mut bytes).unwrap();
    assert_eq!(&bytes, b"headerrecords");
}

#[test]
fn mounted_incremental_growth_coalesces_repeated_fat_sector_updates() {
    const SIZE: usize = 1024 * 1024;
    const CHUNK: usize = 3_584;
    const PAYLOAD_SECTORS: usize = SIZE / 512;

    let (storage, blocks) = storage_from_fixture();
    let mounted = storage.into_owned_mount(mounted_scratch()).unwrap();
    let record = UploadJournalRecord::new(
        UploadJournalPhase::Receiving,
        mounted.next_upload_generation().unwrap(),
        SIZE as u32,
        1,
        "fat-cache.binbook",
    )
    .unwrap();
    let mut upload = mounted.begin_book_upload(&record).unwrap();
    blocks.reset_write_counts();
    let bytes = [0xa5; CHUNK];
    let mut offset = 0;
    while offset != SIZE {
        let len = (SIZE - offset).min(CHUNK);
        upload.write_at(offset as u64, &bytes[..len]).unwrap();
        offset += len;
    }
    upload.flush().unwrap();

    let calls = blocks.write_calls();
    assert!(
        calls < PAYLOAD_SECTORS,
        "incremental growth used {calls} physical calls for {PAYLOAD_SECTORS} payload sectors"
    );
}

struct FixedTime;

struct StorageSource<'a> {
    storage: &'a mut SdStorage<SharedBlocks, FixedTime>,
    name: &'a str,
}

impl binbook_core::ReadAt for StorageSource<'_> {
    type Error = embedded_sd_storage::sd_filesystem::StorageError<SharedBlocks>;

    fn len(&mut self) -> Result<u64, Self::Error> {
        self.storage.file_size(self.name)
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        self.storage.read_at(self.name, offset, out)
    }
}

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
fn read_at_rejects_a_short_read_instead_of_zero_filling_it() {
    let (mut storage, _blocks) = storage_from_fixture();
    storage.create_or_truncate("SHORT.BIN").unwrap();
    storage.write_at("SHORT.BIN", 0, b"short").unwrap();
    storage.flush("SHORT.BIN").unwrap();

    let mut out = [0xAA; 8];
    assert!(storage.read_at("SHORT.BIN", 0, &mut out).is_err());
}

#[test]
fn adjacent_long_binbook_names_both_open_through_the_cached_read_path() {
    let (mut storage, _blocks) = storage_from_fixture();
    for name in ["aaa-reader.binbook", "aab-reader.binbook"] {
        storage.begin_upload(name, BOOK_A.len() as u64).unwrap();
        storage.write_upload_chunk(name, 0, BOOK_A).unwrap();
        storage.commit_upload(name).unwrap();
    }

    for name in ["aaa-reader.binbook", "aab-reader.binbook"] {
        let mut scratch = [0u8; 8192];
        let book = binbook_core::Book::open(
            StorageSource {
                storage: &mut storage,
                name,
            },
            &mut scratch,
        )
        .unwrap_or_else(|error| panic!("{name} must open, got {error:?}"));
        assert_eq!(book.page_count(), 1);
    }
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
fn maximum_portable_binbook_filename_roundtrips_and_enumerates() {
    let (mut storage, _blocks) = storage_from_fixture();
    let name = format!("{}.binbook", "a".repeat(113));
    assert_eq!(name.len(), binbook_storage::MAX_BINBOOK_NAME_BYTES);

    storage.create_or_truncate(&name).unwrap();
    storage.write_at(&name, 0, BOOK_A).unwrap();
    storage.flush(&name).unwrap();

    let mut names = Vec::new();
    storage
        .for_each_binbook_entry(&mut |name, _, _| names.push(name.to_string()))
        .unwrap();
    assert_eq!(names, vec![name]);
}

#[test]
fn binbook_enumeration_excludes_names_beyond_portable_limit() {
    let (mut storage, _blocks) = storage_from_fixture();
    let name = format!("{}.binbook", "a".repeat(114));
    assert_eq!(name.len(), binbook_storage::MAX_BINBOOK_NAME_BYTES + 1);

    storage.create_or_truncate(&name).unwrap();
    storage.write_at(&name, 0, BOOK_A).unwrap();
    storage.flush(&name).unwrap();

    let mut names = Vec::new();
    storage
        .for_each_binbook_entry(&mut |name, _, _| names.push(name.to_string()))
        .unwrap();
    assert!(names.is_empty());
}

#[test]
fn mounted_scan_reports_a_portability_unsupported_binbook_name() {
    let (mut storage, _blocks) = storage_from_fixture();
    let name = format!("{}.binbook", "u".repeat(114));
    storage.create_or_truncate(&name).unwrap();
    storage.write_at(&name, 0, BOOK_A).unwrap();
    storage.flush(&name).unwrap();
    let fs = storage.into_mounted_filesystem(mounted_scratch()).unwrap();
    let mut scan = MountedDirectoryScan::start(&fs).unwrap();
    let mut unsupported = false;
    loop {
        let batch = scan.step(8).unwrap();
        unsupported |= batch.unsupported_name;
        assert!(batch
            .entries
            .iter()
            .all(|entry| entry.name.as_str() != name));
        if batch.complete {
            break;
        }
    }
    assert!(unsupported);
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
fn reader_state_slots_alternate_exact_bytes_and_are_not_binbooks() {
    let (mut storage, _blocks) = storage_from_fixture();
    let slot_a = b"BBST-state-generation-one";
    let slot_b = b"BBST-state-generation-two-with-more-bytes";

    storage
        .begin_upload("BINBOOK.A.STATE", slot_a.len() as u64)
        .unwrap();
    storage
        .write_upload_chunk("BINBOOK.A.STATE", 0, slot_a)
        .unwrap();
    storage.commit_upload("BINBOOK.A.STATE").unwrap();
    storage
        .begin_upload("BINBOOK.B.STATE", slot_b.len() as u64)
        .unwrap();
    storage
        .write_upload_chunk("BINBOOK.B.STATE", 0, &slot_b[..13])
        .unwrap();
    storage
        .write_upload_chunk("BINBOOK.B.STATE", 13, &slot_b[13..])
        .unwrap();
    storage.commit_upload("BINBOOK.B.STATE").unwrap();

    let mut read_a = vec![0; slot_a.len()];
    let mut read_b = vec![0; slot_b.len()];
    storage.read_at("BINBOOK.A.STATE", 0, &mut read_a).unwrap();
    storage.read_at("BINBOOK.B.STATE", 0, &mut read_b).unwrap();
    assert_eq!(read_a, slot_a);
    assert_eq!(read_b, slot_b);

    let mut binbooks = Vec::new();
    storage
        .for_each_binbook_entry(&mut |name, _, _| binbooks.push(name.to_string()))
        .unwrap();
    assert!(!binbooks.iter().any(|name| name.ends_with(".STATE")));
}

#[test]
fn reader_state_slot_can_grow_when_overwritten() {
    let (mut storage, _blocks) = storage_from_fixture();
    let small = [0x31; 204];
    let large = [0x42; 248];

    storage
        .begin_upload("BINBOOK.B.STATE", small.len() as u64)
        .unwrap();
    storage
        .write_upload_chunk("BINBOOK.B.STATE", 0, &small)
        .unwrap();
    storage.commit_upload("BINBOOK.B.STATE").unwrap();

    storage
        .begin_upload("BINBOOK.B.STATE", large.len() as u64)
        .unwrap();
    storage
        .write_upload_chunk("BINBOOK.B.STATE", 0, &large)
        .unwrap();
    storage.commit_upload("BINBOOK.B.STATE").unwrap();

    assert_eq!(storage.file_size("BINBOOK.B.STATE").unwrap(), 248);
    let mut actual = [0; 248];
    storage.read_at("BINBOOK.B.STATE", 0, &mut actual).unwrap();
    assert_eq!(actual, large);
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
fn aligned_warm_read_batches_blocks_per_extent() {
    let (mut storage, blocks) = storage_from_fixture();
    storage
        .begin_upload("long-book-title.binbook", BOOK_A.len() as u64)
        .unwrap();
    storage
        .write_upload_chunk("long-book-title.binbook", 0, BOOK_A)
        .unwrap();
    storage.commit_upload("long-book-title.binbook").unwrap();

    let mut out = [0u8; 4096];
    blocks.reset_read_calls();
    storage
        .read_at("long-book-title.binbook", 0, &mut out)
        .unwrap();

    assert_eq!(&out, &BOOK_A[..4096]);
    assert!(
        blocks.read_calls() <= 2,
        "fixture spans at most two extents; got {} device calls",
        blocks.read_calls()
    );
}

#[test]
fn scoped_mounted_source_supports_multiple_random_reads_without_extent_materialization() {
    let (mut storage, _blocks) = storage_from_fixture();
    storage
        .begin_upload("mounted.binbook", BOOK_A.len() as u64)
        .unwrap();
    storage
        .write_upload_chunk("mounted.binbook", 0, BOOK_A)
        .unwrap();
    storage.commit_upload("mounted.binbook").unwrap();
    let mut first = [0u8; 64];
    let mut later = [0u8; 1024];

    let result = storage
        .with_mounted_file("mounted.binbook", |source| {
            source.read_exact_at(0, &mut first).unwrap();
            source.read_exact_at(4096, &mut later).unwrap();
            source.len().unwrap()
        })
        .unwrap();

    assert_eq!(result, BOOK_A.len() as u64);
    assert_eq!(&first, &BOOK_A[..64]);
    assert_eq!(&later, &BOOK_A[4096..5120]);
    assert_eq!(storage.file_size("mounted.binbook").unwrap(), result);
}

#[test]
fn consuming_mount_keeps_one_directory_iterator_position_across_batches() {
    let (mut storage, _blocks) = storage_from_fixture();
    storage.create_or_truncate("A.binbook").unwrap();
    storage.create_or_truncate("B.binbook").unwrap();
    let fs = storage.into_mounted_filesystem(mounted_scratch()).unwrap();
    let root = fs.root_dir();
    let dir = root.open_dir("BOOKS").unwrap();
    let mut entries = dir.iter();

    let mut first_file = None;
    for entry in entries.by_ref() {
        let entry = entry.unwrap();
        if entry.is_file() {
            first_file = Some(entry.short_file_name_as_bytes().to_vec());
            break;
        }
    }
    let mut second_file = None;
    for entry in entries.by_ref() {
        let entry = entry.unwrap();
        if entry.is_file() {
            second_file = Some(entry.short_file_name_as_bytes().to_vec());
            break;
        }
    }

    assert!(first_file.is_some());
    assert!(second_file.is_some());
    assert_ne!(first_file, second_file);
    assert!(entries.all(|entry| !entry.unwrap().is_file()));
}

#[test]
fn mounted_scan_batches_eight_entries_and_allows_foreground_read_between_steps() {
    use lamfat::{Read, Seek, SeekFrom};

    let (mut storage, _blocks) = storage_from_fixture();
    for index in 0..18 {
        let name = format!("scan-{index:02}.binbook");
        storage.create_or_truncate(&name).unwrap();
        storage.write_at(&name, 0, &[index as u8]).unwrap();
    }
    let fs = storage.into_mounted_filesystem(mounted_scratch()).unwrap();
    let mut scan = MountedDirectoryScan::start(&fs).unwrap();

    let first = scan.step(8).unwrap();
    assert_eq!(first.inspected, 8);
    assert!(!first.complete);
    assert!(first.entries.len() <= 8);

    let root = fs.root_dir();
    let books = root.open_dir("BOOKS").unwrap();
    let mut foreground = books.open_file("scan-17.binbook").unwrap();
    foreground.seek(SeekFrom::Start(0)).unwrap();
    let mut byte = [0u8; 1];
    foreground.read_exact(&mut byte).unwrap();
    assert_eq!(byte, [17]);

    let mut inspected = usize::from(first.inspected);
    let mut names: Vec<String> = first
        .entries
        .iter()
        .map(|entry| entry.name.as_str().to_owned())
        .collect();
    loop {
        let batch = scan.step(8).unwrap();
        assert!(batch.inspected <= 8);
        inspected += usize::from(batch.inspected);
        names.extend(
            batch
                .entries
                .iter()
                .map(|entry| entry.name.as_str().to_owned()),
        );
        if batch.complete {
            break;
        }
    }

    assert!(inspected >= 18);
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("scan-"))
            .count(),
        18
    );

    drop(scan);
    let restarted = MountedDirectoryScan::start(&fs).unwrap().step(1).unwrap();
    assert_eq!(
        restarted.inspected, 1,
        "dropping a session cancels it cleanly"
    );
}

#[test]
fn owned_mounted_storage_serves_foreground_io_and_borrowed_scan_from_one_mount() {
    let (mut storage, _blocks) = storage_from_fixture();
    storage.create_or_truncate("before.binbook").unwrap();
    storage.write_at("before.binbook", 0, b"before").unwrap();
    let mounted = storage.into_owned_mount(mounted_scratch()).unwrap();

    assert_eq!(mounted.file_size("before.binbook").unwrap(), 6);
    let mut before = [0u8; 6];
    mounted.read_at("before.binbook", 0, &mut before).unwrap();
    assert_eq!(&before, b"before");

    mounted.create_or_truncate("after.binbook").unwrap();
    mounted.write_at("after.binbook", 0, b"after").unwrap();
    let mut scan = mounted.start_scan().unwrap();
    let first = scan.step(8).unwrap();
    assert!(first
        .entries
        .iter()
        .any(|entry| entry.name == "after.binbook"));

    mounted.delete_file("after.binbook").unwrap();
    assert!(mounted.file_size("after.binbook").is_err());
}

#[test]
fn file_size_after_read_cache_reports_logical_length() {
    let (mut storage, _blocks) = storage_from_fixture();

    storage.create_or_truncate("BOOK_A.BIN").unwrap();
    storage.write_at("BOOK_A.BIN", 0, b"BOOKDATA").unwrap();
    storage.flush("BOOK_A.BIN").unwrap();

    let mut out = [0u8; 4];
    storage.read_at("BOOK_A.BIN", 0, &mut out).unwrap();
    assert_eq!(&out, b"BOOK");

    assert_eq!(storage.file_size("BOOK_A.BIN").unwrap(), 8);
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
