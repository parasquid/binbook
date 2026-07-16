use binbook_diagnostic_protocol::{
    Status, StoreBulkCredit, StoreBulkData, StoreBulkFinishResponse, StoreBulkState,
    StoreBulkStatusResponse, StoreUploadCommitResult, TransferDirection,
};

pub use crate::bulk_readback_types::BulkReadbackError;
use crate::bulk_readback_types::{DescriptorQueue, ReadbackSession};
use crate::bulk_slots::{
    BulkSlotDescriptor, BulkSlotPool, BulkSlotState, TRANSFER_DATA_BYTES, TRANSFER_SLOT_COUNT,
};

pub struct BulkReadbackPipeline {
    pool: BulkSlotPool,
    queued: DescriptorQueue,
    active: Option<ReadbackSession>,
    aborted_tombstone: Option<u32>,
}

const _: () = assert!(
    core::mem::size_of::<BulkReadbackPipeline>() <= crate::bulk_slots::TRANSFER_POOL_RAM_MAX + 256
);

impl BulkReadbackPipeline {
    pub const fn new() -> Self {
        Self {
            pool: BulkSlotPool::new(),
            queued: DescriptorQueue::new(),
            active: None,
            aborted_tombstone: None,
        }
    }

    pub fn open(
        &mut self,
        stream_id: u32,
        total_size: u32,
        chunk_size: u16,
        window: u8,
    ) -> Result<u8, BulkReadbackError> {
        if self.active.is_some() {
            return Err(BulkReadbackError::Active);
        }
        if stream_id == 0
            || total_size == 0
            || chunk_size == 0
            || usize::from(chunk_size) > TRANSFER_DATA_BYTES
            || window == 0
            || usize::from(window) > TRANSFER_SLOT_COUNT
        {
            return Err(BulkReadbackError::Length);
        }
        self.pool.begin_session();
        self.queued.clear();
        self.aborted_tombstone = None;
        self.active = Some(ReadbackSession {
            stream_id,
            total_size,
            chunk_size,
            window,
            produced_offset: 0,
            durable_offset: 0,
            crc_state: u32::MAX,
        });
        Ok(window)
    }

    pub fn reserve_next(&mut self) -> Result<BulkSlotDescriptor, BulkReadbackError> {
        let session = self.active.as_mut().ok_or(BulkReadbackError::Inactive)?;
        if session.produced_offset == session.total_size {
            return Err(BulkReadbackError::Incomplete);
        }
        if self.queued.len() >= usize::from(session.window) {
            return Err(crate::bulk_slots::BulkSlotError::Full.into());
        }
        let remaining = session.total_size - session.produced_offset;
        let len = remaining.min(u32::from(session.chunk_size)) as u16;
        let descriptor = self.pool.reserve_read(session.produced_offset, len)?;
        self.queued
            .push_back(descriptor)
            .map_err(|_| BulkReadbackError::Slot(crate::bulk_slots::BulkSlotError::Full))?;
        session.produced_offset += u32::from(len);
        Ok(descriptor)
    }

    pub fn complete_read(
        &mut self,
        descriptor: BulkSlotDescriptor,
    ) -> Result<(), BulkReadbackError> {
        if self.active.is_none() {
            return Err(BulkReadbackError::Inactive);
        }
        self.pool.complete_read(descriptor).map_err(Into::into)
    }

