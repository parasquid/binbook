use crate::{
    bulk_lease_pool::BulkLeaseError,
    bulk_readback_pipeline::BulkReadbackError,
    bulk_slots::{BulkSlotDescriptor, TRANSFER_DATA_BYTES, TRANSFER_SLOT_COUNT},
    storage_owner::{
        BufferLease, OwnedRequest, OwnedResponse, Request, RequestEnvelope, StorageClient,
        StorageName,
    },
};

pub(crate) const FIRST_SEQUENCE: u32 = 500_000;
pub(crate) const FIRST_STREAM: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkReadbackRuntimeError {
    Active,
    InvalidOpen,
    Correlation,
    Owner,
    Lease(BulkLeaseError),
    Pipeline(BulkReadbackError),
    Busy,
}

impl From<BulkLeaseError> for BulkReadbackRuntimeError {
    fn from(value: BulkLeaseError) -> Self {
        Self::Lease(value)
    }
}

impl From<BulkReadbackError> for BulkReadbackRuntimeError {
    fn from(value: BulkReadbackError) -> Self {
        Self::Pipeline(value)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PendingOpen {
    pub(crate) sequence: u32,
    pub(crate) name: StorageName,
    pub(crate) chunk: u16,
    pub(crate) window: u8,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct OutstandingRead {
    pub(crate) sequence: u32,
    pub(crate) descriptor: BulkSlotDescriptor,
}

pub(crate) fn select_chunk(requested: u16) -> Result<u16, BulkReadbackRuntimeError> {
    let automatic = TRANSFER_DATA_BYTES as u16;
    let selected = if requested == 0 {
        automatic
    } else {
        requested.min(automatic) / 512 * 512
    };
    (selected != 0)
        .then_some(selected)
        .ok_or(BulkReadbackRuntimeError::InvalidOpen)
}

pub(crate) fn select_window(requested: u8) -> u8 {
    if requested == 0 {
        TRANSFER_SLOT_COUNT as u8
    } else {
        requested.clamp(1, TRANSFER_SLOT_COUNT as u8)
    }
}

pub(crate) fn owned_request(
    sequence: u32,
    request: Request,
    buffer: Option<BufferLease<'static>>,
) -> OwnedRequest<'static> {
    OwnedRequest {
        envelope: RequestEnvelope {
            client: StorageClient::Diagnostic,
            sequence,
            request,
        },
        buffer,
    }
}

pub(crate) fn validate_response(
    response: &OwnedResponse<'_>,
    sequence: u32,
) -> Result<(), BulkReadbackRuntimeError> {
    if response.envelope.client == StorageClient::Diagnostic
        && response.envelope.sequence == sequence
    {
        Ok(())
    } else {
        Err(BulkReadbackRuntimeError::Correlation)
    }
}
