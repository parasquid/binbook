use core::cmp::min;

use embedded_sdmmc::{Block, BlockDevice, BlockIdx};

pub const STREAM_BATCH_BLOCKS: usize = 8;
const METADATA_CACHE_BLOCKS: usize = 2;

#[allow(dead_code)]
#[derive(Debug)]
pub enum StreamError<E> {
    Device(E),
    UnexpectedEof,
    WriteZero,
    InvalidSeek,
    NoScratch,
}

impl<E: core::fmt::Debug> lamfat::IoError for StreamError<E> {
    fn is_interrupted(&self) -> bool {
        false
    }

    fn new_unexpected_eof_error() -> Self {
        Self::UnexpectedEof
    }

    fn new_write_zero_error() -> Self {
        Self::WriteZero
    }
}

#[allow(dead_code)]
pub trait BlockScratch {
    fn blocks(&mut self) -> &mut [Block];
}

impl<const N: usize> BlockScratch for [Block; N] {
    fn blocks(&mut self) -> &mut [Block] {
        self
    }
}

impl<T> BlockScratch for &mut T
where
    T: BlockScratch + ?Sized,
{
    fn blocks(&mut self) -> &mut [Block] {
        (**self).blocks()
    }
}

pub struct BlockDeviceStream<D, S = [Block; 1]>
where
    D: BlockDevice,
    S: BlockScratch,
{
    device: D,
    base_offset: u64,
    position: u64,
    scratch: S,
    cached_blocks: [Block; METADATA_CACHE_BLOCKS],
    cached_block_indices: [Option<BlockIdx>; METADATA_CACHE_BLOCKS],
    cached_block_dirty: [bool; METADATA_CACHE_BLOCKS],
    next_cache_victim: usize,
}

impl<D> BlockDeviceStream<D, [Block; 1]>
where
    D: BlockDevice,
{
    #[allow(dead_code)]
    pub fn new(device: D, base_offset: u64) -> Self {
        Self {
            device,
            base_offset,
            position: 0,
            scratch: [Block::new()],
            cached_blocks: core::array::from_fn(|_| Block::new()),
            cached_block_indices: [None; METADATA_CACHE_BLOCKS],
            cached_block_dirty: [false; METADATA_CACHE_BLOCKS],
            next_cache_victim: 0,
        }
    }
}

