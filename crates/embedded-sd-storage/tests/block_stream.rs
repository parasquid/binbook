use std::cell::RefCell;
use std::rc::Rc;

use embedded_sd_storage::BlockDeviceStream;
use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx};
use lamfat::{Read as _, Seek as _, Write as _};

#[derive(Default)]
struct State {
    blocks: Vec<Block>,
    reads: usize,
    writes: usize,
    read_calls: usize,
    write_calls: usize,
}

#[derive(Clone)]
struct CountingDevice(Rc<RefCell<State>>);

impl CountingDevice {
    fn patterned(blocks: usize) -> Self {
        let blocks = (0..blocks)
            .map(|block_index| {
                let mut block = Block::new();
                for (index, byte) in block.contents.iter_mut().enumerate() {
                    *byte = (block_index.wrapping_mul(17).wrapping_add(index)) as u8;
                }
                block
            })
            .collect();
        Self(Rc::new(RefCell::new(State {
            blocks,
            reads: 0,
            writes: 0,
            read_calls: 0,
            write_calls: 0,
        })))
    }
}

impl BlockDevice for CountingDevice {
    type Error = core::convert::Infallible;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), Self::Error> {
        let mut state = self.0.borrow_mut();
        state.read_calls += 1;
        state.reads += blocks.len();
        for (offset, block) in blocks.iter_mut().enumerate() {
            *block = state.blocks[start.0 as usize + offset].clone();
        }
        Ok(())
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), Self::Error> {
        let mut state = self.0.borrow_mut();
        state.write_calls += 1;
        state.writes += blocks.len();
        for (offset, block) in blocks.iter().enumerate() {
            state.blocks[start.0 as usize + offset] = block.clone();
        }
        Ok(())
    }

    fn num_blocks(&self) -> Result<BlockCount, Self::Error> {
        Ok(BlockCount(self.0.borrow().blocks.len() as u32))
    }
}

#[test]
fn complete_aligned_block_overwrite_skips_read() {
    let device = CountingDevice::patterned(2);
    let state = device.0.clone();
    let mut stream = BlockDeviceStream::new(device, 0);
    let bytes = [0xA5; 512];

    assert_eq!(stream.write(&bytes).unwrap(), bytes.len());

    let state = state.borrow();
    assert_eq!(state.reads, 0);
    assert_eq!(state.writes, 1);
    assert_eq!(state.blocks[0].contents, bytes);
}

#[test]
fn several_complete_blocks_skip_all_pre_reads() {
    let device = CountingDevice::patterned(4);
    let state = device.0.clone();
    let mut stream = BlockDeviceStream::new(device, 0);
    let bytes = (0..1_536).map(|value| value as u8).collect::<Vec<_>>();

    assert_eq!(stream.write(&bytes).unwrap(), bytes.len());

    let state = state.borrow();
    assert_eq!(state.reads, 0);
    assert_eq!(state.writes, 3);
    assert_eq!(state.blocks[0].contents, bytes[0..512]);
    assert_eq!(state.blocks[2].contents, bytes[1_024..1_536]);
}

#[test]
fn unaligned_prefix_and_suffix_preserve_untouched_bytes() {
    let device = CountingDevice::patterned(2);
    let state = device.0.clone();
    let before = state.borrow().blocks.clone();
    let mut stream = BlockDeviceStream::new(device, 0);
    stream.seek(lamfat::SeekFrom::Start(510)).unwrap();

    assert_eq!(stream.write(&[9, 8, 7, 6]).unwrap(), 4);
    stream.flush().unwrap();

    let state = state.borrow();
    assert_eq!(state.reads, 2);
    assert_eq!(state.writes, 2);
    assert_eq!(&state.blocks[0].contents[..510], &before[0].contents[..510]);
    assert_eq!(&state.blocks[0].contents[510..], &[9, 8]);
    assert_eq!(&state.blocks[1].contents[..2], &[7, 6]);
    assert_eq!(&state.blocks[1].contents[2..], &before[1].contents[2..]);
}

#[test]
fn adjacent_partial_writes_to_one_sector_share_one_read_modify_write() {
    let device = CountingDevice::patterned(2);
    let state = device.0.clone();
    let before = state.borrow().blocks[0].clone();
    let mut stream = BlockDeviceStream::new(device, 0);

    stream.seek(lamfat::SeekFrom::Start(17)).unwrap();
    assert_eq!(stream.write(&[1, 2, 3]).unwrap(), 3);
    stream.seek(lamfat::SeekFrom::Start(29)).unwrap();
    assert_eq!(stream.write(&[4, 5]).unwrap(), 2);

    {
        let state = state.borrow();
        assert_eq!(state.read_calls, 1);
        assert_eq!(state.write_calls, 0);
    }

    stream.flush().unwrap();
    let state = state.borrow();
    assert_eq!(state.read_calls, 1);
    assert_eq!(state.write_calls, 1);
    assert_eq!(&state.blocks[0].contents[17..20], &[1, 2, 3]);
    assert_eq!(&state.blocks[0].contents[29..31], &[4, 5]);
    assert_eq!(&state.blocks[0].contents[..17], &before.contents[..17]);
    assert_eq!(&state.blocks[0].contents[31..], &before.contents[31..]);
}

