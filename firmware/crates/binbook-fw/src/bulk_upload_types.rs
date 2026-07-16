use crate::bulk_slots::BulkSlotError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkUploadError {
    Active,
    Inactive,
    WrongStream,
    WrongOffset,
    Length,
    EndFlag,
    NoCredit,
    Busy,
    Incomplete,
    Slot(BulkSlotError),
}

impl From<BulkSlotError> for BulkUploadError {
    fn from(value: BulkSlotError) -> Self {
        Self::Slot(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UploadSession {
    pub(crate) stream_id: u32,
    pub(crate) total_size: u32,
    pub(crate) expected_crc32: u32,
    pub(crate) chunk_size: u16,
    pub(crate) window: u8,
    pub(crate) sender_credits: u8,
    pub(crate) pending_credits: u8,
    pub(crate) admitted_offset: u32,
    pub(crate) durable_offset: u32,
    pub(crate) crc_state: u32,
    pub(crate) end_seen: bool,
}
