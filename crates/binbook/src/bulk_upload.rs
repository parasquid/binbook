use std::io::{Read, Seek, SeekFrom};

use binbook_diagnostic_protocol::{
    StorageBackend, StoreBulkCredit, StoreBulkData, StoreBulkFinishResponse, StoreBulkOpenRequest,
    StoreBulkOpenResponse, StoreBulkStatusResponse, StoreUploadCommitResult, TransferDirection,
};

use crate::{
    bulk_state::BulkUploadState,
    upload::{Clock, ProgressEvent, ProgressSink, UploadError, UploadRequest, UploadSummary},
    upload_source::measure_source,
};

#[derive(Debug)]
pub enum CreditPollError {
    Timeout,
    Transport(String),
}

pub trait BulkUploadTransport {
    fn open(
        &mut self,
        request: StoreBulkOpenRequest<'_>,
    ) -> Result<StoreBulkOpenResponse, UploadError>;
    fn send_data(&mut self, frame: StoreBulkData<'_>) -> Result<(), UploadError>;
    fn poll_credit(&mut self) -> Result<StoreBulkCredit, CreditPollError>;
    fn status(&mut self, stream_id: u32) -> Result<StoreBulkStatusResponse, UploadError>;
    fn finish(&mut self, stream_id: u32) -> Result<StoreBulkFinishResponse, UploadError>;
    fn abort(&mut self, stream_id: u32) -> bool;
    fn wait_library(&mut self, _name: &str, _size: u64) -> Result<bool, UploadError> {
        Ok(true)
    }
}

pub fn bulk_upload_stream<R, T, P, C>(
    source: &mut R,
    request: UploadRequest<'_>,
    requested_window: u8,
    transport: &mut T,
    progress: &mut P,
    clock: &C,
) -> Result<UploadSummary, UploadError>
where
    R: Read + Seek,
    T: BulkUploadTransport,
    P: ProgressSink,
    C: Clock,
{
    let (size, crc32) = measure_source(source)?;
    let total_size = u32::try_from(size).map_err(|_| UploadError::TooLarge(size))?;
    progress.report(ProgressEvent::Preparing { total: size });
    let opened = transport.open(StoreBulkOpenRequest {
        direction: TransferDirection::Upload,
        backend: StorageBackend::Sd,
        requested_window,
        requested_chunk: 0,
        name: request.destination,
        total_size,
        expected_crc32: crc32,
    })?;
    if opened.direction != TransferDirection::Upload
        || opened.total_size != total_size
        || opened.durable_offset != 0
        || opened.initial_credits > requested_window
    {
        return abort_error(
            transport,
            opened.stream_id,
            "bulk open response did not match the upload request",
        );
    }
    let mut state = match BulkUploadState::new(
        opened.stream_id,
        total_size,
        opened.selected_chunk,
        opened.initial_credits,
        opened.initial_credits,
    ) {
        Ok(state) => state,
        Err(error) => {
            return abort_error(
                transport,
                opened.stream_id,
                &format!("invalid bulk open state: {error}"),
            );
        }
    };
    if let Err(error) = source.seek(SeekFrom::Start(0)) {
        return abort_owned_error(transport, opened.stream_id, UploadError::Io(error));
    }
    let mut buffer = vec![0; usize::from(opened.selected_chunk)];

    while state.durable_offset() < total_size {
        while state.available_credits() != 0 && state.sent_offset() < total_size {
            let wanted = usize::try_from(
                (total_size - state.sent_offset()).min(u32::from(opened.selected_chunk)),
            )
            .expect("bulk chunk fits usize");
            if let Err(error) = source.read_exact(&mut buffer[..wanted]) {
                return abort_owned_error(transport, opened.stream_id, UploadError::Io(error));
            }
            let grant = state.grant(wanted).map_err(|error| {
                UploadError::Response(format!("invalid local bulk grant: {error}"))
            })?;
            if let Err(error) = transport.send_data(StoreBulkData {
                stream_id: opened.stream_id,
                offset: grant.offset,
                end_of_data: grant.end_of_data,
                data: &buffer[..wanted],
            }) {
                return abort_owned_error(transport, opened.stream_id, error);
            }
        }

        match transport.poll_credit() {
            Ok(credit) => {
                if let Err(error) = state.apply_credit(credit) {
                    return abort_error(
                        transport,
                        opened.stream_id,
                        &format!("invalid bulk credit: {error}"),
                    );
                }
            }
            Err(CreditPollError::Timeout) => {
                let status = match transport.status(opened.stream_id) {
                    Ok(status) => status,
                    Err(error) => {
                        return abort_owned_error(transport, opened.stream_id, error);
                    }
                };
                if let Err(error) = state.apply_status(status) {
                    return abort_error(
                        transport,
                        opened.stream_id,
                        &format!("invalid bulk status barrier: {error}"),
                    );
                }
                if let Err(error) = source.seek(SeekFrom::Start(u64::from(state.sent_offset()))) {
                    return abort_owned_error(transport, opened.stream_id, UploadError::Io(error));
                }
            }
            Err(CreditPollError::Transport(message)) => {
                return abort_error(transport, opened.stream_id, &message);
            }
        }
        progress.report(ProgressEvent::Transfer {
            acknowledged: u64::from(state.durable_offset()),
            total: size,
            elapsed_ms: elapsed_ms(clock),
        });
    }

    progress.report(ProgressEvent::Verifying);
    let finish = match transport.finish(opened.stream_id) {
        Ok(finish) => finish,
        Err(error) => return abort_owned_error(transport, opened.stream_id, error),
    };
    if finish.stream_id != opened.stream_id
        || finish.result != StoreUploadCommitResult::Ok
        || finish.total_bytes != total_size
        || finish.computed_crc32 != crc32
    {
        return abort_error(
            transport,
            opened.stream_id,
            "bulk finish size, result, or CRC did not match",
        );
    }
    progress.report(ProgressEvent::Publishing);
    let library_visible = if request.skip_library_wait {
        false
    } else {
        progress.report(ProgressEvent::UpdatingLibrary);
        transport.wait_library(request.destination, size)?
    };
    let elapsed_ms = elapsed_ms(clock);
    let summary = UploadSummary {
        bytes: size,
        crc32,
        elapsed_ms,
        bytes_per_second: size
            .saturating_mul(1_000)
            .checked_div(elapsed_ms)
            .unwrap_or(0),
        library_visible,
    };
    progress.report(ProgressEvent::Complete(summary.clone()));
    Ok(summary)
}

fn abort_error<T: BulkUploadTransport, R>(
    transport: &mut T,
    stream_id: u32,
    message: &str,
) -> Result<R, UploadError> {
    Err(UploadError::Transport {
        message: message.into(),
        cleanup_confirmed: transport.abort(stream_id),
    })
}

fn abort_owned_error<T: BulkUploadTransport, R>(
    transport: &mut T,
    stream_id: u32,
    error: UploadError,
) -> Result<R, UploadError> {
    abort_error(transport, stream_id, &error.to_string())
}

fn elapsed_ms<C: Clock>(clock: &C) -> u64 {
    clock.elapsed().as_millis().min(u64::MAX as u128) as u64
}
