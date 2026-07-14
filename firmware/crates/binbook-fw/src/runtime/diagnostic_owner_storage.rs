use binbook_diagnostic_protocol::{
    encode_store_entry, encode_store_read_response, encode_store_upload_begin_response,
    encode_store_upload_commit_response, encode_store_upload_write_response, FrameHeader, Status,
    StoreUploadCommitResult, MAX_PAYLOAD_BYTES,
};
use binbook_fw::{
    diag::{queue_runtime_response, SerialState},
    diag_owner_storage::{DiagnosticUpload, OwnerStorageCommand},
    library::{
        Library, LibraryIndexCursor, LibraryIndexCursorValidator, INDEX_HEADER_SIZE,
        INDEX_RECORD_SIZE,
    },
    storage_access::LIBRARY_INDEX_FILE,
    storage_owner::{
        BufferLease, BufferSlot, CompletionError, OwnedRequest, OwnedResponse, Request,
        RequestEnvelope, Response, StorageClient, StorageName, MAX_BOUNDED_IO_BYTES,
    },
};

use super::{
    LibraryRefreshTrigger, LIBRARY_REFRESH_TRIGGER_CHANNEL, STORAGE_DIAGNOSTIC_RESPONSE_CHANNEL,
    STORAGE_REQUEST_CHANNEL,
};

pub(super) struct DiagnosticOwnerRuntime {
    sequence: u32,
    upload: Option<DiagnosticUpload>,
}

impl DiagnosticOwnerRuntime {
    pub(super) const fn new() -> Self {
        Self {
            sequence: 250_000,
            upload: None,
        }
    }

    fn next_sequence(&mut self) -> u32 {
        let sequence = self.sequence;
        self.sequence = self.sequence.wrapping_add(1);
        sequence
    }
}

