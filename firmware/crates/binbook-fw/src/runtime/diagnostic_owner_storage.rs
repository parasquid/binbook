use binbook_diagnostic_protocol::{
    encode_store_entry, encode_store_read_response, encode_store_upload_begin_response,
    encode_store_upload_commit_response, encode_store_upload_write_response, FrameHeader, Status,
    StoreUploadCommitResult, TransferDirection, TransferTraceState, MAX_PAYLOAD_BYTES,
};
use binbook_fw::{
    diag::{queue_runtime_response, SerialState},
    diag_owner_storage::{DiagnosticUpload, OwnerStorageCommand},
    legacy_transfer_trace::{trace_phase as record_trace_phase, LegacyTraceLifecycle},
    library::{
        select_library_index_cursor, LibraryIndexCursor, LibraryIndexCursorValidator,
        LibraryIndexSlot, INDEX_HEADER_SIZE, INDEX_RECORD_SIZE, INDEX_V2_HEADER_SIZE,
    },
    storage_access::{LIBRARY_INDEX_FILE, LIBRARY_INDEX_SLOT_A, LIBRARY_INDEX_SLOT_B},
    storage_owner::{
        BufferLease, BufferSlot, CompletionError, OwnedRequest, OwnedResponse, Request,
        RequestEnvelope, Response, StorageClient, StorageName, MAX_BOUNDED_IO_BYTES,
    },
};

use super::{
    LibraryRefreshTrigger, LIBRARY_REFRESH_BARRIER, LIBRARY_REFRESH_TRIGGER_CHANNEL,
    STORAGE_DIAGNOSTIC_RESPONSE_CHANNEL, STORAGE_REQUEST_CHANNEL,
};

pub(super) struct DiagnosticOwnerRuntime {
    sequence: u32,
    upload: Option<DiagnosticUpload>,
    read_file: Option<(StorageName, u64)>,
    trace_lifecycle: LegacyTraceLifecycle,
}

