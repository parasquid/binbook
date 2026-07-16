use binbook_diagnostic_protocol::{
    StorageBackend, StoreBulkCredit, StoreBulkData, StoreBulkOpenRequest, StoreBulkOpenResponse,
    TransferDirection,
};
use heapless::Vec;

use crate::{
    bulk_lease_pool::BulkLeasePool,
    bulk_readback_pipeline::BulkReadbackPipeline,
    bulk_readback_runtime_types::{
        owned_request, select_chunk, select_window, validate_response, OutstandingRead,
        PendingOpen, FIRST_SEQUENCE, FIRST_STREAM,
    },
    bulk_slots::{BulkSlotDescriptor, TRANSFER_DATA_BYTES, TRANSFER_SLOT_COUNT},
    storage_owner::{OwnedRequest, OwnedResponse, Request, Response, StorageName},
};

pub use crate::bulk_readback_runtime_types::BulkReadbackRuntimeError;

pub struct BulkReadbackRuntime {
    pub(crate) pipeline: BulkReadbackPipeline,
    pub(crate) leases: BulkLeasePool,
    pub(crate) pending_open: Option<PendingOpen>,
    pub(crate) name: Option<StorageName>,
    pub(crate) outstanding: Vec<OutstandingRead, TRANSFER_SLOT_COUNT>,
    pub(crate) sent: Vec<BulkSlotDescriptor, TRANSFER_SLOT_COUNT>,
    pub(crate) next_sequence: u32,
    pub(crate) next_stream: u32,
}

const _: () = assert!(core::mem::size_of::<BulkReadbackRuntime>() <= 768);

impl BulkReadbackRuntime {
    pub fn new(buffers: [&'static mut [u8; TRANSFER_DATA_BYTES]; TRANSFER_SLOT_COUNT]) -> Self {
        Self::from_lease_pool(BulkLeasePool::new(buffers))
    }

    pub fn from_lease_pool(leases: BulkLeasePool) -> Self {
        Self {
            pipeline: BulkReadbackPipeline::new(),
            leases,
            pending_open: None,
            name: None,
            outstanding: Vec::new(),
            sent: Vec::new(),
            next_sequence: FIRST_SEQUENCE,
            next_stream: FIRST_STREAM,
        }
    }

    pub fn lease_pool_releasable(&self) -> bool {
        self.pending_open.is_none()
            && self.name.is_none()
            && !self.pipeline.is_active()
            && self.outstanding.is_empty()
            && self.sent.is_empty()
            && usize::from(self.leases.available()) == TRANSFER_SLOT_COUNT
    }

    pub fn into_lease_pool(self) -> BulkLeasePool {
        assert!(
            self.lease_pool_releasable(),
            "active readback cannot release leases"
        );
        self.leases
    }

    pub fn begin_open(
        &mut self,
        request: StoreBulkOpenRequest<'_>,
    ) -> Result<OwnedRequest<'static>, BulkReadbackRuntimeError> {
        if self.pending_open.is_some()
            || self.pipeline.is_active()
            || !self.outstanding.is_empty()
            || usize::from(self.leases.available()) != TRANSFER_SLOT_COUNT
        {
            return Err(BulkReadbackRuntimeError::Active);
        }
        if request.direction != TransferDirection::Readback
            || request.backend != StorageBackend::Sd
            || request.total_size != 0
            || request.expected_crc32 != 0
        {
            return Err(BulkReadbackRuntimeError::InvalidOpen);
        }
        let name =
            StorageName::new(request.name).map_err(|_| BulkReadbackRuntimeError::InvalidOpen)?;
        let chunk = select_chunk(request.requested_chunk)?;
        let window = select_window(request.requested_window);
        let sequence = self.sequence();
        self.pending_open = Some(PendingOpen {
            sequence,
            name: name.clone(),
            chunk,
            window,
        });
        Ok(owned_request(sequence, Request::FileSize { name }, None))
    }

    pub fn complete_open(
        &mut self,
        response: OwnedResponse<'static>,
    ) -> Result<StoreBulkOpenResponse, BulkReadbackRuntimeError> {
        let pending = self
            .pending_open
            .clone()
            .ok_or(BulkReadbackRuntimeError::Correlation)?;
        validate_response(&response, pending.sequence)?;
        if response.buffer.is_some() {
            return Err(BulkReadbackRuntimeError::Correlation);
        }
        let total_size = match response.envelope.response {
            Ok(Response::FileSize(value)) => {
                u32::try_from(value).map_err(|_| BulkReadbackRuntimeError::InvalidOpen)?
            }
            Ok(_) => return Err(BulkReadbackRuntimeError::Correlation),
            Err(_) => return Err(BulkReadbackRuntimeError::Owner),
        };
        if total_size == 0 {
            return Err(BulkReadbackRuntimeError::InvalidOpen);
        }
        let stream_id = self.stream();
        let credits = self
            .pipeline
            .open(stream_id, total_size, pending.chunk, pending.window)?;
        self.leases.begin_session(self.pipeline.generation())?;
        let pending = self.pending_open.take().expect("validated above");
        self.name = Some(pending.name);
        Ok(StoreBulkOpenResponse {
            stream_id,
            selected_chunk: pending.chunk,
            initial_credits: credits,
            direction: TransferDirection::Readback,
            durable_offset: 0,
            total_size,
        })
    }

