use binbook_diagnostic_protocol::{
    StorageBackend, StoreBulkData, StoreBulkOpenRequest, StoreUploadCommitResult, TransferDirection,
};

pub use crate::bulk_readback_io::BulkReadbackOutput;
pub use crate::bulk_readback_transport::{BulkReadbackTransport, DataPollError, OwnedBulkData};
use crate::{
    bulk_readback_io::rollback,
    bulk_state::BulkReadbackState,
    storage_readback::ReadbackError,
    upload::{Clock, ProgressEvent, ProgressSink, UploadSummary},
};

pub fn bulk_readback_stream<T, W, P, C>(
    path: &str,
    requested_window: u8,
    transport: &mut T,
    output: &mut W,
    progress: &mut P,
    clock: &C,
) -> Result<UploadSummary, ReadbackError>
where
    T: BulkReadbackTransport,
    W: BulkReadbackOutput,
    P: ProgressSink,
    C: Clock,
{
    let opened = transport.open(StoreBulkOpenRequest {
        direction: TransferDirection::Readback,
        backend: StorageBackend::Sd,
        requested_window,
        requested_chunk: 0,
        name: path,
        total_size: 0,
        expected_crc32: 0,
    })?;
    if opened.direction != TransferDirection::Readback
        || opened.durable_offset != 0
        || opened.initial_credits > requested_window
    {
        return abort_error(
            transport,
            opened.stream_id,
            "invalid bulk readback open response",
        );
    }
    let mut state = match BulkReadbackState::new(
        opened.stream_id,
        opened.total_size,
        opened.selected_chunk,
        opened.initial_credits,
    ) {
        Ok(state) => state,
        Err(error) => {
            return abort_error(
                transport,
                opened.stream_id,
                &format!("invalid bulk readback state: {error}"),
            );
        }
    };
    progress.report(ProgressEvent::Preparing {
        total: u64::from(opened.total_size),
    });
    let mut crc_state = u32::MAX;

    while state.durable_offset() < opened.total_size {
        match transport.poll_data() {
            Ok(owned) => {
                let frame = StoreBulkData {
                    stream_id: owned.stream_id,
                    offset: owned.offset,
                    end_of_data: owned.end_of_data,
                    data: &owned.data,
                };
                if let Err(error) = state.accept(&frame) {
                    rollback(output, u64::from(state.durable_offset()))?;
                    return abort_error(
                        transport,
                        opened.stream_id,
                        &format!("invalid bulk readback data: {error}"),
                    );
                }
                if let Err(error) = output.write_all(&owned.data) {
                    rollback(output, u64::from(state.durable_offset()))?;
                    let _ = transport.abort(opened.stream_id);
                    return Err(ReadbackError::Output(error));
                }
                crc_state = crc32_update(crc_state, &owned.data);
                let credit = match state.mark_durable(owned.offset, owned.data.len()) {
                    Ok(credit) => credit,
                    Err(error) => {
                        rollback(output, u64::from(state.durable_offset()))?;
                        return abort_error(
                            transport,
                            opened.stream_id,
                            &format!("invalid durable readback write: {error}"),
                        );
                    }
                };
                if let Some(credit) = credit {
                    if let Err(error) = transport.send_credit(credit) {
                        rollback(output, u64::from(state.durable_offset()))?;
                        let _ = transport.abort(opened.stream_id);
                        return Err(error);
                    }
                }
            }
            Err(DataPollError::Timeout) => {
                if let Err(error) = output.flush() {
                    let _ = transport.abort(opened.stream_id);
                    return Err(ReadbackError::Output(error));
                }
                let status = match transport.status(opened.stream_id) {
                    Ok(status) => status,
                    Err(error) => {
                        let _ = transport.abort(opened.stream_id);
                        return Err(error);
                    }
                };
                let local_crc = finalized_crc(state.durable_offset(), crc_state);
                if status.running_crc32 != local_crc {
                    rollback(output, u64::from(state.durable_offset()))?;
                    return abort_error(
                        transport,
                        opened.stream_id,
                        "bulk readback status CRC did not match durable output",
                    );
                }
                if let Err(error) = state.apply_status(status) {
                    rollback(output, u64::from(state.durable_offset()))?;
                    return abort_error(
                        transport,
                        opened.stream_id,
                        &format!("invalid bulk readback status barrier: {error}"),
                    );
                }
                rollback(output, u64::from(state.durable_offset()))?;
            }
            Err(DataPollError::Transport(message)) => {
                rollback(output, u64::from(state.durable_offset()))?;
                return abort_error(transport, opened.stream_id, &message);
            }
        }
        progress.report(ProgressEvent::Transfer {
            acknowledged: u64::from(state.durable_offset()),
            total: u64::from(opened.total_size),
            elapsed_ms: elapsed_ms(clock),
        });
    }

    if let Err(error) = output.flush() {
        let _ = transport.abort(opened.stream_id);
        return Err(ReadbackError::Output(error));
    }
    progress.report(ProgressEvent::Verifying);
    let finish = match transport.finish(opened.stream_id) {
        Ok(finish) => finish,
        Err(error) => {
            let _ = transport.abort(opened.stream_id);
            return Err(error);
        }
    };
    let crc32 = !crc_state;
    if finish.stream_id != opened.stream_id
        || finish.result != StoreUploadCommitResult::Ok
        || finish.total_bytes != opened.total_size
        || finish.computed_crc32 != crc32
    {
        return abort_error(
            transport,
            opened.stream_id,
            "bulk readback finish size or CRC did not match output",
        );
    }
    let elapsed_ms = elapsed_ms(clock);
    let summary = UploadSummary {
        bytes: u64::from(opened.total_size),
        crc32,
        elapsed_ms,
        bytes_per_second: u64::from(opened.total_size)
            .saturating_mul(1_000)
            .checked_div(elapsed_ms)
            .unwrap_or(0),
        library_visible: false,
    };
    progress.report(ProgressEvent::Complete(summary.clone()));
    Ok(summary)
}

fn abort_error<T: BulkReadbackTransport, R>(
    transport: &mut T,
    stream_id: u32,
    message: &str,
) -> Result<R, ReadbackError> {
    let cleanup = transport.abort(stream_id);
    Err(ReadbackError::Protocol(format!(
        "{message}; cleanup confirmed: {cleanup}"
    )))
}

fn finalized_crc(offset: u32, crc_state: u32) -> u32 {
    if offset == 0 {
        0
    } else {
        !crc_state
    }
}

fn crc32_update(mut crc: u32, bytes: &[u8]) -> u32 {
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ 0xedb8_8320
            };
        }
    }
    crc
}

fn elapsed_ms(clock: &impl Clock) -> u64 {
    clock.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}