#[test]
fn same_sector_read_observes_pending_partial_write_without_forcing_writeback() {
    let device = CountingDevice::patterned(2);
    let state = device.0.clone();
    let mut stream = BlockDeviceStream::new(device, 0);

    stream.seek(lamfat::SeekFrom::Start(7)).unwrap();
    stream.write(&[0xaa]).unwrap();
    stream.seek(lamfat::SeekFrom::Start(0)).unwrap();
    let mut actual = [0; 8];
    stream.read(&mut actual).unwrap();

    assert_eq!(actual[7], 0xaa);
    let state = state.borrow();
    assert_eq!(state.read_calls, 1);
    assert_eq!(state.write_calls, 0);
}

#[test]
fn full_read_from_different_sector_does_not_force_metadata_writeback() {
    let device = CountingDevice::patterned(2);
    let state = device.0.clone();
    let mut stream = BlockDeviceStream::new(device, 0);

    stream.seek(lamfat::SeekFrom::Start(7)).unwrap();
    stream.write(&[0xaa]).unwrap();
    stream.seek(lamfat::SeekFrom::Start(512)).unwrap();
    let mut actual = [0; 512];
    stream.read(&mut actual).unwrap();

    {
        let state = state.borrow();
        assert_eq!(state.write_calls, 0);
        assert_eq!(state.read_calls, 2);
    }
    stream.flush().unwrap();
    assert_eq!(state.borrow().blocks[0].contents[7], 0xaa);
}

#[test]
fn full_write_to_different_sector_does_not_force_metadata_writeback() {
    let device = CountingDevice::patterned(2);
    let state = device.0.clone();
    let mut stream = BlockDeviceStream::new(device, 0);

    stream.seek(lamfat::SeekFrom::Start(7)).unwrap();
    stream.write(&[0xaa]).unwrap();
    stream.seek(lamfat::SeekFrom::Start(512)).unwrap();
    stream.write(&[0x55; 512]).unwrap();

    {
        let state = state.borrow();
        assert_eq!(state.write_calls, 1);
        assert_eq!(state.blocks[1].contents, [0x55; 512]);
    }
    stream.flush().unwrap();
    let state = state.borrow();
    assert_eq!(state.write_calls, 2);
    assert_eq!(state.blocks[0].contents[7], 0xaa);
}

#[test]
fn consecutive_3584_byte_chunks_are_contiguous_and_read_free() {
    let device = CountingDevice::patterned(16);
    let state = device.0.clone();
    let mut stream = BlockDeviceStream::new(device, 0);
    let first = vec![0x35; 3_584];
    let second = vec![0x84; 3_584];

    assert_eq!(stream.write(&first).unwrap(), first.len());
    assert_eq!(stream.write(&second).unwrap(), second.len());

    let state = state.borrow();
    assert_eq!(state.reads, 0);
    assert_eq!(state.writes, 14);
    let stored = state
        .blocks
        .iter()
        .take(14)
        .flat_map(|block| block.contents)
        .collect::<Vec<_>>();
    assert_eq!(&stored[..3_584], first);
    assert_eq!(&stored[3_584..], second);
}

#[test]
fn caller_buffer_batches_a_3584_byte_write_into_one_device_call() {
    let device = CountingDevice::patterned(8);
    let state = device.0.clone();
    let mut scratch = core::array::from_fn::<_, 8, _>(|_| Block::new());
    let mut stream = BlockDeviceStream::with_scratch(device, 0, &mut scratch);
    let bytes = (0..3_584).map(|value| value as u8).collect::<Vec<_>>();

    assert_eq!(stream.write(&bytes).unwrap(), bytes.len());

    let state = state.borrow();
    assert_eq!(state.read_calls, 0);
    assert_eq!(state.write_calls, 1);
    assert_eq!(state.writes, 7);
}

#[test]
fn caller_buffer_bounds_writes_larger_than_eight_blocks() {
    let device = CountingDevice::patterned(16);
    let state = device.0.clone();
    let mut scratch = core::array::from_fn::<_, 8, _>(|_| Block::new());
    let mut stream = BlockDeviceStream::with_scratch(device, 0, &mut scratch);
    let bytes = vec![0x5a; 12 * 512];

    assert_eq!(stream.write(&bytes).unwrap(), bytes.len());

    let state = state.borrow();
    assert_eq!(state.write_calls, 2);
    assert_eq!(state.writes, 12);
}

#[test]
fn caller_buffer_batches_aligned_reads_and_preserves_bytes() {
    let device = CountingDevice::patterned(8);
    let state = device.0.clone();
    let expected = state
        .borrow()
        .blocks
        .iter()
        .take(7)
        .flat_map(|block| block.contents)
        .collect::<Vec<_>>();
    let mut scratch = core::array::from_fn::<_, 8, _>(|_| Block::new());
    let mut stream = BlockDeviceStream::with_scratch(device, 0, &mut scratch);
    let mut actual = vec![0; 3_584];

    assert_eq!(stream.read(&mut actual).unwrap(), actual.len());
    assert_eq!(actual, expected);

    let state = state.borrow();
    assert_eq!(state.read_calls, 1);
    assert_eq!(state.reads, 7);
}
