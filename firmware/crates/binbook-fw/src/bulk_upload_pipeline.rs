use binbook_diagnostic_protocol::{
    Status, StoreBulkCredit, StoreBulkData, StoreBulkFinishResponse, StoreBulkState,
    StoreBulkStatusResponse, StoreUploadCommitResult, TransferDirection,
};

use crate::bulk_slots::{
    BulkSlotDescriptor, BulkSlotPool, TRANSFER_DATA_BYTES, TRANSFER_SLOT_COUNT,
};
pub use crate::bulk_upload_types::BulkUploadError;
use crate::bulk_upload_types::UploadSession;

pub struct BulkUploadPipeline {
    pool: BulkSlotPool,
    active: Option<UploadSession>,
    aborted_tombstone: Option<u32>,
}

const _: () = assert!(
    core::mem::size_of::<BulkUploadPipeline>() <= crate::bulk_slots::TRANSFER_POOL_RAM_MAX + 128
);

impl BulkUploadPipeline {
    pub const fn new() -> Self {
        Self {
            pool: BulkSlotPool::new(),
            active: None,
            aborted_tombstone: None,
        }
    }

    pub fn open(
        &mut self,
        stream_id: u32,
        total_size: u32,
        expected_crc32: u32,
        chunk_size: u16,
        window: u8,
    ) -> Result<u8, BulkUploadError> {
        if self.active.is_some() {
            return Err(BulkUploadError::Active);
        }
        if stream_id == 0
            || total_size == 0
            || chunk_size == 0
            || usize::from(chunk_size) > TRANSFER_DATA_BYTES
            || window == 0
            || usize::from(window) > TRANSFER_SLOT_COUNT
        {
            return Err(BulkUploadError::Length);
        }
        self.pool.begin_session();
        self.aborted_tombstone = None;
        self.active = Some(UploadSession {
            stream_id,
            total_size,
            expected_crc32,
            chunk_size,
            window,
            sender_credits: window,
            pending_credits: 0,
            admitted_offset: 0,
            durable_offset: 0,
            crc_state: u32::MAX,
            end_seen: false,
        });
        Ok(window)
    }

    pub fn admit(
        &mut self,
        frame: StoreBulkData<'_>,
    ) -> Result<BulkSlotDescriptor, BulkUploadError> {
        let session = self.active.as_mut().ok_or(BulkUploadError::Inactive)?;
        if frame.stream_id != session.stream_id {
            return Err(BulkUploadError::WrongStream);
        }
        if frame.offset != session.admitted_offset {
            return Err(BulkUploadError::WrongOffset);
        }
        let len = u32::try_from(frame.data.len()).map_err(|_| BulkUploadError::Length)?;
        let end = frame
            .offset
            .checked_add(len)
            .ok_or(BulkUploadError::Length)?;
        if len == 0 || len > u32::from(session.chunk_size) || end > session.total_size {
            return Err(BulkUploadError::Length);
        }
        if end < session.total_size && len != u32::from(session.chunk_size) {
            return Err(BulkUploadError::Length);
        }
        if frame.end_of_data != (end == session.total_size) {
            return Err(BulkUploadError::EndFlag);
        }
        if session.sender_credits == 0 && self.pool.occupied() < TRANSFER_SLOT_COUNT as u8 {
            return Err(BulkUploadError::NoCredit);
        }
        let descriptor = self
            .pool
            .claim_upload(frame.offset, frame.data.len() as u16)?;
        session.sender_credits = session.sender_credits.saturating_sub(1);
        session.admitted_offset = end;
        session.end_seen = frame.end_of_data;
        Ok(descriptor)
    }

    pub fn take_for_storage(
        &mut self,
        descriptor: BulkSlotDescriptor,
    ) -> Result<(), BulkUploadError> {
        if self.active.is_none() {
            return Err(BulkUploadError::Inactive);
        }
        self.pool
            .take_upload_for_storage(descriptor)
            .map_err(Into::into)
    }

    pub fn complete_storage(
        &mut self,
        descriptor: BulkSlotDescriptor,
        durable_bytes: &[u8],
    ) -> Result<Option<StoreBulkCredit>, BulkUploadError> {
        let session = self.active.as_mut().ok_or(BulkUploadError::Inactive)?;
        if descriptor.offset != session.durable_offset {
            return Err(BulkUploadError::WrongOffset);
        }
        if durable_bytes.len() != usize::from(descriptor.len) {
            return Err(BulkUploadError::Length);
        }
        for byte in durable_bytes {
            session.crc_state = crate::crc32::update_byte(session.crc_state, *byte);
        }
        session.durable_offset = self.pool.complete_upload(descriptor)?;
        session.pending_credits = session.pending_credits.saturating_add(1);
        let batch = session.window;
        if session.pending_credits < batch && session.durable_offset != session.total_size {
            return Ok(None);
        }
        let credits = session.pending_credits;
        session.pending_credits = 0;
        session.sender_credits = session
            .sender_credits
            .saturating_add(credits)
            .min(session.window);
        Ok(Some(StoreBulkCredit {
            stream_id: session.stream_id,
            durable_offset: session.durable_offset,
            credits,
            status: Status::Ok,
        }))
    }

    pub fn status(&mut self, stream_id: u32) -> Result<StoreBulkStatusResponse, BulkUploadError> {
        let session = self.active.as_mut().ok_or(BulkUploadError::Inactive)?;
        if stream_id != session.stream_id {
            return Err(BulkUploadError::WrongStream);
        }
        if self.pool.occupied() != 0 {
            return Err(BulkUploadError::Busy);
        }
        session.pending_credits = 0;
        session.sender_credits = session.window;
        Ok(StoreBulkStatusResponse {
            stream_id,
            direction: TransferDirection::Upload,
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

    pub fn finish(&mut self, stream_id: u32) -> Result<StoreBulkFinishResponse, BulkUploadError> {
        let session = self.active.ok_or(BulkUploadError::Inactive)?;
        if stream_id != session.stream_id {
            return Err(BulkUploadError::WrongStream);
        }
        if self.pool.occupied() != 0 {
            return Err(BulkUploadError::Busy);
        }
        if !session.end_seen
            || session.admitted_offset != session.total_size
            || session.durable_offset != session.total_size
        {
            return Err(BulkUploadError::Incomplete);
        }
        let computed_crc32 = !session.crc_state;
        self.active = None;
        Ok(StoreBulkFinishResponse {
            stream_id,
            result: if computed_crc32 == session.expected_crc32 {
                StoreUploadCommitResult::Ok
            } else {
                StoreUploadCommitResult::CrcMismatch
            },
            total_bytes: session.total_size,
            computed_crc32,
        })
    }

    pub fn abort(&mut self, stream_id: u32) -> Result<(), BulkUploadError> {
        match self.active {
            Some(session) if session.stream_id == stream_id => {
                self.pool.abort_session();
                self.active = None;
                self.aborted_tombstone = Some(stream_id);
                Ok(())
            }
            Some(_) => Err(BulkUploadError::WrongStream),
            None if self.aborted_tombstone == Some(stream_id) => Ok(()),
            None => Err(BulkUploadError::WrongStream),
        }
    }

    pub fn admitted_offset(&self) -> u32 {
        self.active.map_or(0, |session| session.admitted_offset)
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

impl Default for BulkUploadPipeline {
    fn default() -> Self {
        Self::new()
    }
}
