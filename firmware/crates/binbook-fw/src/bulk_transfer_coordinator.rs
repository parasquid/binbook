use binbook_diagnostic_protocol::{StoreBulkData, StoreBulkOpenRequest, TransferDirection};
use heapless::Vec;

use crate::{
    bulk_lease_pool::BulkLeasePool,
    bulk_readback_runtime::BulkReadbackRuntime,
    bulk_slots::{TRANSFER_DATA_BYTES, TRANSFER_SLOT_COUNT},
    bulk_transfer_types::{
        DirectionRuntime, OwnerOperationKind, PendingOwnerOperation, OWNER_OPERATION_CAPACITY,
    },
    bulk_upload_runtime::BulkUploadRuntime,
    storage_owner::{OwnedRequest, OwnedResponse, StorageClient},
};

pub use crate::bulk_transfer_types::{BulkOwnerEvent, BulkTransferError};

pub struct BulkTransferCoordinator {
    pub(crate) runtime: Option<DirectionRuntime>,
    pub(crate) pending: Vec<PendingOwnerOperation, OWNER_OPERATION_CAPACITY>,
}

const _: () = assert!(core::mem::size_of::<BulkTransferCoordinator>() <= 1024);

impl BulkTransferCoordinator {
    pub fn new(buffers: [&'static mut [u8; TRANSFER_DATA_BYTES]; TRANSFER_SLOT_COUNT]) -> Self {
        Self {
            runtime: Some(DirectionRuntime::Idle(BulkLeasePool::new(buffers))),
            pending: Vec::new(),
        }
    }

    pub fn begin_open(
        &mut self,
        request: StoreBulkOpenRequest<'_>,
    ) -> Result<OwnedRequest<'static>, BulkTransferError> {
        let state = self.runtime.take().ok_or(BulkTransferError::Active)?;
        let DirectionRuntime::Idle(pool) = state else {
            self.runtime = Some(state);
            return Err(BulkTransferError::Active);
        };
        match request.direction {
            TransferDirection::Upload => {
                let mut runtime = BulkUploadRuntime::from_lease_pool(pool);
                let owner = match runtime.begin_open(request) {
                    Ok(owner) => owner,
                    Err(error) => {
                        self.runtime = Some(DirectionRuntime::Idle(runtime.into_lease_pool()));
                        return Err(error.into());
                    }
                };
                self.record(&owner, OwnerOperationKind::UploadOpen)?;
                self.runtime = Some(DirectionRuntime::Upload(runtime));
                Ok(owner)
            }
            TransferDirection::Readback => {
                let mut runtime = BulkReadbackRuntime::from_lease_pool(pool);
                let owner = match runtime.begin_open(request) {
                    Ok(owner) => owner,
                    Err(error) => {
                        self.runtime = Some(DirectionRuntime::Idle(runtime.into_lease_pool()));
                        return Err(error.into());
                    }
                };
                self.record(&owner, OwnerOperationKind::ReadbackOpen)?;
                self.runtime = Some(DirectionRuntime::Readback(runtime));
                Ok(owner)
            }
            TransferDirection::Unspecified => {
                self.runtime = Some(DirectionRuntime::Idle(pool));
                Err(BulkTransferError::WrongDirection)
            }
        }
    }

