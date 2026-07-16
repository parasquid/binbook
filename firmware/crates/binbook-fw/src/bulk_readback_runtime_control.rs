use binbook_diagnostic_protocol::{StoreBulkFinishResponse, StoreBulkStatusResponse};

use crate::{
    bulk_readback_runtime::BulkReadbackRuntime,
    bulk_readback_runtime_types::BulkReadbackRuntimeError,
};

impl BulkReadbackRuntime {
    pub fn status(
        &mut self,
        stream_id: u32,
    ) -> Result<StoreBulkStatusResponse, BulkReadbackRuntimeError> {
        let status = self.pipeline.status(stream_id)?;
        self.leases.invalidate(self.pipeline.generation());
        self.sent.clear();
        Ok(status)
    }

    pub fn finish(
        &mut self,
        stream_id: u32,
    ) -> Result<StoreBulkFinishResponse, BulkReadbackRuntimeError> {
        let response = self.pipeline.finish(stream_id)?;
        self.name = None;
        Ok(response)
    }

    pub fn abort(&mut self, stream_id: u32) -> Result<(), BulkReadbackRuntimeError> {
        self.pipeline.abort(stream_id)?;
        self.leases.invalidate(self.pipeline.generation());
        self.sent.clear();
        self.name = None;
        Ok(())
    }
}
