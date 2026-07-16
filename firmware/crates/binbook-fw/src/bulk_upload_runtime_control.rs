use binbook_diagnostic_protocol::{StoreBulkStatusResponse, StoreUploadCommitResult};

use crate::{
    bulk_upload_runtime::BulkUploadRuntime,
    bulk_upload_runtime_types::{
        owned_request, validate_response, BulkUploadFinish, BulkUploadRuntimeError, PendingFinish,
    },
    storage_owner::{OwnedRequest, OwnedResponse, Request, Response},
};

impl BulkUploadRuntime {
    pub fn status(
        &mut self,
        stream_id: u32,
    ) -> Result<StoreBulkStatusResponse, BulkUploadRuntimeError> {
        self.pipeline.status(stream_id).map_err(Into::into)
    }

    pub fn begin_abort(
        &mut self,
        stream_id: u32,
    ) -> Result<OwnedRequest<'static>, BulkUploadRuntimeError> {
        if self.pending_abort.is_some() || self.pending_finish.is_some() {
            return Err(BulkUploadRuntimeError::Busy);
        }
        self.pipeline.abort(stream_id)?;
        self.leases.invalidate(self.pipeline.generation());
        let sequence = self.sequence();
        self.pending_abort = Some(sequence);
        Ok(owned_request(
            sequence,
            Request::UploadAbort {
                upload_id: stream_id,
            },
            None,
        ))
    }

    pub fn complete_abort(
        &mut self,
        response: OwnedResponse<'static>,
    ) -> Result<(), BulkUploadRuntimeError> {
        let sequence = self
            .pending_abort
            .ok_or(BulkUploadRuntimeError::Correlation)?;
        validate_response(&response, sequence)?;
        if response.buffer.is_some() {
            return Err(BulkUploadRuntimeError::Correlation);
        }
        match response.envelope.response {
            Ok(Response::Ack) => {
                self.pending_abort = None;
                Ok(())
            }
            Ok(_) => Err(BulkUploadRuntimeError::Correlation),
            Err(_) => Err(BulkUploadRuntimeError::Owner),
        }
    }

    pub fn begin_finish(
        &mut self,
        stream_id: u32,
    ) -> Result<OwnedRequest<'static>, BulkUploadRuntimeError> {
        if self.pending_finish.is_some() || !self.outstanding.is_empty() {
            return Err(BulkUploadRuntimeError::Busy);
        }
        let response = self.pipeline.finish(stream_id)?;
        let published = response.result == StoreUploadCommitResult::Ok;
        let request = if published {
            Request::upload_commit(stream_id, false)
        } else {
            Request::UploadAbort {
                upload_id: stream_id,
            }
        };
        let sequence = self.sequence();
        self.pending_finish = Some(PendingFinish {
            sequence,
            response,
            published,
        });
        Ok(owned_request(sequence, request, None))
    }

    pub fn complete_finish(
        &mut self,
        response: OwnedResponse<'static>,
    ) -> Result<BulkUploadFinish, BulkUploadRuntimeError> {
        let pending = self
            .pending_finish
            .ok_or(BulkUploadRuntimeError::Correlation)?;
        validate_response(&response, pending.sequence)?;
        if response.buffer.is_some() {
            return Err(BulkUploadRuntimeError::Correlation);
        }
        match response.envelope.response {
            Ok(Response::Ack) => {}
            Ok(_) => return Err(BulkUploadRuntimeError::Correlation),
            Err(_) => return Err(BulkUploadRuntimeError::Owner),
        }
        self.pending_finish = None;
        Ok(BulkUploadFinish {
            response: pending.response,
            published: pending.published,
        })
    }
}
