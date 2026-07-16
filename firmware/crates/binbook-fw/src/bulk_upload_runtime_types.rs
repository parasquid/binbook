use binbook_diagnostic_protocol::StoreBulkFinishResponse;

use crate::{
    bulk_lease_pool::BulkLeaseError,
    bulk_slots::{BulkSlotDescriptor, TRANSFER_DATA_BYTES, TRANSFER_SLOT_COUNT},
    bulk_upload_pipeline::BulkUploadError,
    storage_owner::{
        BufferLease, OwnedRequest, OwnedResponse, Request, RequestEnvelope, StorageClient,
    },
};

pub(crate) const FIRST_SEQUENCE: u32 = 500_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkUploadRuntimeError {
    Active,
    InvalidOpen,
    Correlation,
    Owner,
    Lease(BulkLeaseError),
    Pipeline(BulkUploadError),
    Busy,
}

impl From<BulkLeaseError> for BulkUploadRuntimeError {
    fn from(value: BulkLeaseError) -> Self {
        Self::Lease(value)
    }
}

impl From<BulkUploadError> for BulkUploadRuntimeError {
    fn from(value: BulkUploadError) -> Self {
        Self::Pipeline(value)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingOpen {
    pub(crate) sequence: u32,
    pub(crate) total_size: u32,
    pub(crate) expected_crc32: u32,
    pub(crate) chunk: u16,
    pub(crate) window: u8,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct OutstandingWrite {
    pub(crate) sequence: u32,
    pub(crate) descriptor: BulkSlotDescriptor,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingFinish {
    pub(crate) sequence: u32,
    pub(crate) response: StoreBulkFinishResponse,
    pub(crate) published: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulkUploadFinish {
    pub response: StoreBulkFinishResponse,
    pub published: bool,
}

pub(crate) fn select_chunk(requested: u16) -> Result<u16, BulkUploadRuntimeError> {
    let automatic = TRANSFER_DATA_BYTES as u16;
    let selected = if requested == 0 {
        automatic
    } else {
        requested.min(automatic) / 512 * 512
    };
    (selected != 0)
        .then_some(selected)
        .ok_or(BulkUploadRuntimeError::InvalidOpen)
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
) -> Result<(), BulkUploadRuntimeError> {
    if response.envelope.client == StorageClient::Diagnostic
        && response.envelope.sequence == sequence
    {
        Ok(())
    } else {
        Err(BulkUploadRuntimeError::Correlation)
    }
}
