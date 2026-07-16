use binbook_diagnostic_protocol::{
    StoreBulkCredit, StoreBulkFinishResponse, StoreBulkOpenResponse,
};

use crate::{
    bulk_lease_pool::BulkLeasePool,
    bulk_readback_runtime::{BulkReadbackRuntime, BulkReadbackRuntimeError},
    bulk_upload_runtime::{BulkUploadRuntime, BulkUploadRuntimeError},
};

pub(crate) const OWNER_OPERATION_CAPACITY: usize = 8;

#[expect(
    clippy::large_enum_variant,
    reason = "one in-place direction runtime avoids heap use and duplicate four-slot pools"
)]
pub(crate) enum DirectionRuntime {
    Idle(BulkLeasePool),
    Upload(BulkUploadRuntime),
    Readback(BulkReadbackRuntime),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OwnerOperationKind {
    UploadOpen,
    UploadWrite,
    UploadFinish,
    UploadAbort,
    ReadbackOpen,
    ReadbackRead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingOwnerOperation {
    pub(crate) sequence: u32,
    pub(crate) kind: OwnerOperationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkTransferError {
    Active,
    WrongDirection,
    Correlation,
    QueueFull,
    Upload(BulkUploadRuntimeError),
    Readback(BulkReadbackRuntimeError),
}

impl From<BulkUploadRuntimeError> for BulkTransferError {
    fn from(value: BulkUploadRuntimeError) -> Self {
        Self::Upload(value)
    }
}

impl From<BulkReadbackRuntimeError> for BulkTransferError {
    fn from(value: BulkReadbackRuntimeError) -> Self {
        Self::Readback(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkOwnerEvent {
    Opened(StoreBulkOpenResponse),
    UploadCredit(Option<StoreBulkCredit>),
    UploadFinished {
        response: StoreBulkFinishResponse,
        published: bool,
    },
    UploadAborted,
}
