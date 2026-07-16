use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockIoOperation {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockIoPhase {
    Start,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockIoEvent {
    pub operation: BlockIoOperation,
    pub phase: BlockIoPhase,
    pub start_block: u32,
    pub blocks: u16,
    pub success: bool,
}

pub trait BlockIoObserver {
    fn observe(&self, event: BlockIoEvent);

    fn observe_read_data(&self, _start_block: u32, _blocks: &[Block]) {}

    fn observe_write_data(&self, _start_block: u32, _blocks: &[Block]) {}
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopBlockIoObserver;

impl BlockIoObserver for NoopBlockIoObserver {
    fn observe(&self, _: BlockIoEvent) {}
}

pub struct MeasuredBlockDevice<D, O> {
    device: D,
    observer: O,
}

impl<D, O> MeasuredBlockDevice<D, O> {
    pub const fn new(device: D, observer: O) -> Self {
        Self { device, observer }
    }

    pub fn into_inner(self) -> D {
        self.device
    }

    fn event(
        &self,
        operation: BlockIoOperation,
        phase: BlockIoPhase,
        start_block: BlockIdx,
        blocks: usize,
        success: bool,
    ) where
        O: BlockIoObserver,
    {
        self.observer.observe(BlockIoEvent {
            operation,
            phase,
            start_block: start_block.0,
            blocks: blocks.min(usize::from(u16::MAX)) as u16,
            success,
        });
    }
}

impl<D, O> BlockDevice for MeasuredBlockDevice<D, O>
where
    D: BlockDevice,
    O: BlockIoObserver,
{
    type Error = D::Error;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), Self::Error> {
        self.event(
            BlockIoOperation::Read,
            BlockIoPhase::Start,
            start,
            blocks.len(),
            false,
        );
        let result = self.device.read(blocks, start);
        if result.is_ok() {
            self.observer.observe_read_data(start.0, blocks);
        }
        self.event(
            BlockIoOperation::Read,
            BlockIoPhase::Complete,
            start,
            blocks.len(),
            result.is_ok(),
        );
        result
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), Self::Error> {
        self.event(
            BlockIoOperation::Write,
            BlockIoPhase::Start,
            start,
            blocks.len(),
            false,
        );
        let result = self.device.write(blocks, start);
        if result.is_ok() {
            self.observer.observe_write_data(start.0, blocks);
        }
        self.event(
            BlockIoOperation::Write,
            BlockIoPhase::Complete,
            start,
            blocks.len(),
            result.is_ok(),
        );
        result
    }

    fn num_blocks(&self) -> Result<BlockCount, Self::Error> {
        self.device.num_blocks()
    }
}
