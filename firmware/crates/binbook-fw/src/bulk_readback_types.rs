use crate::bulk_slots::{BulkSlotDescriptor, BulkSlotError, TRANSFER_SLOT_COUNT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkReadbackError {
    Active,
    Inactive,
    WrongStream,
    WrongOffset,
    Length,
    Credit,
    Status,
    Busy,
    Incomplete,
    Slot(BulkSlotError),
}

impl From<BulkSlotError> for BulkReadbackError {
    fn from(value: BulkSlotError) -> Self {
        Self::Slot(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReadbackSession {
    pub(crate) stream_id: u32,
    pub(crate) total_size: u32,
    pub(crate) chunk_size: u16,
    pub(crate) window: u8,
    pub(crate) produced_offset: u32,
    pub(crate) durable_offset: u32,
    pub(crate) crc_state: u32,
}

pub(crate) type DescriptorQueue = heapless::Deque<BulkSlotDescriptor, TRANSFER_SLOT_COUNT>;
