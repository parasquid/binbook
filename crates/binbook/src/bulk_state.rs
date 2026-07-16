use binbook_diagnostic_protocol::{
    Status, StoreBulkCredit, StoreBulkStatusResponse, TransferDirection,
};
use thiserror::Error;

pub use crate::bulk_readback::BulkReadbackState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UploadGrant {
    pub offset: u32,
    pub end_of_data: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BulkStateError {
    #[error("bulk stream parameters are invalid")]
    InvalidParameters,
    #[error("bulk frame belongs to another stream")]
    WrongStream,
    #[error("no bulk credit is available")]
    NoCredit,
    #[error("a non-final bulk chunk must have the negotiated length")]
    EarlyShortChunk,
    #[error("bulk chunk exceeds the remaining transfer length")]
    LengthOverflow,
    #[error("durable offset is not an admitted chunk boundary")]
    InvalidDurableOffset,
    #[error("bulk credit would exceed the negotiated window")]
    CreditOverflow,
    #[error("bulk peer reported status {0:?}")]
    DeviceStatus(Status),
    #[error("bulk status response is inconsistent with the active stream")]
    InvalidStatus,
    #[error("bulk data offset is not the next expected offset")]
    WrongOffset,
    #[error("bulk end-of-data flag does not match the declared total")]
    InvalidEndFlag,
    #[error("durable output does not match received bulk data")]
    InvalidDurableWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulkUploadState {
    stream_id: u32,
    total_size: u32,
    chunk_size: u16,
    maximum_credits: u8,
    available_credits: u8,
    sent_offset: u32,
    durable_offset: u32,
}

impl BulkUploadState {
    pub fn new(
        stream_id: u32,
        total_size: u32,
        chunk_size: u16,
        maximum_credits: u8,
        initial_credits: u8,
    ) -> Result<Self, BulkStateError> {
        if stream_id == 0
            || total_size == 0
            || chunk_size == 0
            || maximum_credits == 0
            || initial_credits == 0
            || initial_credits > maximum_credits
        {
            return Err(BulkStateError::InvalidParameters);
        }
        Ok(Self {
            stream_id,
            total_size,
            chunk_size,
            maximum_credits,
            available_credits: initial_credits,
            sent_offset: 0,
            durable_offset: 0,
        })
    }

    pub fn grant(&mut self, len: usize) -> Result<UploadGrant, BulkStateError> {
        if self.available_credits == 0 {
            return Err(BulkStateError::NoCredit);
        }
        let len = u32::try_from(len).map_err(|_| BulkStateError::LengthOverflow)?;
        if len == 0 || len > u32::from(self.chunk_size) {
            return Err(BulkStateError::LengthOverflow);
        }
        let end = self
            .sent_offset
            .checked_add(len)
            .ok_or(BulkStateError::LengthOverflow)?;
        if end > self.total_size {
            return Err(BulkStateError::LengthOverflow);
        }
        if end < self.total_size && len != u32::from(self.chunk_size) {
            return Err(BulkStateError::EarlyShortChunk);
        }
        let grant = UploadGrant {
            offset: self.sent_offset,
            end_of_data: end == self.total_size,
        };
        self.sent_offset = end;
        self.available_credits -= 1;
        Ok(grant)
    }

    pub fn apply_credit(&mut self, credit: StoreBulkCredit) -> Result<(), BulkStateError> {
        if credit.stream_id != self.stream_id {
            return Err(BulkStateError::WrongStream);
        }
        if credit.status != Status::Ok {
            return Err(BulkStateError::DeviceStatus(credit.status));
        }
        if !self.valid_durable_offset(credit.durable_offset) {
            return Err(BulkStateError::InvalidDurableOffset);
        }
        let credits = self
            .available_credits
            .checked_add(credit.credits)
            .ok_or(BulkStateError::CreditOverflow)?;
        if credits > self.maximum_credits {
            return Err(BulkStateError::CreditOverflow);
        }
        self.durable_offset = credit.durable_offset;
        self.available_credits = credits;
        Ok(())
    }

    pub fn apply_status(&mut self, status: StoreBulkStatusResponse) -> Result<(), BulkStateError> {
        if status.stream_id != self.stream_id {
            return Err(BulkStateError::WrongStream);
        }
        if status.direction != TransferDirection::Upload
            || status.total_size != self.total_size
            || status.available_credits != self.maximum_credits
            || !self.valid_durable_offset(status.durable_offset)
        {
            return Err(BulkStateError::InvalidStatus);
        }
        self.durable_offset = status.durable_offset;
        self.sent_offset = status.durable_offset;
        self.available_credits = status.available_credits;
        Ok(())
    }

    fn valid_durable_offset(&self, offset: u32) -> bool {
        offset >= self.durable_offset
            && offset <= self.sent_offset
            && (offset == self.total_size || offset.is_multiple_of(u32::from(self.chunk_size)))
    }

    pub const fn sent_offset(&self) -> u32 {
        self.sent_offset
    }

    pub const fn durable_offset(&self) -> u32 {
        self.durable_offset
    }

    pub const fn available_credits(&self) -> u8 {
        self.available_credits
    }
}
