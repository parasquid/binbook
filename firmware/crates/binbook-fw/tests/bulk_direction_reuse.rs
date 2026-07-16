#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::{StorageBackend, StoreBulkOpenRequest, TransferDirection};
use binbook_fw::{
    bulk_readback_runtime::BulkReadbackRuntime,
    bulk_slots::{TRANSFER_DATA_BYTES, TRANSFER_SLOT_COUNT},
    bulk_upload_runtime::BulkUploadRuntime,
    storage_owner::{OwnedRequest, OwnedResponse, Response, ResponseEnvelope},
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
fn one_four_buffer_pool_moves_between_upload_and_readback_directions() {
    let mut upload = BulkUploadRuntime::new(buffers());
    let open = upload
        .begin_open(StoreBulkOpenRequest {
            direction: TransferDirection::Upload,
            backend: StorageBackend::Sd,
            requested_window: 4,
            requested_chunk: 512,
            name: "book.binbook",
            total_size: 512,
            expected_crc32: crc32(&[4; 512]),
        })
        .unwrap();
    upload
        .complete_open(complete(open, Response::UploadStarted(7)))
        .unwrap();
    let write = upload
        .admit(binbook_diagnostic_protocol::StoreBulkData {
            stream_id: 7,
            offset: 0,
            end_of_data: true,
            data: &[4; 512],
        })
        .unwrap();
    upload
        .complete_write(complete(write, Response::Written(512)))
        .unwrap();
    let commit = upload.begin_finish(7).unwrap();
    upload
        .complete_finish(complete(commit, Response::Ack))
        .unwrap();

    assert!(upload.lease_pool_releasable());
    let pool = upload.into_lease_pool();
    assert_eq!(pool.available(), 4);

    let mut readback = BulkReadbackRuntime::from_lease_pool(pool);
    let size = readback
        .begin_open(StoreBulkOpenRequest {
            direction: TransferDirection::Readback,
            backend: StorageBackend::Sd,
            requested_window: 4,
            requested_chunk: 512,
            name: "book.binbook",
            total_size: 0,
            expected_crc32: 0,
        })
        .unwrap();
    let opened = readback
        .complete_open(complete(size, Response::FileSize(512)))
        .unwrap();
    assert_eq!(opened.initial_credits, 4);
    readback.abort(opened.stream_id).unwrap();
    assert!(readback.lease_pool_releasable());
    let _pool = readback.into_lease_pool();
}

fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(u32::MAX, |state, byte| {
        binbook_fw::crc32::update_byte(state, *byte)
    })
}
