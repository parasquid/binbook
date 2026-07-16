use binbook_diagnostic_protocol::{
    Status, StoreBulkCredit, StoreBulkData, StoreBulkStatusResponse, TransferDirection,
};

use crate::bulk_state::BulkStateError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulkReadbackState {
    stream_id: u32,
    total_size: u32,
    chunk_size: u16,
    maximum_credits: u8,
    credit_batch: u8,
    pending_credits: u8,
    received_offset: u32,
    durable_offset: u32,
}

impl BulkReadbackState {
    pub fn new(
        stream_id: u32,
        total_size: u32,
        chunk_size: u16,
        maximum_credits: u8,
    ) -> Result<Self, BulkStateError> {
        if stream_id == 0 || total_size == 0 || chunk_size == 0 || maximum_credits == 0 {
            return Err(BulkStateError::InvalidParameters);
        }
        Ok(Self {
            stream_id,
            total_size,
            chunk_size,
            maximum_credits,
            credit_batch: (maximum_credits / 2).max(1),
            pending_credits: 0,
            received_offset: 0,
            durable_offset: 0,
        })
    }

    pub fn accept(&mut self, frame: &StoreBulkData<'_>) -> Result<(), BulkStateError> {
        if frame.stream_id != self.stream_id {
            return Err(BulkStateError::WrongStream);
        }
        if frame.offset != self.received_offset {
            return Err(BulkStateError::WrongOffset);
        }
        let len = u32::try_from(frame.data.len()).map_err(|_| BulkStateError::LengthOverflow)?;
        if len == 0 || len > u32::from(self.chunk_size) {
            return Err(BulkStateError::LengthOverflow);
        }
        let end = frame
            .offset
            .checked_add(len)
            .ok_or(BulkStateError::LengthOverflow)?;
        if end > self.total_size {
            return Err(BulkStateError::LengthOverflow);
        }
        if end < self.total_size && len != u32::from(self.chunk_size) {
            return Err(BulkStateError::EarlyShortChunk);
        }
        if frame.end_of_data != (end == self.total_size) {
            return Err(BulkStateError::InvalidEndFlag);
        }
        self.received_offset = end;
        Ok(())
    }

    pub fn mark_durable(
        &mut self,
        offset: u32,
        len: usize,
    ) -> Result<Option<StoreBulkCredit>, BulkStateError> {
        let len = u32::try_from(len).map_err(|_| BulkStateError::InvalidDurableWrite)?;
        let end = offset
            .checked_add(len)
            .ok_or(BulkStateError::InvalidDurableWrite)?;
        if offset != self.durable_offset
            || len == 0
            || end > self.received_offset
            || (end < self.total_size && len != u32::from(self.chunk_size))
        {
            return Err(BulkStateError::InvalidDurableWrite);
        }
        self.durable_offset = end;
        self.pending_credits = self
            .pending_credits
            .checked_add(1)
            .ok_or(BulkStateError::CreditOverflow)?;
        if self.pending_credits < self.credit_batch && end != self.total_size {
            return Ok(None);
        }
        let credit = StoreBulkCredit {
            stream_id: self.stream_id,
            durable_offset: self.durable_offset,
            credits: self.pending_credits,
            status: Status::Ok,
        };
        self.pending_credits = 0;
        Ok(Some(credit))
    }

    pub fn apply_status(&mut self, status: StoreBulkStatusResponse) -> Result<(), BulkStateError> {
        if status.stream_id != self.stream_id {
            return Err(BulkStateError::WrongStream);
        }
        if status.direction != TransferDirection::Readback
            || status.total_size != self.total_size
            || status.available_credits != self.maximum_credits
            || status.durable_offset != self.durable_offset
        {
            return Err(BulkStateError::InvalidStatus);
        }
        self.received_offset = status.durable_offset;
        self.pending_credits = 0;
        Ok(())
    }

    pub const fn received_offset(&self) -> u32 {
        self.received_offset
    }

    pub const fn durable_offset(&self) -> u32 {
        self.durable_offset
    }
}
