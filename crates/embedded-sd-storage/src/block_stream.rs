use core::cmp::min;

use embedded_sdmmc::{Block, BlockDevice, BlockIdx};

#[allow(dead_code)]
#[derive(Debug)]
pub enum StreamError<E> {
    Device(E),
    UnexpectedEof,
    WriteZero,
    InvalidSeek,
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
pub struct BlockDeviceStream<D>
where
    D: BlockDevice,
{
    device: D,
    base_offset: u64,
    position: u64,
    scratch: Block,
}

impl<D> BlockDeviceStream<D>
where
    D: BlockDevice,
{
    #[allow(dead_code)]
    pub fn new(device: D, base_offset: u64) -> Self {
        Self {
            device,
            base_offset,
            position: 0,
            scratch: Block::new(),
        }
    }

    #[allow(dead_code)]
    fn read_bytes(&mut self, mut out: &mut [u8]) -> Result<usize, StreamError<D::Error>> {
        let total = out.len();
        while !out.is_empty() {
            let absolute = self.base_offset + self.position;
            let block_idx = BlockIdx((absolute / 512) as u32);
            let block_off = (absolute % 512) as usize;
            self.device
                .read(core::slice::from_mut(&mut self.scratch), block_idx)
                .map_err(StreamError::Device)?;
            let len = min(out.len(), 512 - block_off);
            out[..len].copy_from_slice(&self.scratch.contents[block_off..block_off + len]);
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
            if block_off != 0 || len != 512 {
                self.device
                    .read(core::slice::from_mut(&mut self.scratch), block_idx)
                    .map_err(StreamError::Device)?;
            }
            self.scratch.contents[block_off..block_off + len].copy_from_slice(&input[..len]);
            self.device
                .write(core::slice::from_ref(&self.scratch), block_idx)
                .map_err(StreamError::Device)?;
            self.position += len as u64;
            input = &input[len..];
        }
        Ok(total)
    }
}

impl<D> lamfat::IoBase for BlockDeviceStream<D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    type Error = StreamError<D::Error>;
}

impl<D> lamfat::Read for BlockDeviceStream<D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    fn read(&mut self, out: &mut [u8]) -> Result<usize, Self::Error> {
        self.read_bytes(out)
    }
}

impl<D> lamfat::Write for BlockDeviceStream<D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    fn write(&mut self, input: &[u8]) -> Result<usize, Self::Error> {
        self.write_bytes(input)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl<D> lamfat::Seek for BlockDeviceStream<D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
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
