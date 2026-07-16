#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::{
    Status, StorageBackend, StoreBulkData, StoreBulkOpenRequest, TransferDirection,
};
use binbook_fw::{
    bulk_slots::{TRANSFER_DATA_BYTES, TRANSFER_SLOT_COUNT},
    bulk_upload_runtime::BulkUploadRuntime,
    storage_owner::{OwnedRequest, OwnedResponse, Response, ResponseEnvelope, StorageClient},
};

fn buffers() -> [&'static mut [u8; TRANSFER_DATA_BYTES]; TRANSFER_SLOT_COUNT] {
    core::array::from_fn(|_| Box::leak(Box::new([0u8; TRANSFER_DATA_BYTES])))
}

fn complete(request: OwnedRequest<'static>, response: Response) -> OwnedResponse<'static> {
    OwnedResponse {
        envelope: ResponseEnvelope {
            client: request.envelope.client,
            sequence: request.envelope.sequence,
            response: Ok(response),
        },
        buffer: request.buffer,
    }
}

#[test]
fn runtime_queues_four_static_leases_before_storage_and_credits_only_completions() {
    let mut runtime = BulkUploadRuntime::new(buffers());
    let begin = runtime
        .begin_open(StoreBulkOpenRequest {
            direction: TransferDirection::Upload,
            backend: StorageBackend::Sd,
            requested_window: 4,
            requested_chunk: 3_584,
            name: "book.binbook",
            total_size: 4 * 3_584,
            expected_crc32: 0,
        })
        .unwrap();
    assert!(begin.buffer.is_none());
    let opened = runtime
        .complete_open(complete(begin, Response::UploadStarted(41)))
        .unwrap();
    assert_eq!(opened.stream_id, 41);
    assert_eq!(opened.initial_credits, 4);

    let first = runtime
        .admit(StoreBulkData {
            stream_id: 41,
            offset: 0,
            end_of_data: false,
            data: &[1; 3_584],
        })
        .unwrap();
    let second = runtime
        .admit(StoreBulkData {
            stream_id: 41,
            offset: 3_584,
            end_of_data: false,
            data: &[2; 3_584],
        })
        .unwrap();
    let third = runtime
        .admit(StoreBulkData {
            stream_id: 41,
            offset: 7_168,
            end_of_data: false,
            data: &[3; 3_584],
        })
        .unwrap();
    let fourth = runtime
        .admit(StoreBulkData {
            stream_id: 41,
            offset: 10_752,
            end_of_data: true,
            data: &[4; 3_584],
        })
        .unwrap();
    assert_eq!(runtime.available_leases(), 0);
    assert_eq!(runtime.outstanding(), 4);

    assert_eq!(
        runtime
            .complete_write(complete(first, Response::Written(3_584)))
            .unwrap(),
        None
    );
    assert_eq!(
        runtime
            .complete_write(complete(second, Response::Written(3_584)))
            .unwrap(),
        None
    );
    assert_eq!(
        runtime
            .complete_write(complete(third, Response::Written(3_584)))
            .unwrap(),
        None
    );
    let credit = runtime
        .complete_write(complete(fourth, Response::Written(3_584)))
        .unwrap()
        .unwrap();
    assert_eq!(credit.stream_id, 41);
    assert_eq!(credit.durable_offset, 14_336);
    assert_eq!(credit.credits, 4);
    assert_eq!(credit.status, Status::Ok);
    assert_eq!(runtime.available_leases(), 4);
}

#[test]
fn runtime_abort_invalidates_admission_and_returns_control_priority_request() {
    let mut runtime = BulkUploadRuntime::new(buffers());
    let begin = runtime
        .begin_open(StoreBulkOpenRequest {
            direction: TransferDirection::Upload,
            backend: StorageBackend::Sd,
            requested_window: 1,
            requested_chunk: 512,
            name: "book.binbook",
            total_size: 512,
            expected_crc32: 0,
        })
        .unwrap();
    runtime
        .complete_open(complete(begin, Response::UploadStarted(9)))
        .unwrap();
    let abort = runtime.begin_abort(9).unwrap();
    assert_eq!(
        abort.envelope.request.priority(),
        binbook_fw::storage_owner::RequestPriority::Control
    );
    assert!(runtime
        .admit(StoreBulkData {
            stream_id: 9,
            offset: 0,
            end_of_data: true,
            data: &[0; 512],
        })
        .is_err());
    assert!(!runtime.lease_pool_releasable());
    runtime
        .complete_abort(complete(abort, Response::Ack))
        .unwrap();
    assert!(runtime.lease_pool_releasable());
}

#[test]
fn completion_after_abort_only_reclaims_the_stale_lease() {
    let mut runtime = BulkUploadRuntime::new(buffers());
    let begin = runtime.begin_open(open_request(4, 3_584, 7_168)).unwrap();
    runtime
        .complete_open(complete(begin, Response::UploadStarted(9)))
        .unwrap();
    let write = runtime
        .admit(StoreBulkData {
            stream_id: 9,
            offset: 0,
            end_of_data: false,
            data: &[3; 3_584],
        })
        .unwrap();
    let abort = runtime.begin_abort(9).unwrap();

    let stale = OwnedResponse {
        envelope: ResponseEnvelope {
            client: write.envelope.client,
            sequence: write.envelope.sequence,
            response: Err(binbook_fw::storage_owner::CompletionError::Request(
                binbook_fw::storage_owner::RequestError::Stale,
            )),
        },
        buffer: write.buffer,
    };
    assert_eq!(runtime.complete_write(stale).unwrap(), None);
    assert_eq!(runtime.outstanding(), 0);
    assert_eq!(runtime.available_leases(), 4);
    runtime
        .complete_abort(complete(abort, Response::Ack))
        .unwrap();
}

#[test]
fn wrong_owner_correlation_does_not_consume_the_lease() {
    let mut runtime = BulkUploadRuntime::new(buffers());
    let begin = runtime.begin_open(open_request(1, 512, 512)).unwrap();
    runtime
        .complete_open(complete(begin, Response::UploadStarted(9)))
        .unwrap();
    let write = runtime
        .admit(StoreBulkData {
            stream_id: 9,
            offset: 0,
            end_of_data: true,
            data: &[3; 512],
        })
        .unwrap();
    let mut response = complete(write, Response::Written(512));
    response.envelope.client = StorageClient::Reader;
    assert!(runtime.complete_write(response).is_err());
    assert_eq!(runtime.outstanding(), 0);
    assert_eq!(runtime.available_leases(), 4);
    let abort = runtime.begin_abort(9).unwrap();
    runtime
        .complete_abort(complete(abort, Response::Ack))
        .unwrap();
}

#[test]
fn new_session_waits_for_aborted_write_leases_to_return() {
    let mut runtime = BulkUploadRuntime::new(buffers());
    let begin = runtime.begin_open(open_request(1, 512, 512)).unwrap();
    runtime
        .complete_open(complete(begin, Response::UploadStarted(9)))
        .unwrap();
    let write = runtime
        .admit(StoreBulkData {
            stream_id: 9,
            offset: 0,
            end_of_data: true,
            data: &[3; 512],
        })
        .unwrap();
    let abort = runtime.begin_abort(9).unwrap();
    assert!(runtime.begin_open(open_request(1, 512, 512)).is_err());
    let stale = OwnedResponse {
        envelope: ResponseEnvelope {
            client: write.envelope.client,
            sequence: write.envelope.sequence,
            response: Err(binbook_fw::storage_owner::CompletionError::Request(
                binbook_fw::storage_owner::RequestError::Stale,
            )),
        },
        buffer: write.buffer,
    };
    runtime.complete_write(stale).unwrap();
    assert!(runtime.begin_open(open_request(1, 512, 512)).is_err());
    runtime
        .complete_abort(complete(abort, Response::Ack))
        .unwrap();
    assert!(runtime.begin_open(open_request(1, 512, 512)).is_ok());
}

#[test]
fn wrong_write_length_reclaims_lease_and_requires_abort() {
    let mut runtime = BulkUploadRuntime::new(buffers());
    let begin = runtime.begin_open(open_request(1, 512, 512)).unwrap();
    runtime
        .complete_open(complete(begin, Response::UploadStarted(9)))
        .unwrap();
    let write = runtime
        .admit(StoreBulkData {
            stream_id: 9,
            offset: 0,
            end_of_data: true,
            data: &[3; 512],
        })
        .unwrap();
    assert!(runtime
        .complete_write(complete(write, Response::Written(511)))
        .is_err());
    assert_eq!(runtime.outstanding(), 0);
    assert_eq!(runtime.available_leases(), 4);
    let abort = runtime.begin_abort(9).unwrap();
    runtime
        .complete_abort(complete(abort, Response::Ack))
        .unwrap();
}

#[test]
fn finish_commits_only_after_exact_crc_and_owner_ack() {
    let data = [7u8; 512];
    let mut runtime = BulkUploadRuntime::new(buffers());
    let mut request = open_request(1, 512, 512);
    request.expected_crc32 = crc32(&data);
    let begin = runtime.begin_open(request).unwrap();
    runtime
        .complete_open(complete(begin, Response::UploadStarted(9)))
        .unwrap();
    let write = runtime
        .admit(StoreBulkData {
            stream_id: 9,
            offset: 0,
            end_of_data: true,
            data: &data,
        })
        .unwrap();
    runtime
        .complete_write(complete(write, Response::Written(512)))
        .unwrap();

    let commit = runtime.begin_finish(9).unwrap();
    assert!(matches!(
        commit.envelope.request,
        binbook_fw::storage_owner::Request::UploadCommit { upload_id: 9, .. }
    ));
    let finished = runtime
        .complete_finish(complete(commit, Response::Ack))
        .unwrap();
    assert!(finished.published);
    assert_eq!(finished.response.total_bytes, 512);
    assert_eq!(finished.response.computed_crc32, crc32(&data));
    assert_eq!(
        finished.response.result,
        binbook_diagnostic_protocol::StoreUploadCommitResult::Ok
    );
}

#[test]
fn crc_mismatch_aborts_storage_instead_of_publishing() {
    let data = [7u8; 512];
    let mut runtime = BulkUploadRuntime::new(buffers());
    let begin = runtime.begin_open(open_request(1, 512, 512)).unwrap();
    runtime
        .complete_open(complete(begin, Response::UploadStarted(9)))
        .unwrap();
    let write = runtime
        .admit(StoreBulkData {
            stream_id: 9,
            offset: 0,
            end_of_data: true,
            data: &data,
        })
        .unwrap();
    runtime
        .complete_write(complete(write, Response::Written(512)))
        .unwrap();

    let abort = runtime.begin_finish(9).unwrap();
    assert!(matches!(
        abort.envelope.request,
        binbook_fw::storage_owner::Request::UploadAbort { upload_id: 9 }
    ));
    let finished = runtime
        .complete_finish(complete(abort, Response::Ack))
        .unwrap();
    assert!(!finished.published);
    assert_eq!(
        finished.response.result,
        binbook_diagnostic_protocol::StoreUploadCommitResult::CrcMismatch
    );
}

fn open_request(
    requested_window: u8,
    requested_chunk: u16,
    total_size: u32,
) -> StoreBulkOpenRequest<'static> {
    StoreBulkOpenRequest {
        direction: TransferDirection::Upload,
        backend: StorageBackend::Sd,
        requested_window,
        requested_chunk,
        name: "book.binbook",
        total_size,
        expected_crc32: 0,
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(u32::MAX, |state, byte| {
        binbook_fw::crc32::update_byte(state, *byte)
    })
}
