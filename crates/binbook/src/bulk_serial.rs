use std::io::{Read, Write};
use std::time::{Duration, Instant};

use binbook_diagnostic_protocol::{
    decode_store_bulk_abort_request, decode_store_bulk_credit, decode_store_bulk_finish_response,
    decode_store_bulk_open_response, decode_store_bulk_status_response, decode_store_list_count,
    decode_store_list_entry, encode_frame, encode_store_bulk_abort_request, encode_store_bulk_data,
    encode_store_bulk_finish_request, encode_store_bulk_open_request,
    encode_store_bulk_status_request, encode_store_list_request, FrameHeader, FrameKind, Opcode,
    Status, StorageBackend, StoreBulkCredit, StoreBulkData, StoreBulkFinishResponse,
    StoreBulkOpenRequest, StoreBulkOpenResponse, StoreBulkStatusResponse, StoreListRequest,
    MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES, STORE_LIST_ENTRY_HEADER_BYTES,
};

use crate::{
    bulk_upload::{BulkUploadTransport, CreditPollError},
    frame_router::{FrameRouter, RoutedFrame},
    upload::UploadError,
    upload_library::is_transient_library_scan_error,
};

pub struct BulkSerial<'a, T: Read + Write + ?Sized> {
    pub(crate) io: &'a mut T,
    pub(crate) router: FrameRouter,
    next_sequence: u16,
    pub(crate) timeout: Duration,
    status_queries: u32,
    retries: u32,
}

impl<'a, T: Read + Write + ?Sized> BulkSerial<'a, T> {
    pub fn new(io: &'a mut T, timeout: Duration) -> Self {
        Self {
            io,
            router: FrameRouter::new(),
            next_sequence: 1,
            timeout,
            status_queries: 0,
            retries: 0,
        }
    }

    pub fn recovery_metrics(&self) -> (u32, u32) {
        (self.status_queries, self.retries)
    }

    pub(crate) fn request(
        &mut self,
        opcode: Opcode,
        encode: impl FnOnce(&mut [u8]) -> Result<usize, binbook_diagnostic_protocol::ProtocolError>,
    ) -> Result<RoutedFrame, UploadError> {
        let sequence = self.take_sequence();
        let mut payload = [0; MAX_PAYLOAD_BYTES];
        let payload_len = encode(&mut payload)?;
        let mut encoded = [0; MAX_FRAME_BYTES];
        let encoded_len = encode_frame(
            &FrameHeader {
                kind: FrameKind::Request,
                opcode,
                status: Status::Ok,
                sequence,
                payload_len: payload_len as u16,
            },
            &payload[..payload_len],
            &mut encoded,
        )?;
        self.io
            .write_all(&encoded[..encoded_len])
            .map_err(UploadError::Io)?;
        self.io.flush().map_err(UploadError::Io)?;
        self.wait_response(opcode, sequence)
    }

    fn wait_response(&mut self, opcode: Opcode, sequence: u16) -> Result<RoutedFrame, UploadError> {
        let deadline = Instant::now() + self.timeout;
        loop {
            if let Some(frame) = self
                .router
                .take_response(opcode, sequence)
                .map_err(|error| UploadError::Response(error.to_string()))?
            {
                if frame.header.status != Status::Ok {
                    return Err(UploadError::DeviceStatus {
                        opcode,
                        sequence,
                        status: frame.header.status,
                        cleanup_confirmed: false,
                    });
                }
                return Ok(frame);
            }
            if Instant::now() >= deadline {
                return Err(UploadError::Transport {
                    message: format!("timeout waiting for {opcode:?} sequence {sequence}"),
                    cleanup_confirmed: false,
                });
            }
            self.read_once().map_err(|message| UploadError::Transport {
                message,
                cleanup_confirmed: false,
            })?;
        }
    }

    pub(crate) fn read_once(&mut self) -> Result<(), String> {
        let mut chunk = [0; 256];
        match self.io.read(&mut chunk) {
            Ok(0) => std::thread::yield_now(),
            Ok(count) => self
                .router
                .push(&chunk[..count])
                .map_err(|error| error.to_string())?,
            Err(error)
                if error.kind() == std::io::ErrorKind::TimedOut
                    || error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(format!("bulk serial read failed: {error}")),
        }
        Ok(())
    }

    fn take_sequence(&mut self) -> u16 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1).max(1);
        sequence
    }
}

