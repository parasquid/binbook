use binbook_diagnostic_protocol::{
    StorageBackend, StoreBulkCredit, StoreBulkData, StoreBulkOpenRequest, StoreBulkOpenResponse,
    TransferDirection,
};
use heapless::Vec;

use crate::{
    bulk_lease_pool::BulkLeasePool,
    bulk_slots::{TRANSFER_DATA_BYTES, TRANSFER_SLOT_COUNT},
    bulk_upload_pipeline::BulkUploadPipeline,
    bulk_upload_runtime_types::{
        owned_request, select_chunk, select_window, validate_response, OutstandingWrite,
        PendingFinish, PendingOpen, FIRST_SEQUENCE,
    },
    storage_owner::{OwnedRequest, OwnedResponse, Request, Response, StorageClient, StorageName},
};

pub use crate::bulk_upload_runtime_types::{BulkUploadFinish, BulkUploadRuntimeError};

pub struct BulkUploadRuntime {
    pub(crate) pipeline: BulkUploadPipeline,
    pub(crate) leases: BulkLeasePool,
    pub(crate) pending_open: Option<PendingOpen>,
    pub(crate) pending_finish: Option<PendingFinish>,
    pub(crate) pending_abort: Option<u32>,
    pub(crate) outstanding: Vec<OutstandingWrite, TRANSFER_SLOT_COUNT>,
    pub(crate) next_sequence: u32,
}

const _: () = assert!(core::mem::size_of::<BulkUploadRuntime>() <= 512);

impl BulkUploadRuntime {
    pub fn new(buffers: [&'static mut [u8; TRANSFER_DATA_BYTES]; TRANSFER_SLOT_COUNT]) -> Self {
        Self::from_lease_pool(BulkLeasePool::new(buffers))
    }

    pub fn from_lease_pool(leases: BulkLeasePool) -> Self {
        Self {
            pipeline: BulkUploadPipeline::new(),
            leases,
            pending_open: None,
            pending_finish: None,
            pending_abort: None,
            outstanding: Vec::new(),
            next_sequence: FIRST_SEQUENCE,
        }
    }

    pub fn lease_pool_releasable(&self) -> bool {
        self.pending_open.is_none()
            && self.pending_finish.is_none()
            && self.pending_abort.is_none()
            && !self.pipeline.is_active()
            && self.outstanding.is_empty()
            && usize::from(self.leases.available()) == TRANSFER_SLOT_COUNT
    }

    pub fn into_lease_pool(self) -> BulkLeasePool {
        assert!(
            self.lease_pool_releasable(),
            "active upload cannot release leases"
        );
        self.leases
    }

    pub fn begin_open(
        &mut self,
        request: StoreBulkOpenRequest<'_>,
    ) -> Result<OwnedRequest<'static>, BulkUploadRuntimeError> {
        if self.pending_open.is_some()
            || self.pipeline.is_active()
            || self.pending_finish.is_some()
            || self.pending_abort.is_some()
            || !self.outstanding.is_empty()
            || usize::from(self.leases.available()) != TRANSFER_SLOT_COUNT
        {
            return Err(BulkUploadRuntimeError::Active);
        }
        if request.direction != TransferDirection::Upload
            || request.backend != StorageBackend::Sd
            || request.total_size == 0
        {
            return Err(BulkUploadRuntimeError::InvalidOpen);
        }
        let chunk = select_chunk(request.requested_chunk)?;
        let window = select_window(request.requested_window);
        let name =
            StorageName::new(request.name).map_err(|_| BulkUploadRuntimeError::InvalidOpen)?;
        let sequence = self.sequence();
        self.pending_open = Some(PendingOpen {
            sequence,
            total_size: request.total_size,
            expected_crc32: request.expected_crc32,
            chunk,
            window,
        });
        Ok(owned_request(
            sequence,
            Request::UploadBegin {
                name,
                expected_size: u64::from(request.total_size),
                expected_crc32: request.expected_crc32,
                power_save: false,
            },
            None,
        ))
    }

