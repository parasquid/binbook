use binbook_diagnostic_protocol::{
    StoreBulkCredit, StoreBulkFinishResponse, StoreBulkOpenRequest, StoreBulkOpenResponse,
    StoreBulkStatusResponse,
};

use crate::storage_readback::ReadbackError;

#[derive(Debug)]
pub enum DataPollError {
    Timeout,
    Transport(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedBulkData {
    pub stream_id: u32,
    pub offset: u32,
    pub end_of_data: bool,
    pub data: Vec<u8>,
}

pub trait BulkReadbackTransport {
    fn open(
        &mut self,
        request: StoreBulkOpenRequest<'_>,
    ) -> Result<StoreBulkOpenResponse, ReadbackError>;
    fn poll_data(&mut self) -> Result<OwnedBulkData, DataPollError>;
    fn send_credit(&mut self, credit: StoreBulkCredit) -> Result<(), ReadbackError>;
    fn status(&mut self, stream_id: u32) -> Result<StoreBulkStatusResponse, ReadbackError>;
    fn finish(&mut self, stream_id: u32) -> Result<StoreBulkFinishResponse, ReadbackError>;
    fn abort(&mut self, stream_id: u32) -> bool;
}
