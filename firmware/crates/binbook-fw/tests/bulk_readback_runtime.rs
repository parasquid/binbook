#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::{
    Status, StorageBackend, StoreBulkCredit, StoreBulkOpenRequest, TransferDirection,
};
use binbook_fw::{
    bulk_readback_runtime::BulkReadbackRuntime,
    bulk_slots::{TRANSFER_DATA_BYTES, TRANSFER_SLOT_COUNT},
    storage_owner::{OwnedRequest, OwnedResponse, Response, ResponseEnvelope, StorageClient},
};

fn buffers() -> [&'static mut [u8; TRANSFER_DATA_BYTES]; TRANSFER_SLOT_COUNT] {
    core::array::from_fn(|_| Box::leak(Box::new([0u8; TRANSFER_DATA_BYTES])))
}

fn complete(mut request: OwnedRequest<'static>, response: Response) -> OwnedResponse<'static> {
    if let Some(buffer) = request.buffer.as_mut() {
        buffer.bytes_mut()[..512].fill(7);
    }
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
fn runtime_opens_from_owner_size_and_fills_four_static_slots() {
    let mut runtime = BulkReadbackRuntime::new(buffers());
    let size = runtime.begin_open(open_request(4, 512)).unwrap();
    let opened = runtime
        .complete_open(complete(size, Response::FileSize(2_048)))
        .unwrap();
    assert_eq!(opened.direction, TransferDirection::Readback);
    assert_eq!(opened.initial_credits, 4);
    assert_eq!(opened.total_size, 2_048);

    let reads: Vec<_> = (0..4).map(|_| runtime.begin_read().unwrap()).collect();
    assert_eq!(runtime.outstanding(), 4);
    assert_eq!(runtime.available_leases(), 0);
    for (index, read) in reads.into_iter().enumerate() {
        let frame = runtime
            .complete_read(complete(read, Response::Read(512)))
            .unwrap()
            .unwrap();
        assert_eq!(frame.offset, index as u32 * 512);
        assert_eq!(frame.data, &[7; 512]);
    }
    assert_eq!(runtime.sent(), 4);
}

#[test]
fn durable_host_credit_releases_slots_and_finish_reports_exact_crc() {
    let mut runtime = BulkReadbackRuntime::new(buffers());
    let size = runtime.begin_open(open_request(2, 512)).unwrap();
    let opened = runtime
        .complete_open(complete(size, Response::FileSize(1_024)))
        .unwrap();
    for _ in 0..2 {
        let read = runtime.begin_read().unwrap();
        runtime
            .complete_read(complete(read, Response::Read(512)))
            .unwrap();
    }
    runtime
        .apply_credit(StoreBulkCredit {
            stream_id: opened.stream_id,
            durable_offset: 1_024,
            credits: 2,
            status: Status::Ok,
        })
        .unwrap();
    assert_eq!(runtime.sent(), 0);
    let finish = runtime.finish(opened.stream_id).unwrap();
    assert_eq!(finish.total_bytes, 1_024);
    assert_eq!(finish.computed_crc32, crc32(&[7; 1_024]));
}

#[test]
fn status_invalidates_inflight_reads_and_stale_completion_only_reclaims_lease() {
    let mut runtime = BulkReadbackRuntime::new(buffers());
    let size = runtime.begin_open(open_request(2, 512)).unwrap();
    let opened = runtime
        .complete_open(complete(size, Response::FileSize(1_024)))
        .unwrap();
    let read = runtime.begin_read().unwrap();
    let status = runtime.status(opened.stream_id).unwrap();
    assert_eq!(status.durable_offset, 0);
    assert_eq!(status.available_credits, 2);
    assert_eq!(runtime.available_leases(), 3);
    assert!(runtime
        .complete_read(complete(read, Response::Read(512)))
        .unwrap()
        .is_none());
    assert_eq!(runtime.available_leases(), 4);
    assert_eq!(runtime.outstanding(), 0);
    assert_eq!(runtime.begin_read().unwrap().envelope.sequence, 500_002);
}

#[test]
fn reordered_stale_completions_reclaim_their_sequence_matched_leases() {
    let mut runtime = BulkReadbackRuntime::new(buffers());
    let size = runtime.begin_open(open_request(2, 512)).unwrap();
    let opened = runtime
        .complete_open(complete(size, Response::FileSize(1_024)))
        .unwrap();
    let reads: Vec<_> = (0..2).map(|_| runtime.begin_read().unwrap()).collect();

    runtime.abort(opened.stream_id).unwrap();
    for read in reads.into_iter().rev() {
        assert!(runtime
            .complete_read(complete(read, Response::Read(512)))
            .unwrap()
            .is_none());
    }

    assert_eq!(runtime.outstanding(), 0);
    assert_eq!(runtime.available_leases(), 4);
    assert!(runtime.lease_pool_releasable());
}

#[test]
fn wrong_owner_correlation_reclaims_matching_static_lease_and_requires_status() {
    let mut runtime = BulkReadbackRuntime::new(buffers());
    let size = runtime.begin_open(open_request(1, 512)).unwrap();
    let opened = runtime
        .complete_open(complete(size, Response::FileSize(512)))
        .unwrap();
    let read = runtime.begin_read().unwrap();
    let mut response = complete(read, Response::Read(512));
    response.envelope.client = StorageClient::Reader;
    assert!(runtime.complete_read(response).is_err());
    assert_eq!(runtime.available_leases(), 4);
    assert_eq!(runtime.outstanding(), 0);
    assert_eq!(runtime.status(opened.stream_id).unwrap().durable_offset, 0);
}

fn open_request(requested_window: u8, requested_chunk: u16) -> StoreBulkOpenRequest<'static> {
    StoreBulkOpenRequest {
        direction: TransferDirection::Readback,
        backend: StorageBackend::Sd,
        requested_window,
        requested_chunk,
        name: "book.binbook",
        total_size: 0,
        expected_crc32: 0,
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(u32::MAX, |state, byte| {
        binbook_fw::crc32::update_byte(state, *byte)
    })
}