    pub fn complete_open(
        &mut self,
        response: OwnedResponse<'static>,
    ) -> Result<StoreBulkOpenResponse, BulkUploadRuntimeError> {
        let pending = self
            .pending_open
            .ok_or(BulkUploadRuntimeError::Correlation)?;
        validate_response(&response, pending.sequence)?;
        if response.buffer.is_some() {
            return Err(BulkUploadRuntimeError::Correlation);
        }
        let stream_id = match response.envelope.response {
            Ok(Response::UploadStarted(value)) if value != 0 => value,
            Ok(_) => return Err(BulkUploadRuntimeError::Correlation),
            Err(_) => return Err(BulkUploadRuntimeError::Owner),
        };
        let credits = self.pipeline.open(
            stream_id,
            pending.total_size,
            pending.expected_crc32,
            pending.chunk,
            pending.window,
        )?;
        self.leases.begin_session(self.pipeline.generation())?;
        self.pending_open = None;
        Ok(StoreBulkOpenResponse {
            stream_id,
            selected_chunk: pending.chunk,
            initial_credits: credits,
            direction: TransferDirection::Upload,
            durable_offset: 0,
            total_size: pending.total_size,
        })
    }

    pub fn admit(
        &mut self,
        frame: StoreBulkData<'_>,
    ) -> Result<OwnedRequest<'static>, BulkUploadRuntimeError> {
        let descriptor = self.pipeline.admit(frame)?;
        self.leases.write(descriptor, frame.data)?;
        self.pipeline.take_for_storage(descriptor)?;
        let lease = self.leases.take(descriptor)?;
        let sequence = self.sequence();
        self.outstanding
            .push(OutstandingWrite {
                sequence,
                descriptor,
            })
            .map_err(|_| BulkUploadRuntimeError::Busy)?;
        let slot = lease.slot();
        Ok(owned_request(
            sequence,
            Request::UploadWrite {
                upload_id: frame.stream_id,
                offset: u64::from(frame.offset),
                buffer_offset: 0,
                len: descriptor.len,
                buffer: slot,
                power_save: false,
            },
            Some(lease),
        ))
    }

    pub fn complete_write(
        &mut self,
        mut response: OwnedResponse<'static>,
    ) -> Result<Option<StoreBulkCredit>, BulkUploadRuntimeError> {
        let position = self
            .outstanding
            .iter()
            .position(|write| write.sequence == response.envelope.sequence)
            .ok_or(BulkUploadRuntimeError::Correlation)?;
        let expected = self.outstanding[position];
        if response.envelope.client != StorageClient::Diagnostic {
            if response
                .buffer
                .as_ref()
                .is_some_and(|lease| lease.slot().bulk_index() == Some(expected.descriptor.slot))
            {
                let lease = response.buffer.take().expect("checked above");
                self.leases.restore(expected.descriptor, lease)?;
                self.outstanding.remove(position);
            }
            return Err(BulkUploadRuntimeError::Correlation);
        }
        let lease = response
            .buffer
            .take()
            .ok_or(BulkUploadRuntimeError::Correlation)?;
        self.leases.restore(expected.descriptor, lease)?;
        self.outstanding.remove(position);
        if expected.descriptor.generation != self.pipeline.generation() {
            return Ok(None);
        }
        if position != 0 {
            return Err(BulkUploadRuntimeError::Correlation);
        }
        match response.envelope.response {
            Ok(Response::Written(len)) if len == expected.descriptor.len => {}
            Ok(_) => return Err(BulkUploadRuntimeError::Correlation),
            Err(_) => return Err(BulkUploadRuntimeError::Owner),
        }
        let credit = {
            let bytes = self.leases.bytes(expected.descriptor)?;
            self.pipeline.complete_storage(expected.descriptor, bytes)?
        };
        Ok(credit)
    }

    pub fn available_leases(&self) -> u8 {
        self.leases.available()
    }

    pub fn outstanding(&self) -> usize {
        self.outstanding.len()
    }

    pub fn stream_id(&self) -> Option<u32> {
        self.pipeline.stream_id()
    }

    pub(crate) fn sequence(&mut self) -> u32 {
        let value = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1).max(1);
        value
    }
}