    pub fn begin_read(&mut self) -> Result<OwnedRequest<'static>, BulkReadbackRuntimeError> {
        if self
            .outstanding
            .iter()
            .any(|read| read.descriptor.generation != self.pipeline.generation())
        {
            return Err(BulkReadbackRuntimeError::Busy);
        }
        let name = self.name.clone().ok_or(BulkReadbackRuntimeError::Busy)?;
        let descriptor = self.pipeline.reserve_next()?;
        let lease = self.leases.take(descriptor)?;
        let sequence = self.sequence();
        self.outstanding
            .push(OutstandingRead {
                sequence,
                descriptor,
            })
            .map_err(|_| BulkReadbackRuntimeError::Busy)?;
        let slot = lease.slot();
        Ok(owned_request(
            sequence,
            Request::read(name, u64::from(descriptor.offset), descriptor.len, slot)
                .map_err(|_| BulkReadbackRuntimeError::InvalidOpen)?,
            Some(lease),
        ))
    }

    pub fn complete_read(
        &mut self,
        mut response: OwnedResponse<'static>,
    ) -> Result<Option<StoreBulkData<'_>>, BulkReadbackRuntimeError> {
        let position = self
            .outstanding
            .iter()
            .position(|read| read.sequence == response.envelope.sequence)
            .ok_or(BulkReadbackRuntimeError::Correlation)?;
        let expected = self.outstanding[position];
        if validate_response(&response, expected.sequence).is_err() {
            if response
                .buffer
                .as_ref()
                .is_some_and(|lease| lease.slot().bulk_index() == Some(expected.descriptor.slot))
            {
                let lease = response.buffer.take().expect("checked above");
                self.leases.restore(expected.descriptor, lease)?;
                self.outstanding.remove(position);
            }
            return Err(BulkReadbackRuntimeError::Correlation);
        }
        let lease = response
            .buffer
            .take()
            .ok_or(BulkReadbackRuntimeError::Correlation)?;
        self.leases.restore(expected.descriptor, lease)?;
        self.outstanding.remove(position);
        if expected.descriptor.generation != self.pipeline.generation() {
            return Ok(None);
        }
        if position != 0 {
            return Err(BulkReadbackRuntimeError::Correlation);
        }
        match response.envelope.response {
            Ok(Response::Read(len)) if len == expected.descriptor.len => {}
            Ok(_) => return Err(BulkReadbackRuntimeError::Correlation),
            Err(_) => return Err(BulkReadbackRuntimeError::Owner),
        }
        self.pipeline.complete_read(expected.descriptor)?;
        self.sent
            .push(expected.descriptor)
            .map_err(|_| BulkReadbackRuntimeError::Busy)?;
        let bytes = self.leases.bytes(expected.descriptor)?;
        self.pipeline
            .mark_sent(expected.descriptor, bytes)
            .map(Some)
            .map_err(Into::into)
    }

    pub fn apply_credit(
        &mut self,
        credit: StoreBulkCredit,
    ) -> Result<(), BulkReadbackRuntimeError> {
        let count = usize::from(credit.credits);
        if count == 0 || count > self.sent.len() {
            return Err(BulkReadbackRuntimeError::InvalidOpen);
        }
        let acknowledged: Vec<&[u8], TRANSFER_SLOT_COUNT> = self
            .sent
            .iter()
            .take(count)
            .map(|descriptor| self.leases.bytes(*descriptor))
            .collect::<Result<_, _>>()?;
        self.pipeline.apply_credit(credit, &acknowledged)?;
        for _ in 0..count {
            self.sent.remove(0);
        }
        Ok(())
    }

    pub fn available_leases(&self) -> u8 {
        self.leases.available()
    }

    pub fn outstanding(&self) -> usize {
        self.outstanding.len()
    }

    pub fn sent(&self) -> usize {
        self.sent.len()
    }

    pub fn stream_id(&self) -> Option<u32> {
        self.pipeline.stream_id()
    }

    pub(crate) fn sequence(&mut self) -> u32 {
        let value = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1).max(1);
        value
    }

    pub(crate) fn stream(&mut self) -> u32 {
        let value = self.next_stream;
        self.next_stream = self.next_stream.wrapping_add(1).max(1);
        value
    }
}