impl DiagnosticOwnerRuntime {
    pub(super) const fn new() -> Self {
        Self {
            sequence: 250_000,
            upload: None,
            read_file: None,
            trace_lifecycle: LegacyTraceLifecycle::new(),
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
    transfer_trace: &mut binbook_fw::transfer_trace_command::TransferTraceCommand,
) -> &'static mut [u8; MAX_BOUNDED_IO_BYTES as usize] {
    response_payload.fill(0);
    let (status, payload_len) = match command {
        OwnerStorageCommand::List { start_index } => {
            if LIBRARY_REFRESH_BARRIER.is_pending() {
                (Status::Error, 0)
            } else {
                let result = list_index(runtime, start_index, transfer, response_payload).await;
                transfer = result.2;
                (result.0, result.1)
            }
        }
        OwnerStorageCommand::Read {
            name,
            offset,
            max_bytes,
        } => {
            start_data_phase(runtime, TransferDirection::Readback, transfer_trace);
            match read_file_size(runtime, &name).await {
                Some(file_size) if u64::from(offset) <= file_size => {
                    let available = file_size - u64::from(offset);
                    let len = available
                        .min(u64::from(max_bytes))
                        .min((MAX_PAYLOAD_BYTES - 5) as u64)
                        .min(u64::from(MAX_BOUNDED_IO_BYTES)) as u16;
                    if len == 0 {
                        match encode_store_read_response(&[], true, response_payload) {
                            Ok(wrote) => {
                                finish_data_trace(
                                    runtime,
                                    TransferDirection::Readback,
                                    TransferTraceState::Succeeded,
                                    serial,
                                    transfer_trace,
                                    false,
                                );
                                (Status::Ok, wrote)
                            }
                            Err(_) => {
                                finish_data_trace(
                                    runtime,
                                    TransferDirection::Readback,
                                    TransferTraceState::Failed,
                                    serial,
                                    transfer_trace,
                                    false,
                                );
                                (Status::InternalError, 0)
                            }
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
                                let encoded = match encode_store_read_response(
                                    &transfer[..usize::from(actual)],
                                    eof,
                                    response_payload,
                                ) {
                                    Ok(wrote) => (Status::Ok, wrote),
                                    Err(_) => (Status::InternalError, 0),
                                };
                                if eof && encoded.0 == Status::Ok {
                                    finish_data_trace(
                                        runtime,
                                        TransferDirection::Readback,
                                        TransferTraceState::Succeeded,
                                        serial,
                                        transfer_trace,
                                        false,
                                    );
                                } else if encoded.0 != Status::Ok {
                                    finish_data_trace(
                                        runtime,
                                        TransferDirection::Readback,
                                        TransferTraceState::Failed,
                                        serial,
                                        transfer_trace,
                                        false,
                                    );
                                }
                                encoded
                            }
                            _ => {
                                finish_data_trace(
                                    runtime,
                                    TransferDirection::Readback,
                                    TransferTraceState::Failed,
                                    serial,
                                    transfer_trace,
                                    false,
                                );
                                (Status::Error, 0)
                            }
                        }
                    }
                }
                Some(_) => {
                    finish_data_trace(
                        runtime,
                        TransferDirection::Readback,
                        TransferTraceState::Failed,
                        serial,
                        transfer_trace,
                        false,
                    );
                    (Status::BadRequest, 0)
                }
                None => {
                    finish_data_trace(
                        runtime,
                        TransferDirection::Readback,
                        TransferTraceState::Failed,
                        serial,
                        transfer_trace,
                        false,
                    );
                    (Status::NotFound, 0)
                }
            }
        }
        OwnerStorageCommand::Delete { name } => {
            runtime.read_file = None;
            let deleted = exchange(runtime, Request::Delete { name }, None).await;
            if matches!(deleted.envelope.response, Ok(Response::Ack)) {
                request_library_refresh(None).await;
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
            runtime.read_file = None;
            if runtime.upload.is_some() {
                (Status::BadRequest, 0)
            } else {
                trace_phase(
                    transfer_trace,
                    TransferDirection::Upload,
                    binbook_fw::transfer_metrics::TransferPhase::UploadPrepare,
                    true,
                );
                let started = exchange(
                    runtime,
                    Request::UploadBegin {
                        name,
                        expected_size: u64::from(file_size),
                        expected_crc32,
                        power_save: false,
                    },
                    None,
                )
                .await;
                trace_phase(
                    transfer_trace,
                    TransferDirection::Upload,
                    binbook_fw::transfer_metrics::TransferPhase::UploadPrepare,
                    false,
                );
                match started.envelope.response {
                    Ok(Response::UploadStarted(upload_id)) => {
                        runtime.upload =
                            Some(DiagnosticUpload::new(upload_id, file_size, expected_crc32));
                        start_data_phase(runtime, TransferDirection::Upload, transfer_trace);
                        match encode_store_upload_begin_response(upload_id, response_payload) {
                            Ok(wrote) => (Status::Ok, wrote),
                            Err(_) => (Status::InternalError, 0),
                        }
                    }
                    _ => {
                        finish_data_trace(
                            runtime,
                            TransferDirection::Upload,
                            TransferTraceState::Failed,
                            serial,
                            transfer_trace,
                            false,
                        );
                        (Status::Error, 0)
                    }
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
            trace_phase(
                transfer_trace,
                TransferDirection::Upload,
                binbook_fw::transfer_metrics::TransferPhase::Crc,
                true,
            );
            let accepted = next_upload
                .accept(upload_id, offset, &transfer[..usize::from(len)])
                .is_ok();
            trace_phase(
                transfer_trace,
                TransferDirection::Upload,
                binbook_fw::transfer_metrics::TransferPhase::Crc,
                false,
            );
            if !accepted {
                (Status::BadRequest, 0)
            } else {
                trace_phase(
                    transfer_trace,
                    TransferDirection::Upload,
                    binbook_fw::transfer_metrics::TransferPhase::DataIo,
                    true,
                );
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
                trace_phase(
                    transfer_trace,
                    TransferDirection::Upload,
                    binbook_fw::transfer_metrics::TransferPhase::DataIo,
                    false,
                );
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
                    end_data_phase(runtime, TransferDirection::Upload, transfer_trace);
                    if !crc_matches {
                        finish_trace(TransferTraceState::Failed, serial, transfer_trace, false);
                        match encode_store_upload_commit_response(
                            StoreUploadCommitResult::CrcMismatch,
                            computed_crc32,
                            response_payload,
                        ) {
                            Ok(wrote) => (Status::Ok, wrote),
                            Err(_) => (Status::InternalError, 0),
                        }
                    } else {
                        trace_phase(
                            transfer_trace,
                            TransferDirection::Upload,
                            binbook_fw::transfer_metrics::TransferPhase::Commit,
                            true,
                        );
                        let committed =
                            exchange(runtime, Request::upload_commit(upload_id, false), None).await;
                        trace_phase(
                            transfer_trace,
                            TransferDirection::Upload,
                            binbook_fw::transfer_metrics::TransferPhase::Commit,
                            false,
                        );
                        if matches!(committed.envelope.response, Ok(Response::Ack)) {
                            runtime.upload = None;
                            let trace_id = transfer_trace.active_trace_id();
                            finish_trace(
                                TransferTraceState::Succeeded,
                                serial,
                                transfer_trace,
                                trace_id.is_some(),
                            );
                            request_library_refresh(trace_id).await;
                            match encode_store_upload_commit_response(
                                StoreUploadCommitResult::Ok,
                                computed_crc32,
                                response_payload,
                            ) {
                                Ok(wrote) => (Status::Ok, wrote),
                                Err(_) => (Status::InternalError, 0),
                            }
                        } else {
                            finish_trace(TransferTraceState::Failed, serial, transfer_trace, false);
                            (Status::Error, 0)
                        }
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
                    finish_data_trace(
                        runtime,
                        TransferDirection::Upload,
                        TransferTraceState::Aborted,
                        serial,
                        transfer_trace,
                        false,
                    );
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

fn start_data_phase(
    runtime: &mut DiagnosticOwnerRuntime,
    direction: TransferDirection,
    trace: &mut binbook_fw::transfer_trace_command::TransferTraceCommand,
) {
    runtime
        .trace_lifecycle
        .start_data(direction, trace, embassy_time::Instant::now().as_micros());
}

fn end_data_phase(
    runtime: &mut DiagnosticOwnerRuntime,
    direction: TransferDirection,
    trace: &mut binbook_fw::transfer_trace_command::TransferTraceCommand,
) {
    runtime
        .trace_lifecycle
        .end_data(direction, trace, embassy_time::Instant::now().as_micros());
}

fn finish_data_trace(
    runtime: &mut DiagnosticOwnerRuntime,
    direction: TransferDirection,
    state: TransferTraceState,
    serial: &SerialState,
    trace: &mut binbook_fw::transfer_trace_command::TransferTraceCommand,
    library_pending: bool,
) {
    runtime.trace_lifecycle.finish(
        direction,
        state,
        serial.protocol_error_count(),
        embassy_time::Instant::now().as_micros(),
        library_pending,
        trace,
    );
}

fn finish_trace(
    state: TransferTraceState,
    serial: &SerialState,
    trace: &mut binbook_fw::transfer_trace_command::TransferTraceCommand,
    library_pending: bool,
) {
    let _ = trace.finish(
        state,
        serial.protocol_error_count(),
        embassy_time::Instant::now().as_micros(),
        library_pending,
    );
}

fn trace_phase(
    trace: &mut binbook_fw::transfer_trace_command::TransferTraceCommand,
    direction: TransferDirection,
    phase: binbook_fw::transfer_metrics::TransferPhase,
    enter: bool,
) {
    record_trace_phase(
        trace,
        direction,
        phase,
        enter,
        embassy_time::Instant::now().as_micros(),
    );
}

async fn read_file_size(runtime: &mut DiagnosticOwnerRuntime, name: &StorageName) -> Option<u64> {
    if let Some((cached_name, file_size)) = runtime.read_file.as_ref() {
        if cached_name == name {
            return Some(*file_size);
        }
    }
    let size = exchange(runtime, Request::FileSize { name: name.clone() }, None).await;
    let Ok(Response::FileSize(file_size)) = size.envelope.response else {
        runtime.read_file = None;
        return None;
    };
    runtime.read_file = Some((name.clone(), file_size));
    Some(file_size)
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
    let (cursor, returned) = select_diagnostic_index(runtime, transfer).await;
    transfer = returned;
    let Some(cursor) = cursor else {
        return (Status::NotFound, 0, transfer);
    };
    let index_name = diagnostic_index_name(cursor.slot());
    let file_len = cursor.file_len();
    let header_size = cursor.header_size();

    let header_read = Request::read(
        index_name.clone(),
        0,
        header_size as u16,
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
        Ok(Response::Read(read)) if usize::from(read) == header_size
    ) {
        return (Status::Error, 0, transfer);
    }
    let mut validator = LibraryIndexCursorValidator::new(cursor, &transfer[..header_size]);
    response_payload[..2].fill(0);
    let mut response_len = 2usize;
    let mut encoded_count = 0u16;
    let mut encode_failed = false;
    let mut offset = header_size;
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
            let record_index = (offset - header_size) / INDEX_RECORD_SIZE + local_index;
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

async fn select_diagnostic_index(
    runtime: &mut DiagnosticOwnerRuntime,
    mut transfer: &'static mut [u8; MAX_BOUNDED_IO_BYTES as usize],
) -> (
    Option<LibraryIndexCursor>,
    &'static mut [u8; MAX_BOUNDED_IO_BYTES as usize],
) {
    let (a, returned) = validate_diagnostic_index(runtime, LibraryIndexSlot::A, transfer).await;
    transfer = returned;
    let (b, returned) = validate_diagnostic_index(runtime, LibraryIndexSlot::B, transfer).await;
    transfer = returned;
    if a.is_some() || b.is_some() {
        return (select_library_index_cursor(a, b, None), transfer);
    }
    let (legacy, transfer) =
        validate_diagnostic_index(runtime, LibraryIndexSlot::Legacy, transfer).await;
    (select_library_index_cursor(None, None, legacy), transfer)
}

async fn validate_diagnostic_index(
    runtime: &mut DiagnosticOwnerRuntime,
    slot: LibraryIndexSlot,
    mut transfer: &'static mut [u8; MAX_BOUNDED_IO_BYTES as usize],
) -> (
    Option<LibraryIndexCursor>,
    &'static mut [u8; MAX_BOUNDED_IO_BYTES as usize],
) {
    let name = diagnostic_index_name(slot);
    let size = exchange(runtime, Request::FileSize { name: name.clone() }, None).await;
    let Ok(Response::FileSize(file_len)) = size.envelope.response else {
        return (None, transfer);
    };
    let Ok(file_len) = usize::try_from(file_len) else {
        return (None, transfer);
    };
    let header_size = if slot == LibraryIndexSlot::Legacy {
        INDEX_HEADER_SIZE
    } else {
        INDEX_V2_HEADER_SIZE
    };
    if file_len < header_size
        || file_len > header_size + 200 * INDEX_RECORD_SIZE
        || !(file_len - header_size).is_multiple_of(INDEX_RECORD_SIZE)
    {
        return (None, transfer);
    }
    let response = exchange(
        runtime,
        Request::read(name.clone(), 0, header_size as u16, BufferSlot::Diagnostic)
            .expect("header read is bounded"),
        Some(BufferLease::new(BufferSlot::Diagnostic, transfer)),
    )
    .await;
    transfer = take_transfer(response.buffer);
    if !matches!(response.envelope.response, Ok(Response::Read(len)) if usize::from(len) == header_size)
    {
        return (None, transfer);
    }
    let cursor = match slot {
        LibraryIndexSlot::Legacy => transfer[..INDEX_HEADER_SIZE]
            .try_into()
            .ok()
            .and_then(|header| LibraryIndexCursor::from_header(header, file_len).ok()),
        LibraryIndexSlot::A | LibraryIndexSlot::B => transfer[..INDEX_V2_HEADER_SIZE]
            .try_into()
            .ok()
            .and_then(|header| LibraryIndexCursor::from_v2_header(slot, header, file_len).ok()),
    };
    let Some(cursor) = cursor else {
        return (None, transfer);
    };
    let mut validator = LibraryIndexCursorValidator::new(cursor, &transfer[..header_size]);
    let mut offset = header_size;
    while offset < file_len {
        let len = (file_len - offset)
            .min((MAX_BOUNDED_IO_BYTES as usize / INDEX_RECORD_SIZE) * INDEX_RECORD_SIZE);
        let response = exchange(
            runtime,
            Request::read(
                name.clone(),
                offset as u64,
                len as u16,
                BufferSlot::Diagnostic,
            )
            .expect("validation read is bounded"),
            Some(BufferLease::new(BufferSlot::Diagnostic, transfer)),
        )
        .await;
        transfer = take_transfer(response.buffer);
        if !matches!(response.envelope.response, Ok(Response::Read(actual)) if usize::from(actual) == len)
        {
            return (None, transfer);
        }
        for record in transfer[..len].chunks_exact(INDEX_RECORD_SIZE) {
            if validator.push_record(record.try_into().unwrap()).is_err() {
                return (None, transfer);
            }
        }
        offset += len;
    }
    (validator.finish().ok().map(|()| cursor), transfer)
}

fn diagnostic_index_name(slot: LibraryIndexSlot) -> StorageName {
    StorageName::new(match slot {
        LibraryIndexSlot::A => LIBRARY_INDEX_SLOT_A,
        LibraryIndexSlot::B => LIBRARY_INDEX_SLOT_B,
        LibraryIndexSlot::Legacy => LIBRARY_INDEX_FILE,
    })
    .expect("index name is bounded")
}

async fn exchange(
    runtime: &mut DiagnosticOwnerRuntime,
    request: Request,
    buffer: Option<BufferLease<'static>>,
) -> OwnedResponse<'static> {
    let sequence = runtime.next_sequence();
    super::SD_CAPTURE_PURPOSE.store(
        match &request {
            Request::UploadWrite { .. } => 6,
            Request::UploadBegin { .. }
            | Request::UploadCommit { .. }
            | Request::UploadAbort { .. }
            | Request::Delete { .. } => 4,
            _ => 7,
        },
        portable_atomic::Ordering::Relaxed,
    );
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
    super::SD_CAPTURE_PURPOSE.store(0, portable_atomic::Ordering::Relaxed);
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

pub(super) async fn request_library_refresh(trace_id: Option<u32>) {
    LIBRARY_REFRESH_BARRIER.mark_pending();
    LIBRARY_REFRESH_TRIGGER_CHANNEL
        .send(LibraryRefreshTrigger {
            trace_id,
            prefetch: heapless::Vec::new(),
        })
        .await;
}