    pub fn mark_sent<'a>(
        &mut self,
        descriptor: BulkSlotDescriptor,
        data: &'a [u8],
    ) -> Result<StoreBulkData<'a>, BulkReadbackError> {
        let session = self.active.ok_or(BulkReadbackError::Inactive)?;
        if self.queued.iter().all(|queued| *queued != descriptor) {
            return Err(BulkReadbackError::WrongOffset);
        }
        if data.len() != usize::from(descriptor.len) {
            return Err(BulkReadbackError::Length);
        }
        self.pool.mark_awaiting_ack(descriptor)?;
        let end = descriptor.offset + u32::from(descriptor.len);
        Ok(StoreBulkData {
            stream_id: session.stream_id,
            offset: descriptor.offset,
            end_of_data: end == session.total_size,
            data,
        })
    }

    pub fn apply_credit(
        &mut self,
        credit: StoreBulkCredit,
        acknowledged: &[&[u8]],
    ) -> Result<(), BulkReadbackError> {
        let session = self.active.as_ref().ok_or(BulkReadbackError::Inactive)?;
        if credit.stream_id != session.stream_id {
            return Err(BulkReadbackError::WrongStream);
        }
        if credit.status != Status::Ok || credit.credits == 0 {
            return Err(BulkReadbackError::Status);
        }
        let count = usize::from(credit.credits);
        if count > self.queued.len() || count > usize::from(session.window) {
            return Err(BulkReadbackError::Credit);
        }
        if acknowledged.len() != count {
            return Err(BulkReadbackError::Credit);
        }
        let descriptors: heapless::Vec<_, TRANSFER_SLOT_COUNT> =
            self.queued.iter().take(count).copied().collect();
        if descriptors.len() != count
            || descriptors
                .iter()
                .any(|item| self.pool.state(item.slot) != Ok(BulkSlotState::AwaitingAck))
        {
            return Err(BulkReadbackError::Credit);
        }
        let expected = descriptors
            .last()
            .map(|item| item.offset + u32::from(item.len))
            .ok_or(BulkReadbackError::Credit)?;
        if credit.durable_offset != expected {
            return Err(BulkReadbackError::WrongOffset);
        }
        let session = self.active.as_mut().ok_or(BulkReadbackError::Inactive)?;
        for (descriptor, bytes) in descriptors.into_iter().zip(acknowledged) {
            if bytes.len() != usize::from(descriptor.len) {
                return Err(BulkReadbackError::Length);
            }
            for byte in *bytes {
                session.crc_state = crate::crc32::update_byte(session.crc_state, *byte);
            }
            self.pool.ack_read(descriptor)?;
            self.queued.pop_front();
        }
        session.durable_offset = expected;
        Ok(())
    }

    pub fn status(&mut self, stream_id: u32) -> Result<StoreBulkStatusResponse, BulkReadbackError> {
        let session = self.active.as_mut().ok_or(BulkReadbackError::Inactive)?;
        if stream_id != session.stream_id {
            return Err(BulkReadbackError::WrongStream);
        }
        self.pool.abort_session();
        self.queued.clear();
        session.produced_offset = session.durable_offset;
        Ok(StoreBulkStatusResponse {
            stream_id,
            direction: TransferDirection::Readback,
            state: StoreBulkState::Receiving,
            available_credits: session.window,
            durable_offset: session.durable_offset,
            total_size: session.total_size,
            running_crc32: if session.durable_offset == 0 {
                0
            } else {
                !session.crc_state
            },
        })
    }

    pub fn finish(&mut self, stream_id: u32) -> Result<StoreBulkFinishResponse, BulkReadbackError> {
        let session = self.active.ok_or(BulkReadbackError::Inactive)?;
        if stream_id != session.stream_id {
            return Err(BulkReadbackError::WrongStream);
        }
        if !self.queued.is_empty() || session.durable_offset != session.total_size {
            return Err(BulkReadbackError::Incomplete);
        }
        self.active = None;
        Ok(StoreBulkFinishResponse {
            stream_id,
            result: StoreUploadCommitResult::Ok,
            total_bytes: session.total_size,
            computed_crc32: !session.crc_state,
        })
    }

    pub fn abort(&mut self, stream_id: u32) -> Result<(), BulkReadbackError> {
        match self.active {
            Some(session) if session.stream_id == stream_id => {
                self.pool.abort_session();
                self.queued.clear();
                self.active = None;
                self.aborted_tombstone = Some(stream_id);
                Ok(())
            }
            Some(_) => Err(BulkReadbackError::WrongStream),
            None if self.aborted_tombstone == Some(stream_id) => Ok(()),
            None => Err(BulkReadbackError::WrongStream),
        }
    }

    pub fn produced_offset(&self) -> u32 {
        self.active.map_or(0, |session| session.produced_offset)
    }

    pub fn durable_offset(&self) -> u32 {
        self.active.map_or(0, |session| session.durable_offset)
    }

    pub const fn generation(&self) -> u32 {
        self.pool.generation()
    }

    pub const fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub const fn stream_id(&self) -> Option<u32> {
        match self.active {
            Some(session) => Some(session.stream_id),
            None => self.aborted_tombstone,
        }
    }
}

impl Default for BulkReadbackPipeline {
    fn default() -> Self {
        Self::new()
    }
}