pub(super) async fn handle_owner_storage_command(
    serial: &mut SerialState,
    runtime: &mut DiagnosticOwnerRuntime,
    header: FrameHeader,
    command: OwnerStorageCommand,
    mut transfer: &'static mut [u8; MAX_BOUNDED_IO_BYTES as usize],
    response_payload: &mut [u8; MAX_PAYLOAD_BYTES],
) -> &'static mut [u8; MAX_BOUNDED_IO_BYTES as usize] {
    response_payload.fill(0);
    let (status, payload_len) = match command {
        OwnerStorageCommand::List { start_index } => {
            let result = list_index(runtime, start_index, transfer, response_payload).await;
            transfer = result.2;
            (result.0, result.1)
        }
        OwnerStorageCommand::Read {
            name,
            offset,
            max_bytes,
        } => {
            let size = exchange(runtime, Request::FileSize { name: name.clone() }, None).await;
            match size.envelope.response {
                Ok(Response::FileSize(file_size)) if u64::from(offset) <= file_size => {
                    let available = file_size - u64::from(offset);
                    let len = available
                        .min(u64::from(max_bytes))
                        .min((MAX_PAYLOAD_BYTES - 5) as u64)
                        .min(u64::from(MAX_BOUNDED_IO_BYTES)) as u16;
                    if len == 0 {
                        match encode_store_read_response(&[], true, response_payload) {
                            Ok(wrote) => (Status::Ok, wrote),
                            Err(_) => (Status::InternalError, 0),
                        }
                    } else {
                        let request =
                            Request::read(name, u64::from(offset), len, BufferSlot::Diagnostic)
                                .expect("diagnostic read is bounded");
                        let read = exchange(
                            runtime,
                            request,
                            Some(BufferLease::new(BufferSlot::Diagnostic, transfer)),
                        )
                        .await;
                        transfer = take_transfer(read.buffer);
                        match read.envelope.response {
                            Ok(Response::Read(actual)) if actual == len => {
                                let eof = u64::from(offset) + u64::from(actual) >= file_size;
                                match encode_store_read_response(
                                    &transfer[..usize::from(actual)],
                                    eof,
                                    response_payload,
                                ) {
                                    Ok(wrote) => (Status::Ok, wrote),
                                    Err(_) => (Status::InternalError, 0),
                                }
                            }
                            _ => (Status::Error, 0),
                        }
                    }
                }
                Ok(Response::FileSize(_)) => (Status::BadRequest, 0),
                _ => (Status::NotFound, 0),
            }
        }
        OwnerStorageCommand::Delete { name } => {
            let deleted = exchange(runtime, Request::Delete { name }, None).await;
            if matches!(deleted.envelope.response, Ok(Response::Ack)) {
                request_library_refresh().await;
                (Status::Ok, 0)
            } else {
                (Status::NotFound, 0)
            }
        }
        OwnerStorageCommand::UploadBegin {
            name,
            file_size,
            expected_crc32,
        } => {
            if runtime.upload.is_some() {
                (Status::BadRequest, 0)
            } else {
                let started = exchange(
                    runtime,
                    Request::UploadBegin {
                        name,
                        expected_size: u64::from(file_size),
                        power_save: false,
                    },
                    None,
                )
                .await;
                match started.envelope.response {
                    Ok(Response::UploadStarted(upload_id)) => {
                        runtime.upload =
                            Some(DiagnosticUpload::new(upload_id, file_size, expected_crc32));
                        match encode_store_upload_begin_response(upload_id, response_payload) {
                            Ok(wrote) => (Status::Ok, wrote),
                            Err(_) => (Status::InternalError, 0),
                        }
                    }
                    _ => (Status::Error, 0),
                }
            }
        }
        OwnerStorageCommand::UploadWrite {
            upload_id,
            offset,
            len,
        } => {
            let Some(mut next_upload) = runtime.upload else {
                queue_runtime_response(serial, header, Status::BadRequest, &[]);
                return transfer;
            };
            if next_upload
                .accept(upload_id, offset, &transfer[..usize::from(len)])
                .is_err()
            {
                (Status::BadRequest, 0)
            } else {
                let written = exchange(
                    runtime,
                    Request::UploadWrite {
                        upload_id,
                        offset: u64::from(offset),
                        buffer_offset: 0,
                        len,
                        buffer: BufferSlot::Diagnostic,
                        power_save: false,
                    },
                    Some(BufferLease::new(BufferSlot::Diagnostic, transfer)),
                )
                .await;
                transfer = take_transfer(written.buffer);
                match written.envelope.response {
                    Ok(Response::Written(actual)) if actual == len => {
                        runtime.upload = Some(next_upload);
                        match encode_store_upload_write_response(
                            u32::from(actual),
                            response_payload,
                        ) {
                            Ok(wrote) => (Status::Ok, wrote),
                            Err(_) => (Status::InternalError, 0),
                        }
                    }
                    _ => (Status::Error, 0),
                }
            }
        }
        OwnerStorageCommand::UploadCommit { upload_id } => {
            let Some(upload) = runtime.upload else {
                queue_runtime_response(serial, header, Status::BadRequest, &[]);
                return transfer;
            };
            match upload.finish(upload_id) {
                Ok((crc_matches, computed_crc32)) => {
                    let committed =
                        exchange(runtime, Request::upload_commit(upload_id, false), None).await;
                    if matches!(committed.envelope.response, Ok(Response::Ack)) {
                        runtime.upload = None;
                        request_library_refresh().await;
                        let result = if crc_matches {
                            StoreUploadCommitResult::Ok
                        } else {
                            StoreUploadCommitResult::CrcMismatch
                        };
                        match encode_store_upload_commit_response(
                            result,
                            computed_crc32,
                            response_payload,
                        ) {
                            Ok(wrote) => (Status::Ok, wrote),
                            Err(_) => (Status::InternalError, 0),
                        }
                    } else {
                        (Status::Error, 0)
                    }
                }
                Err(_) => (Status::BadRequest, 0),
            }
        }
        OwnerStorageCommand::UploadAbort { upload_id } => {
            if runtime.upload.is_none() {
                (Status::BadRequest, 0)
            } else {
                let aborted = exchange(runtime, Request::UploadAbort { upload_id }, None).await;
                if matches!(aborted.envelope.response, Ok(Response::Ack)) {
                    runtime.upload = None;
                    (Status::Ok, 0)
                } else {
                    (Status::Error, 0)
                }
            }
        }
    };
    queue_runtime_response(serial, header, status, &response_payload[..payload_len]);
    transfer
}