impl<D, S> BlockDeviceStream<D, S>
where
    D: BlockDevice,
    S: BlockScratch,
{
    pub fn with_scratch(device: D, base_offset: u64, scratch: S) -> Self {
        Self {
            device,
            base_offset,
            position: 0,
            scratch,
            cached_blocks: core::array::from_fn(|_| Block::new()),
            cached_block_indices: [None; METADATA_CACHE_BLOCKS],
            cached_block_dirty: [false; METADATA_CACHE_BLOCKS],
            next_cache_victim: 0,
        }
    }

    fn flush_cache_slot(&mut self, slot: usize) -> Result<(), StreamError<D::Error>> {
        if self.cached_block_dirty[slot] {
            let block_idx = self.cached_block_indices[slot].ok_or(StreamError::InvalidSeek)?;
            self.device
                .write(core::slice::from_ref(&self.cached_blocks[slot]), block_idx)
                .map_err(StreamError::Device)?;
            self.cached_block_dirty[slot] = false;
        }
        Ok(())
    }

    fn flush_cached_blocks(&mut self) -> Result<(), StreamError<D::Error>> {
        for slot in 0..METADATA_CACHE_BLOCKS {
            self.flush_cache_slot(slot)?;
        }
        Ok(())
    }

    fn load_cached_block(&mut self, block_idx: BlockIdx) -> Result<usize, StreamError<D::Error>> {
        if let Some(slot) = self
            .cached_block_indices
            .iter()
            .position(|candidate| *candidate == Some(block_idx))
        {
            return Ok(slot);
        }
        let slot = self
            .cached_block_indices
            .iter()
            .position(Option::is_none)
            .unwrap_or_else(|| {
                let slot = self.next_cache_victim;
                self.next_cache_victim = (self.next_cache_victim + 1) % METADATA_CACHE_BLOCKS;
                slot
            });
        self.flush_cache_slot(slot)?;
        self.device
            .read(
                core::slice::from_mut(&mut self.cached_blocks[slot]),
                block_idx,
            )
            .map_err(StreamError::Device)?;
        self.cached_block_indices[slot] = Some(block_idx);
        Ok(slot)
    }

    fn cached_block_overlaps(cached: Option<BlockIdx>, start: BlockIdx, blocks: usize) -> bool {
        cached.is_some_and(|cached| {
            let cached = u64::from(cached.0);
            let start = u64::from(start.0);
            cached >= start && cached < start.saturating_add(blocks as u64)
        })
    }

    fn prepare_direct_io(
        &mut self,
        start: BlockIdx,
        blocks: usize,
    ) -> Result<(), StreamError<D::Error>> {
        for slot in 0..METADATA_CACHE_BLOCKS {
            if Self::cached_block_overlaps(self.cached_block_indices[slot], start, blocks) {
                self.flush_cache_slot(slot)?;
                self.cached_block_indices[slot] = None;
            }
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn read_bytes(&mut self, mut out: &mut [u8]) -> Result<usize, StreamError<D::Error>> {
        let total = out.len();
        while !out.is_empty() {
            let absolute = self.base_offset + self.position;
            let block_idx = BlockIdx((absolute / 512) as u32);
            let block_off = (absolute % 512) as usize;
            let scratch_len = self.scratch.blocks().len();
            if scratch_len == 0 {
                return Err(StreamError::NoScratch);
            }
            let full_blocks = if block_off == 0 { out.len() / 512 } else { 0 };
            let blocks = min(full_blocks, scratch_len);
            let len = if blocks > 0 {
                self.prepare_direct_io(block_idx, blocks)?;
                let scratch = self.scratch.blocks();
                self.device
                    .read(&mut scratch[..blocks], block_idx)
                    .map_err(StreamError::Device)?;
                let len = blocks * 512;
                for (destination, block) in out[..len].chunks_exact_mut(512).zip(&scratch[..blocks])
                {
                    destination.copy_from_slice(&block.contents);
                }
                len
            } else {
                let slot = self.load_cached_block(block_idx)?;
                let len = min(out.len(), 512 - block_off);
                out[..len].copy_from_slice(
                    &self.cached_blocks[slot].contents[block_off..block_off + len],
                );
                len
            };
            self.position += len as u64;
            out = &mut out[len..];
        }
        Ok(total)
    }

    #[allow(dead_code)]
    fn write_bytes(&mut self, mut input: &[u8]) -> Result<usize, StreamError<D::Error>> {
        let total = input.len();
        while !input.is_empty() {
            let absolute = self.base_offset + self.position;
            let block_idx = BlockIdx((absolute / 512) as u32);
            let block_off = (absolute % 512) as usize;
            let len = min(input.len(), 512 - block_off);
            let scratch_len = self.scratch.blocks().len();
            if scratch_len == 0 {
                return Err(StreamError::NoScratch);
            }
            let full_blocks = if block_off == 0 { input.len() / 512 } else { 0 };
            let blocks = min(full_blocks, scratch_len);
            let len = if blocks > 0 {
                self.prepare_direct_io(block_idx, blocks)?;
                let len = blocks * 512;
                let scratch = self.scratch.blocks();
                for (block, source) in scratch[..blocks]
                    .iter_mut()
                    .zip(input[..len].chunks_exact(512))
                {
                    block.contents.copy_from_slice(source);
                }
                self.device
                    .write(&scratch[..blocks], block_idx)
                    .map_err(StreamError::Device)?;
                len
            } else {
                let slot = self.load_cached_block(block_idx)?;
                self.cached_blocks[slot].contents[block_off..block_off + len]
                    .copy_from_slice(&input[..len]);
                self.cached_block_dirty[slot] = true;
                len
            };
            self.position += len as u64;
            input = &input[len..];
        }
        Ok(total)
    }
}

impl<D, S> lamfat::IoBase for BlockDeviceStream<D, S>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
    S: BlockScratch,
{
    type Error = StreamError<D::Error>;
}

impl<D, S> lamfat::Read for BlockDeviceStream<D, S>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
    S: BlockScratch,
{
    fn read(&mut self, out: &mut [u8]) -> Result<usize, Self::Error> {
        self.read_bytes(out)
    }
}

impl<D, S> lamfat::Write for BlockDeviceStream<D, S>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
    S: BlockScratch,
{
    fn write(&mut self, input: &[u8]) -> Result<usize, Self::Error> {
        self.write_bytes(input)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.flush_cached_blocks()
    }
}

impl<D, S> lamfat::Seek for BlockDeviceStream<D, S>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
    S: BlockScratch,
{
    fn seek(&mut self, pos: lamfat::SeekFrom) -> Result<u64, Self::Error> {
        let next = match pos {
            lamfat::SeekFrom::Start(offset) => offset,
            lamfat::SeekFrom::Current(delta) => self
                .position
                .checked_add_signed(delta)
                .ok_or(StreamError::InvalidSeek)?,
            lamfat::SeekFrom::End(_) => return Err(StreamError::InvalidSeek),
        };
        self.position = next;
        Ok(self.position)
    }
}

pub(crate) struct BorrowedBlockDevice<'a, D>(&'a mut D)
where
    D: BlockDevice;

impl<'a, D> BorrowedBlockDevice<'a, D>
where
    D: BlockDevice,
{
    pub(crate) fn new(device: &'a mut D) -> Self {
        Self(device)
    }
}

impl<D> BlockDevice for BorrowedBlockDevice<'_, D>
where
    D: BlockDevice,
{
    type Error = D::Error;

    fn read(&self, blocks: &mut [Block], start_block_idx: BlockIdx) -> Result<(), Self::Error> {
        self.0.read(blocks, start_block_idx)
    }

    fn write(&self, blocks: &[Block], start_block_idx: BlockIdx) -> Result<(), Self::Error> {
        self.0.write(blocks, start_block_idx)
    }

    fn num_blocks(&self) -> Result<embedded_sdmmc::BlockCount, Self::Error> {
        self.0.num_blocks()
    }
}
