use binbook_diagnostic_protocol::TransferDirection;

pub const TRANSFER_DATA_BYTES: usize = 3584;
pub const TRANSFER_SLOT_COUNT: usize = 4;
pub const TRANSFER_POOL_RAM_MAX: usize = 15 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkSlotState {
    Free,
    RxReady,
    StorageOwned,
    TxReady,
    AwaitingAck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkSlotError {
    Full,
    Length,
    InvalidSlot,
    StaleGeneration,
    DescriptorMismatch,
    WrongState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulkSlotDescriptor {
    pub slot: u8,
    pub generation: u32,
    pub offset: u32,
    pub len: u16,
    pub direction: TransferDirection,
}

#[derive(Clone, Copy)]
pub(crate) struct BulkSlot {
    pub(crate) generation: u32,
    pub(crate) offset: u32,
    pub(crate) len: u16,
    pub(crate) direction: TransferDirection,
    pub(crate) state: BulkSlotState,
}

impl BulkSlot {
    pub(crate) const fn new() -> Self {
        Self {
            generation: 0,
            offset: 0,
            len: 0,
            direction: TransferDirection::Unspecified,
            state: BulkSlotState::Free,
        }
    }

    pub(crate) fn descriptor(&self, slot: usize) -> BulkSlotDescriptor {
        BulkSlotDescriptor {
            slot: slot as u8,
            generation: self.generation,
            offset: self.offset,
            len: self.len,
            direction: self.direction,
        }
    }

    pub(crate) fn reset(&mut self, generation: u32) {
        self.generation = generation;
        self.offset = 0;
        self.len = 0;
        self.direction = TransferDirection::Unspecified;
        self.state = BulkSlotState::Free;
    }
}
