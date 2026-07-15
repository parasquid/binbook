use crate::{
    upload_library::wait_for_library,
    upload_protocol::{abort, checked_payload, encode_request, with_abort, Sequence},
    upload_source::measure_source,
};
use binbook_diagnostic_protocol::{
    decode_store_upload_begin_response, decode_store_upload_commit_response,
    decode_store_upload_write_response, encode_store_upload_begin_request,
    encode_store_upload_commit_request, encode_store_upload_write_request, Opcode, StorageBackend,
    StoreUploadBeginRequest, StoreUploadCommitResult, StoreUploadWriteRequest,
};
use std::io::{Read, Seek, SeekFrom};

pub use crate::upload_source::UPLOAD_CHUNK_BYTES;
pub use crate::upload_types::{
    Clock, ProgressEvent, ProgressSink, SystemClock, UploadError, UploadPacket, UploadRequest,
    UploadSummary, UploadTransport,
};

pub fn upload_stream<R, T, P, C>(
    source: &mut R,
    request: UploadRequest<'_>,
    transport: &mut T,
    progress: &mut P,
    clock: &C,
) -> Result<UploadSummary, UploadError>
where
    R: Read + Seek,
    T: UploadTransport,
    P: ProgressSink,
    C: Clock,
{
    let (size, crc32) = measure_source(source)?;
    let file_size = u32::try_from(size).map_err(|_| UploadError::TooLarge(size))?;
    progress.report(ProgressEvent::Preparing { total: size });
    let upload_window = transport.upload_window()?.clamp(1, 2);

    let mut sequence = Sequence::new();
    let begin_sequence = sequence.take();
    let begin = StoreUploadBeginRequest {
        backend: StorageBackend::Sd,
        name: request.destination,
        file_size,
        expected_crc32: crc32,
    };
    let begin_frame = encode_request(begin_sequence, Opcode::StoreUploadBegin, |payload| {
        encode_store_upload_begin_request(&begin, payload)
    })?;
    let begin_response =
        transport.exchange(&begin_frame, Opcode::StoreUploadBegin, begin_sequence)?;
    let begin_payload = checked_payload(&begin_response, Opcode::StoreUploadBegin, begin_sequence)?;
    let upload_id = decode_store_upload_begin_response(&begin_payload)?;

    source.seek(SeekFrom::Start(0))?;
    let mut buffer = [0u8; UPLOAD_CHUNK_BYTES];
    let mut acknowledged = 0u64;
    let mut queued = 0u64;
    while acknowledged < size {
        let mut packets = Vec::with_capacity(upload_window);
        while packets.len() < upload_window && queued < size {
            let wanted = usize::try_from((size - queued).min(UPLOAD_CHUNK_BYTES as u64))
                .expect("bounded upload chunk fits usize");
            if let Err(error) = source.read_exact(&mut buffer[..wanted]) {
                return Err(with_abort(
                    UploadError::Io(error),
                    transport,
                    upload_id,
                    &mut sequence,
                ));
            }
            let write_sequence = sequence.take();
            let write = StoreUploadWriteRequest {
                upload_id,
                offset: queued as u32,
                data: &buffer[..wanted],
            };
            let frame = encode_request(write_sequence, Opcode::StoreUploadWrite, |payload| {
                encode_store_upload_write_request(&write, payload)
            })?;
            packets.push(UploadPacket {
                frame,
                opcode: Opcode::StoreUploadWrite,
                sequence: write_sequence,
                sent_len: wanted,
            });
            queued += wanted as u64;
        }
        let responses = match transport.exchange_batch(&packets) {
            Ok(responses) if responses.len() == packets.len() => responses,
            Ok(responses) => {
                let error = UploadError::Response(format!(
                    "expected {} write responses, got {}",
                    packets.len(),
                    responses.len()
                ));
                return Err(with_abort(error, transport, upload_id, &mut sequence));
            }
            Err(error) => return Err(with_abort(error, transport, upload_id, &mut sequence)),
        };
        for (packet, response) in packets.iter().zip(responses) {
            let write_payload = match checked_payload(&response, packet.opcode, packet.sequence) {
                Ok(payload) => payload,
                Err(error) => {
                    return Err(with_abort(error, transport, upload_id, &mut sequence));
                }
            };
            let accepted = match decode_store_upload_write_response(&write_payload) {
                Ok(accepted) => accepted as usize,
                Err(error) => {
                    return Err(with_abort(
                        UploadError::Codec(error),
                        transport,
                        upload_id,
                        &mut sequence,
                    ));
                }
            };
            if accepted != packet.sent_len {
                let cleanup_confirmed = abort(transport, upload_id, &mut sequence);
                return Err(UploadError::AcceptedLength {
                    expected: packet.sent_len,
                    actual: accepted,
                    cleanup_confirmed,
                });
            }
            acknowledged += accepted as u64;
            progress.report(ProgressEvent::Transfer {
                acknowledged,
                total: size,
                elapsed_ms: elapsed_ms(clock),
            });
        }
    }

    progress.report(ProgressEvent::Verifying);
    let commit_sequence = sequence.take();
    let commit_frame = encode_request(commit_sequence, Opcode::StoreUploadCommit, |payload| {
        encode_store_upload_commit_request(upload_id, payload)
    })?;
    let commit_response =
        match transport.exchange(&commit_frame, Opcode::StoreUploadCommit, commit_sequence) {
            Ok(response) => response,
            Err(error) => return Err(with_abort(error, transport, upload_id, &mut sequence)),
        };
    let commit_payload =
        checked_payload(&commit_response, Opcode::StoreUploadCommit, commit_sequence)?;
    let commit = decode_store_upload_commit_response(&commit_payload)?;
    if commit.result != StoreUploadCommitResult::Ok || commit.computed_crc32 != crc32 {
        let cleanup_confirmed = abort(transport, upload_id, &mut sequence);
        return Err(UploadError::Crc {
            expected: crc32,
            actual: commit.computed_crc32,
            cleanup_confirmed,
        });
    }

    let library_visible = if request.skip_library_wait {
        false
    } else {
        progress.report(ProgressEvent::UpdatingLibrary);
        wait_for_library(transport, request.destination, size, &mut sequence)?
    };
    let elapsed = clock.elapsed();
    let bytes_per_second = if elapsed.is_zero() {
        0
    } else {
        (size as u128 * 1_000 / elapsed.as_millis().max(1)) as u64
    };
    let summary = UploadSummary {
        bytes: size,
        crc32,
        elapsed_ms: elapsed.as_millis().min(u64::MAX as u128) as u64,
        bytes_per_second,
        library_visible,
    };
    progress.report(ProgressEvent::Complete(summary.clone()));
    Ok(summary)
}

fn elapsed_ms<C: Clock>(clock: &C) -> u64 {
    clock.elapsed().as_millis().min(u64::MAX as u128) as u64
}