    pub fn admit_upload(
        &mut self,
        frame: StoreBulkData<'_>,
    ) -> Result<OwnedRequest<'static>, BulkTransferError> {
        let DirectionRuntime::Upload(runtime) = self.runtime_mut()? else {
            return Err(BulkTransferError::WrongDirection);
        };
        let owner = runtime.admit(frame)?;
        self.record(&owner, OwnerOperationKind::UploadWrite)?;
        Ok(owner)
    }

    pub fn begin_read(&mut self) -> Result<OwnedRequest<'static>, BulkTransferError> {
        let DirectionRuntime::Readback(runtime) = self.runtime_mut()? else {
            return Err(BulkTransferError::WrongDirection);
        };
        let owner = runtime.begin_read()?;
        self.record(&owner, OwnerOperationKind::ReadbackRead)?;
        Ok(owner)
    }

    pub fn complete_owner(
        &mut self,
        response: OwnedResponse<'static>,
    ) -> Result<BulkOwnerEvent, BulkTransferError> {
        if response.envelope.client != StorageClient::Diagnostic {
            return Err(BulkTransferError::Correlation);
        }
        let index = self
            .pending
            .iter()
            .position(|item| item.sequence == response.envelope.sequence)
            .ok_or(BulkTransferError::Correlation)?;
        let kind = self.pending.remove(index).kind;
        match (kind, self.runtime_mut()?) {
            (OwnerOperationKind::UploadOpen, DirectionRuntime::Upload(runtime)) => {
                let event = BulkOwnerEvent::Opened(runtime.complete_open(response)?);
                self.release_if_quiescent();
                Ok(event)
            }
            (OwnerOperationKind::UploadWrite, DirectionRuntime::Upload(runtime)) => {
                let event = BulkOwnerEvent::UploadCredit(runtime.complete_write(response)?);
                self.release_if_quiescent();
                Ok(event)
            }
            (OwnerOperationKind::UploadFinish, DirectionRuntime::Upload(runtime)) => {
                let finish = runtime.complete_finish(response)?;
                let event = BulkOwnerEvent::UploadFinished {
                    response: finish.response,
                    published: finish.published,
                };
                self.release_if_quiescent();
                Ok(event)
            }
            (OwnerOperationKind::UploadAbort, DirectionRuntime::Upload(runtime)) => {
                runtime.complete_abort(response)?;
                self.release_if_quiescent();
                Ok(BulkOwnerEvent::UploadAborted)
            }
            (OwnerOperationKind::ReadbackOpen, DirectionRuntime::Readback(runtime)) => {
                Ok(BulkOwnerEvent::Opened(runtime.complete_open(response)?))
            }
            _ => Err(BulkTransferError::WrongDirection),
        }
    }

    pub fn complete_read_owner(
        &mut self,
        response: OwnedResponse<'static>,
    ) -> Result<Option<StoreBulkData<'_>>, BulkTransferError> {
        if response.envelope.client != StorageClient::Diagnostic {
            return Err(BulkTransferError::Correlation);
        }
        let index = self
            .pending
            .iter()
            .position(|item| item.sequence == response.envelope.sequence)
            .ok_or(BulkTransferError::Correlation)?;
        if self.pending[index].kind != OwnerOperationKind::ReadbackRead {
            return Err(BulkTransferError::Correlation);
        }
        self.pending.remove(index);
        let DirectionRuntime::Readback(runtime) = self.runtime_mut()? else {
            return Err(BulkTransferError::WrongDirection);
        };
        runtime.complete_read(response).map_err(Into::into)
    }

    pub fn pending_owner(&self) -> usize {
        self.pending.len()
    }

    pub fn pending_owner_is_read(&self, sequence: u32) -> bool {
        self.pending.iter().any(|operation| {
            operation.sequence == sequence && operation.kind == OwnerOperationKind::ReadbackRead
        })
    }

    pub fn pending_owner_is_upload_write(&self, sequence: u32) -> bool {
        self.pending.iter().any(|operation| {
            operation.sequence == sequence && operation.kind == OwnerOperationKind::UploadWrite
        })
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.runtime, Some(DirectionRuntime::Idle(_)))
    }

    pub fn available_leases(&self) -> u8 {
        match self.runtime.as_ref() {
            Some(DirectionRuntime::Idle(pool)) => pool.available(),
            Some(DirectionRuntime::Upload(runtime)) => runtime.available_leases(),
            Some(DirectionRuntime::Readback(runtime)) => runtime.available_leases(),
            None => 0,
        }
    }

    pub fn reap_quiescent(&mut self) {
        self.release_if_quiescent();
    }

    pub(crate) fn runtime_mut(&mut self) -> Result<&mut DirectionRuntime, BulkTransferError> {
        self.runtime.as_mut().ok_or(BulkTransferError::Active)
    }

    pub(crate) fn record(
        &mut self,
        request: &OwnedRequest<'_>,
        kind: OwnerOperationKind,
    ) -> Result<(), BulkTransferError> {
        self.pending
            .push(PendingOwnerOperation {
                sequence: request.envelope.sequence,
                kind,
            })
            .map_err(|_| BulkTransferError::QueueFull)
    }

    pub(crate) fn release_if_quiescent(&mut self) {
        if !self.pending.is_empty() {
            return;
        }
        let Some(state) = self.runtime.take() else {
            return;
        };
        self.runtime = Some(match state {
            DirectionRuntime::Upload(runtime) if runtime.lease_pool_releasable() => {
                DirectionRuntime::Idle(runtime.into_lease_pool())
            }
            DirectionRuntime::Readback(runtime) if runtime.lease_pool_releasable() => {
                DirectionRuntime::Idle(runtime.into_lease_pool())
            }
            state => state,
        });
    }
}
