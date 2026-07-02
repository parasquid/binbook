//! Host test: serve a FAT image via a mock `BlockDevice` and verify
//! `SdStorage` enumerates and reads files.

use std::vec::Vec;

use embedded_sd_storage::SdStorage;
use embedded_sdmmc::TimeSource;
use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx};

const FAT_IMAGE: &[u8] = include_bytes!("fixtures/fat16_with_book.img");

#[derive(Debug)]
struct RamBlockDevice {
    blocks: Vec<Block>,
}

impl BlockDevice for RamBlockDevice {
    type Error = core::convert::Infallible;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), Self::Error> {
        for (i, block) in blocks.iter_mut().enumerate() {
            let idx = (start.0 as usize) + i;
            *block = self.blocks[idx].clone();
        }
        Ok(())
    }

    fn write(&self, _blocks: &[Block], _start: BlockIdx) -> Result<(), Self::Error> {
        unimplemented!("read-only test")
    }

    fn num_blocks(&self) -> Result<BlockCount, Self::Error> {
        Ok(BlockCount(self.blocks.len() as u32))
    }
}

struct FixedTime;

impl TimeSource for FixedTime {
    fn get_timestamp(&self) -> embedded_sdmmc::Timestamp {
        embedded_sdmmc::Timestamp::from_calendar(2026, 7, 1, 12, 0, 0).expect("valid timestamp")
    }
}

#[test]
fn enumerates_and_reads_file_from_fat_image() {
    let blocks: Vec<Block> = FAT_IMAGE
        .chunks_exact(512)
        .map(|c| {
            let mut b = Block::new();
            b.contents.copy_from_slice(c);
            b
        })
        .collect();

    let bd = RamBlockDevice { blocks };
    let mut storage = SdStorage::from_block_device(bd, FixedTime);

    let mut names: Vec<(String, u64)> = Vec::new();
    storage
        .for_each_entry(&mut |name, size| {
            names.push((name.to_string(), size));
        })
        .unwrap();

    let book = names.iter().find(|(n, _)| n.contains("BOOK_A"));
    assert!(book.is_some(), "BOOK_A should be found, got: {names:?}");

    let mut buf = [0u8; 8];
    storage
        .read_at("BOOK_A.BIN", 0, &mut buf)
        .expect("read BOOK_A.BIN");
    assert_eq!(&buf, b"BOOKDATA");
}

#[test]
fn file_size_returns_correct_length() {
    let blocks: Vec<Block> = FAT_IMAGE
        .chunks_exact(512)
        .map(|c| {
            let mut b = Block::new();
            b.contents.copy_from_slice(c);
            b
        })
        .collect();

    let bd = RamBlockDevice { blocks };
    let mut storage = SdStorage::from_block_device(bd, FixedTime);

    match storage.file_size("BOOK_A.BIN") {
        Ok(size) => assert_eq!(size, 8),
        Err(e) => panic!("file_size failed: {e:?}"),
    }
}