async fn list_index(
    runtime: &mut DiagnosticOwnerRuntime,
    start_index: u16,
    mut transfer: &'static mut [u8; MAX_BOUNDED_IO_BYTES as usize],
    response_payload: &mut [u8; MAX_PAYLOAD_BYTES],
) -> (
    Status,
    usize,
    &'static mut [u8; MAX_BOUNDED_IO_BYTES as usize],
) {
    let index_name = StorageName::new(LIBRARY_INDEX_FILE).expect("index name is bounded");
    let size = exchange(
        runtime,
        Request::FileSize {
            name: index_name.clone(),
        },
        None,
    )
    .await;
    let file_len = match size.envelope.response {
        Ok(Response::FileSize(value)) => match usize::try_from(value) {
            Ok(value) if (INDEX_HEADER_SIZE..=Library::max_index_bytes()).contains(&value) => value,
            _ => return (Status::Error, 0, transfer),
        },
        _ => return (Status::NotFound, 0, transfer),
    };

    let header_read = Request::read(
        index_name.clone(),
        0,
        INDEX_HEADER_SIZE as u16,
        BufferSlot::Diagnostic,
    )
    .expect("index header is bounded");
    let header_response = exchange(
        runtime,
        header_read,
        Some(BufferLease::new(BufferSlot::Diagnostic, transfer)),
    )
    .await;
    transfer = take_transfer(header_response.buffer);
    if !matches!(
        header_response.envelope.response,
        Ok(Response::Read(read)) if usize::from(read) == INDEX_HEADER_SIZE
    ) {
        return (Status::Error, 0, transfer);
    }
    let header: &[u8; INDEX_HEADER_SIZE] = match transfer[..INDEX_HEADER_SIZE].try_into() {
        Ok(header) => header,
        Err(_) => return (Status::InternalError, 0, transfer),
    };
    let cursor = match LibraryIndexCursor::from_header(header, file_len) {
        Ok(cursor) => cursor,
        Err(_) => return (Status::Error, 0, transfer),
    };
    let mut validator = LibraryIndexCursorValidator::new(cursor, header);
    response_payload[..2].fill(0);
    let mut response_len = 2usize;
    let mut encoded_count = 0u16;
    let mut encode_failed = false;
    let mut offset = INDEX_HEADER_SIZE;
    while offset < file_len {
        let remaining_records = (file_len - offset) / INDEX_RECORD_SIZE;
        let record_count =
            remaining_records.min((MAX_BOUNDED_IO_BYTES as usize / INDEX_RECORD_SIZE).max(1));
        let len = record_count * INDEX_RECORD_SIZE;
        let read = Request::read(
            index_name.clone(),
            offset as u64,
            len as u16,
            BufferSlot::Diagnostic,
        )
        .expect("index record chunk is bounded");
        let response = exchange(
            runtime,
            read,
            Some(BufferLease::new(BufferSlot::Diagnostic, transfer)),
        )
        .await;
        transfer = take_transfer(response.buffer);
        if !matches!(response.envelope.response, Ok(Response::Read(actual)) if usize::from(actual) == len)
        {
            return (Status::Error, 0, transfer);
        }
        for (local_index, record) in transfer[..len].chunks_exact(INDEX_RECORD_SIZE).enumerate() {
            let record: &[u8; INDEX_RECORD_SIZE] = record.try_into().unwrap();
            if validator.push_record(record).is_err() {
                return (Status::Error, 0, transfer);
            }
            let record_index = (offset - INDEX_HEADER_SIZE) / INDEX_RECORD_SIZE + local_index;
            if record_index >= usize::from(start_index) && !encode_failed {
                let entry = match cursor.decode_record(record) {
                    Ok(entry) => entry,
                    Err(_) => return (Status::Error, 0, transfer),
                };
                match encode_store_entry(
                    entry.name.as_bytes(),
                    entry.size,
                    0,
                    &mut response_payload[response_len..],
                ) {
                    Ok(wrote) => {
                        response_len += wrote;
                        encoded_count = encoded_count.saturating_add(1);
                    }
                    Err(_) => encode_failed = true,
                }
            }
        }
        offset += len;
    }
    if validator.finish().is_err() || (encode_failed && encoded_count == 0) {
        return (Status::Error, 0, transfer);
    }
    response_payload[..2].copy_from_slice(&encoded_count.to_le_bytes());
    (Status::Ok, response_len, transfer)
}

async fn exchange(
    runtime: &mut DiagnosticOwnerRuntime,
    request: Request,
    buffer: Option<BufferLease<'static>>,
) -> OwnedResponse<'static> {
    let sequence = runtime.next_sequence();
    STORAGE_REQUEST_CHANNEL
        .send(OwnedRequest {
            envelope: RequestEnvelope {
                client: StorageClient::Diagnostic,
                sequence,
                request,
            },
            buffer,
        })
        .await;
    let response = STORAGE_DIAGNOSTIC_RESPONSE_CHANNEL.receive().await;
    if response.envelope.client != StorageClient::Diagnostic
        || response.envelope.sequence != sequence
    {
        return OwnedResponse {
            envelope: binbook_fw::storage_owner::ResponseEnvelope {
                client: StorageClient::Diagnostic,
                sequence,
                response: Err(CompletionError::Request(
                    binbook_fw::storage_owner::RequestError::Stale,
                )),
            },
            buffer: response.buffer,
        };
    }
    response
}

fn take_transfer(
    lease: Option<BufferLease<'static>>,
) -> &'static mut [u8; MAX_BOUNDED_IO_BYTES as usize] {
    lease
        .expect("diagnostic owner must return its caller-owned buffer")
        .into_bytes()
        .try_into()
        .expect("diagnostic transfer buffer keeps its fixed size")
}

async fn request_library_refresh() {
    LIBRARY_REFRESH_TRIGGER_CHANNEL
        .send(LibraryRefreshTrigger {
            prefetch: heapless::Vec::new(),
        })
        .await;
}