impl<T: Read + Write + ?Sized> BulkUploadTransport for BulkSerial<'_, T> {
    fn open(
        &mut self,
        request: StoreBulkOpenRequest<'_>,
    ) -> Result<StoreBulkOpenResponse, UploadError> {
        let response = self.request(Opcode::StoreBulkOpen, |payload| {
            encode_store_bulk_open_request(&request, payload)
        })?;
        decode_store_bulk_open_response(&response.payload).map_err(Into::into)
    }

    fn send_data(&mut self, frame: StoreBulkData<'_>) -> Result<(), UploadError> {
        let mut payload = [0; MAX_PAYLOAD_BYTES];
        let payload_len = encode_store_bulk_data(&frame, &mut payload)?;
        let mut encoded = [0; MAX_FRAME_BYTES];
        let encoded_len = encode_frame(
            &FrameHeader {
                kind: FrameKind::Event,
                opcode: Opcode::StoreBulkData,
                status: Status::Ok,
                sequence: 0,
                payload_len: payload_len as u16,
            },
            &payload[..payload_len],
            &mut encoded,
        )?;
        self.io
            .write_all(&encoded[..encoded_len])
            .map_err(UploadError::Io)
    }

    fn poll_credit(&mut self) -> Result<StoreBulkCredit, CreditPollError> {
        let deadline = Instant::now() + self.timeout;
        loop {
            match self.router.take_bulk_event(Opcode::StoreBulkCredit) {
                Ok(Some(frame)) => {
                    return decode_store_bulk_credit(&frame.payload)
                        .map_err(|error| CreditPollError::Transport(format!("{error:?}")));
                }
                Ok(None) => {}
                Err(error) => return Err(CreditPollError::Transport(error.to_string())),
            }
            if Instant::now() >= deadline {
                return Err(CreditPollError::Timeout);
            }
            self.io
                .flush()
                .map_err(|error| CreditPollError::Transport(error.to_string()))?;
            self.read_once().map_err(CreditPollError::Transport)?;
        }
    }

    fn status(&mut self, stream_id: u32) -> Result<StoreBulkStatusResponse, UploadError> {
        self.status_queries = self.status_queries.saturating_add(1);
        self.retries = self.retries.saturating_add(1);
        let response = self.request(Opcode::StoreBulkStatus, |payload| {
            encode_store_bulk_status_request(stream_id, payload)
        })?;
        decode_store_bulk_status_response(&response.payload).map_err(Into::into)
    }

    fn finish(&mut self, stream_id: u32) -> Result<StoreBulkFinishResponse, UploadError> {
        let response = self.request(Opcode::StoreBulkFinish, |payload| {
            encode_store_bulk_finish_request(stream_id, payload)
        })?;
        decode_store_bulk_finish_response(&response.payload).map_err(Into::into)
    }

    fn abort(&mut self, stream_id: u32) -> bool {
        let Ok(response) = self.request(Opcode::StoreBulkAbort, |payload| {
            encode_store_bulk_abort_request(stream_id, payload)
        }) else {
            return false;
        };
        decode_store_bulk_abort_request(&response.payload) == Ok(stream_id)
    }

    fn wait_library(&mut self, name: &str, size: u64) -> Result<bool, UploadError> {
        'polls: for poll in 0..10 {
            let mut start_index = 0u16;
            loop {
                let response = match self.request(Opcode::StoreList, |payload| {
                    encode_store_list_request(
                        &StoreListRequest {
                            backend: StorageBackend::Sd,
                            start_index,
                        },
                        payload,
                    )
                }) {
                    Ok(response) => response,
                    Err(error) if is_transient_library_scan_error(&error) => {
                        if poll != 9 {
                            std::thread::sleep(Duration::from_secs(1));
                        }
                        continue 'polls;
                    }
                    Err(error) => return Err(error),
                };
                let count = decode_store_list_count(&response.payload)?;
                if count == 0 {
                    break;
                }
                let mut position = 2;
                for _ in 0..count {
                    let entry = decode_store_list_entry(&response.payload[position..])?;
                    position += STORE_LIST_ENTRY_HEADER_BYTES + entry.name.len();
                    if entry.name == name && entry.file_size == size {
                        return Ok(true);
                    }
                }
                start_index = start_index
                    .checked_add(count)
                    .ok_or_else(|| UploadError::LibraryWait("list cursor overflow".into()))?;
            }
            if poll != 9 {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        Ok(false)
    }
}
