use std::io::{Read, Write};
use std::time::Instant;

use binbook_diagnostic_protocol::{
    decode_store_bulk_data, encode_frame, encode_store_bulk_credit, FrameHeader, FrameKind, Opcode,
    Status, StoreBulkCredit, StoreBulkFinishResponse, StoreBulkOpenRequest, StoreBulkOpenResponse,
    StoreBulkStatusResponse, MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES,
};

use crate::{
    bulk_readback_stream::{BulkReadbackTransport, DataPollError, OwnedBulkData},
    bulk_serial::BulkSerial,
    bulk_upload::BulkUploadTransport,
    storage_readback::ReadbackError,
};

impl<T: Read + Write + ?Sized> BulkReadbackTransport for BulkSerial<'_, T> {
    fn open(
        &mut self,
        request: StoreBulkOpenRequest<'_>,
    ) -> Result<StoreBulkOpenResponse, ReadbackError> {
        BulkUploadTransport::open(self, request).map_err(protocol_error)
    }

    fn poll_data(&mut self) -> Result<OwnedBulkData, DataPollError> {
        let deadline = Instant::now() + self.timeout;
        loop {
            match self.router.take_bulk_event(Opcode::StoreBulkData) {
                Ok(Some(frame)) => {
                    let data = decode_store_bulk_data(&frame.payload)
                        .map_err(|error| DataPollError::Transport(format!("{error:?}")))?;
                    return Ok(OwnedBulkData {
                        stream_id: data.stream_id,
                        offset: data.offset,
                        end_of_data: data.end_of_data,
                        data: data.data.to_vec(),
                    });
                }
                Ok(None) => {}
                Err(error) => return Err(DataPollError::Transport(error.to_string())),
            }
            if Instant::now() >= deadline {
                return Err(DataPollError::Timeout);
            }
            self.io
                .flush()
                .map_err(|error| DataPollError::Transport(error.to_string()))?;
            self.read_once().map_err(DataPollError::Transport)?;
        }
    }

    fn send_credit(&mut self, credit: StoreBulkCredit) -> Result<(), ReadbackError> {
        let mut payload = [0; MAX_PAYLOAD_BYTES];
        let payload_len = encode_store_bulk_credit(&credit, &mut payload).map_err(codec_error)?;
        let mut encoded = [0; MAX_FRAME_BYTES];
        let encoded_len = encode_frame(
            &FrameHeader {
                kind: FrameKind::Event,
                opcode: Opcode::StoreBulkCredit,
                status: Status::Ok,
                sequence: 0,
                payload_len: payload_len as u16,
            },
            &payload[..payload_len],
            &mut encoded,
        )
        .map_err(codec_error)?;
        self.io
            .write_all(&encoded[..encoded_len])
            .map_err(ReadbackError::Output)
    }

    fn status(&mut self, stream_id: u32) -> Result<StoreBulkStatusResponse, ReadbackError> {
        BulkUploadTransport::status(self, stream_id).map_err(protocol_error)
    }

    fn finish(&mut self, stream_id: u32) -> Result<StoreBulkFinishResponse, ReadbackError> {
        BulkUploadTransport::finish(self, stream_id).map_err(protocol_error)
    }

    fn abort(&mut self, stream_id: u32) -> bool {
        BulkUploadTransport::abort(self, stream_id)
    }
}

fn codec_error(error: binbook_diagnostic_protocol::ProtocolError) -> ReadbackError {
    ReadbackError::Protocol(format!("bulk codec failed: {error:?}"))
}

fn protocol_error(error: crate::upload::UploadError) -> ReadbackError {
    ReadbackError::Protocol(error.to_string())
}
