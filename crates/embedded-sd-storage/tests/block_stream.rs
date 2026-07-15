use std::cell::RefCell;
use std::rc::Rc;

use embedded_sd_storage::BlockDeviceStream;
use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx};
use lamfat::{Seek as _, Write as _};

#[derive(Default)]
struct State {
    blocks: Vec<Block>,
    reads: usize,
    writes: usize,
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
        })))
    }
}

impl BlockDevice for CountingDevice {
    type Error = core::convert::Infallible;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), Self::Error> {
        let mut state = self.0.borrow_mut();
        state.reads += blocks.len();
        for (offset, block) in blocks.iter_mut().enumerate() {
            *block = state.blocks[start.0 as usize + offset].clone();
        }
        Ok(())
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), Self::Error> {
        let mut state = self.0.borrow_mut();
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

    let state = state.borrow();
    assert_eq!(state.reads, 2);
    assert_eq!(state.writes, 2);
    assert_eq!(&state.blocks[0].contents[..510], &before[0].contents[..510]);
    assert_eq!(&state.blocks[0].contents[510..], &[9, 8]);
    assert_eq!(&state.blocks[1].contents[..2], &[7, 6]);
    assert_eq!(&state.blocks[1].contents[2..], &before[1].contents[2..]);
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
