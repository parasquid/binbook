use binbook_diagnostic_protocol::{
    StoreBulkCredit, StoreBulkFinishResponse, StoreBulkStatusResponse, TransferDirection,
};

use crate::{
    bulk_transfer_coordinator::BulkTransferCoordinator,
    bulk_transfer_types::{BulkTransferError, DirectionRuntime, OwnerOperationKind},
    storage_owner::OwnedRequest,
};

impl BulkTransferCoordinator {
    pub fn direction(&self) -> TransferDirection {
        match self.runtime.as_ref() {
            Some(DirectionRuntime::Upload(_)) => TransferDirection::Upload,
            Some(DirectionRuntime::Readback(_)) => TransferDirection::Readback,
            _ => TransferDirection::Unspecified,
        }
    }

    pub fn begin_upload_abort(
        &mut self,
        stream_id: u32,
    ) -> Result<OwnedRequest<'static>, BulkTransferError> {
        let DirectionRuntime::Upload(runtime) = self.runtime_mut()? else {
            return Err(BulkTransferError::WrongDirection);
        };
        let owner = runtime.begin_abort(stream_id)?;
        self.record(&owner, OwnerOperationKind::UploadAbort)?;
        Ok(owner)
    }

    pub fn begin_upload_finish(
        &mut self,
        stream_id: u32,
    ) -> Result<OwnedRequest<'static>, BulkTransferError> {
        let DirectionRuntime::Upload(runtime) = self.runtime_mut()? else {
            return Err(BulkTransferError::WrongDirection);
        };
        let owner = runtime.begin_finish(stream_id)?;
        self.record(&owner, OwnerOperationKind::UploadFinish)?;
        Ok(owner)
    }

    pub fn apply_readback_credit(
        &mut self,
        credit: StoreBulkCredit,
    ) -> Result<(), BulkTransferError> {
        let DirectionRuntime::Readback(runtime) = self.runtime_mut()? else {
            return Err(BulkTransferError::WrongDirection);
        };
        runtime.apply_credit(credit).map_err(Into::into)
    }

    pub fn status(&mut self, stream_id: u32) -> Result<StoreBulkStatusResponse, BulkTransferError> {
        match self.runtime_mut()? {
            DirectionRuntime::Upload(runtime) => runtime.status(stream_id).map_err(Into::into),
            DirectionRuntime::Readback(runtime) => runtime.status(stream_id).map_err(Into::into),
            DirectionRuntime::Idle(_) => Err(BulkTransferError::WrongDirection),
        }
    }

    pub fn finish_readback(
        &mut self,
        stream_id: u32,
    ) -> Result<StoreBulkFinishResponse, BulkTransferError> {
        let DirectionRuntime::Readback(runtime) = self.runtime_mut()? else {
            return Err(BulkTransferError::WrongDirection);
        };
        let response = runtime.finish(stream_id)?;
        self.release_if_quiescent();
        Ok(response)
    }

    pub fn abort_readback(&mut self, stream_id: u32) -> Result<(), BulkTransferError> {
        let DirectionRuntime::Readback(runtime) = self.runtime_mut()? else {
            return Err(BulkTransferError::WrongDirection);
        };
        runtime.abort(stream_id)?;
        self.release_if_quiescent();
        Ok(())
    }

    pub fn begin_timeout_cleanup(
        &mut self,
    ) -> Result<Option<OwnedRequest<'static>>, BulkTransferError> {
        let stream_id = match self.runtime.as_ref() {
            Some(DirectionRuntime::Upload(runtime)) => runtime.stream_id(),
            Some(DirectionRuntime::Readback(runtime)) => runtime.stream_id(),
            _ => None,
        }
        .ok_or(BulkTransferError::Active)?;
        match self.direction() {
            TransferDirection::Upload => self.begin_upload_abort(stream_id).map(Some),
            TransferDirection::Readback => {
                self.abort_readback(stream_id)?;
                Ok(None)
            }
            TransferDirection::Unspecified => Err(BulkTransferError::WrongDirection),
        }
    }
}
