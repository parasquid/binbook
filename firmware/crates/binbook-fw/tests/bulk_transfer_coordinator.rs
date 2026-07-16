#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::{
    Status, StorageBackend, StoreBulkData, StoreBulkOpenRequest, TransferDirection,
};
use binbook_fw::{
    bulk_slots::{TRANSFER_DATA_BYTES, TRANSFER_SLOT_COUNT},
    bulk_transfer_coordinator::{BulkOwnerEvent, BulkTransferCoordinator},
    storage_owner::{
        CompletionError, OwnedRequest, OwnedResponse, RequestError, Response, ResponseEnvelope,
    },
};

fn buffers() -> [&'static mut [u8; TRANSFER_DATA_BYTES]; TRANSFER_SLOT_COUNT] {
    core::array::from_fn(|_| Box::leak(Box::new([0u8; TRANSFER_DATA_BYTES])))
}

fn complete(mut request: OwnedRequest<'static>, response: Response) -> OwnedResponse<'static> {
    if let Some(buffer) = request.buffer.as_mut() {
        buffer.bytes_mut()[..usize::from(match response {
            Response::Read(len) => len,
            _ => 0,
        })]
            .fill(5);
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

fn stale(request: OwnedRequest<'static>) -> OwnedResponse<'static> {
    OwnedResponse {
        envelope: ResponseEnvelope {
            client: request.envelope.client,
            sequence: request.envelope.sequence,
            response: Err(CompletionError::Request(RequestError::Stale)),
        },
        buffer: request.buffer,
    }
}

#[test]
fn upload_admits_four_owner_writes_before_any_storage_completion() {
    let mut coordinator = BulkTransferCoordinator::new(buffers());
    let open = coordinator
        .begin_open(open_request(
            TransferDirection::Upload,
            2_048,
            crc32(&[3; 2_048]),
        ))
        .unwrap();
    let opened = match coordinator
        .complete_owner(complete(open, Response::UploadStarted(31)))
        .unwrap()
    {
        BulkOwnerEvent::Opened(value) => value,
        _ => panic!("expected open"),
    };

    let writes: Vec<_> = (0..4)
        .map(|index| {
            coordinator
                .admit_upload(StoreBulkData {
                    stream_id: opened.stream_id,
                    offset: index * 512,
                    end_of_data: index == 3,
                    data: &[3; 512],
                })
                .unwrap()
        })
        .collect();
    assert_eq!(coordinator.pending_owner(), 4);
    assert_eq!(coordinator.available_leases(), 0);
    assert!(writes
        .iter()
        .all(|write| coordinator.pending_owner_is_upload_write(write.envelope.sequence)));

    for (index, write) in writes.into_iter().enumerate() {
        let event = coordinator
            .complete_owner(complete(write, Response::Written(512)))
            .unwrap();
        if index == 3 {
            let BulkOwnerEvent::UploadCredit(Some(credit)) = event else {
                panic!("expected batched credit")
            };
            assert_eq!(credit.credits, 4);
            assert_eq!(credit.status, Status::Ok);
        } else {
            assert_eq!(event, BulkOwnerEvent::UploadCredit(None));
        }
    }
}

#[test]
fn abort_ack_and_reordered_stale_writes_are_correlated_by_sequence() {
    let mut coordinator = BulkTransferCoordinator::new(buffers());
    let open = coordinator
        .begin_open(open_request(TransferDirection::Upload, 1_024, 0))
        .unwrap();
    let opened = match coordinator
        .complete_owner(complete(open, Response::UploadStarted(31)))
        .unwrap()
    {
        BulkOwnerEvent::Opened(value) => value,
        _ => panic!("expected open"),
    };
    let writes: Vec<_> = (0..2)
        .map(|index| {
            coordinator
                .admit_upload(StoreBulkData {
                    stream_id: opened.stream_id,
                    offset: index * 512,
                    end_of_data: index == 1,
                    data: &[3; 512],
                })
                .unwrap()
        })
        .collect();
    let abort = coordinator.begin_upload_abort(opened.stream_id).unwrap();
    assert!(matches!(
        coordinator
            .complete_owner(complete(abort, Response::Ack))
            .unwrap(),
        BulkOwnerEvent::UploadAborted
    ));
    assert!(!coordinator.is_idle());
    for write in writes.into_iter().rev() {
        assert!(matches!(
            coordinator.complete_owner(stale(write)).unwrap(),
            BulkOwnerEvent::UploadCredit(None)
        ));
    }
    assert!(coordinator.is_idle());
    assert_eq!(coordinator.available_leases(), 4);
}

#[test]
fn readback_reuses_the_same_pool_and_returns_data_after_owner_completion() {
    let mut coordinator = BulkTransferCoordinator::new(buffers());
    let size = coordinator
        .begin_open(open_request(TransferDirection::Readback, 0, 0))
        .unwrap();
    let opened = match coordinator
        .complete_owner(complete(size, Response::FileSize(512)))
        .unwrap()
    {
        BulkOwnerEvent::Opened(value) => value,
        _ => panic!("expected open"),
    };
    let read = coordinator.begin_read().unwrap();
    let frame = match coordinator
        .complete_read_owner(complete(read, Response::Read(512)))
        .unwrap()
    {
        Some(frame) => frame,
        _ => panic!("expected data"),
    };
    assert_eq!(frame.stream_id, opened.stream_id);
    assert_eq!(frame.data, &[5; 512]);
}

#[test]
fn readback_abort_releases_pool_after_reordered_stale_owner_completions() {
    let mut coordinator = BulkTransferCoordinator::new(buffers());
    let size = coordinator
        .begin_open(open_request(TransferDirection::Readback, 0, 0))
        .unwrap();
    let opened = match coordinator
        .complete_owner(complete(size, Response::FileSize(1_024)))
        .unwrap()
    {
        BulkOwnerEvent::Opened(value) => value,
        _ => panic!("expected open"),
    };
    let reads: Vec<_> = (0..2).map(|_| coordinator.begin_read().unwrap()).collect();

    coordinator.abort_readback(opened.stream_id).unwrap();
    assert!(!coordinator.is_idle());
    for read in reads.into_iter().rev() {
        assert!(coordinator
            .complete_read_owner(complete(read, Response::Read(512)))
            .unwrap()
            .is_none());
        coordinator.reap_quiescent();
    }

    assert!(coordinator.is_idle());
    assert_eq!(coordinator.available_leases(), 4);
}

#[test]
fn timeout_cleanup_uses_owner_abort_for_upload_and_direct_abort_for_readback() {
    let mut upload = BulkTransferCoordinator::new(buffers());
    let open = upload
        .begin_open(open_request(TransferDirection::Upload, 512, 0))
        .unwrap();
    let upload_id = match upload
        .complete_owner(complete(open, Response::UploadStarted(41)))
        .unwrap()
    {
        BulkOwnerEvent::Opened(value) => value.stream_id,
        _ => panic!("expected open"),
    };
    let abort = upload.begin_timeout_cleanup().unwrap().unwrap();
    assert!(matches!(
        abort.envelope.request,
        binbook_fw::storage_owner::Request::UploadAbort { upload_id: 41 }
    ));
    upload
        .complete_owner(complete(abort, Response::Ack))
        .unwrap();
    assert!(upload.is_idle());
    assert_eq!(upload_id, 41);

    let mut readback = BulkTransferCoordinator::new(buffers());
    let size = readback
        .begin_open(open_request(TransferDirection::Readback, 0, 0))
        .unwrap();
    readback
        .complete_owner(complete(size, Response::FileSize(512)))
        .unwrap();
    assert!(readback.begin_timeout_cleanup().unwrap().is_none());
    assert!(readback.is_idle());
}

fn open_request(
    direction: TransferDirection,
    total_size: u32,
    expected_crc32: u32,
) -> StoreBulkOpenRequest<'static> {
    StoreBulkOpenRequest {
        direction,
        backend: StorageBackend::Sd,
        requested_window: 4,
        requested_chunk: 512,
        name: "book.binbook",
        total_size,
        expected_crc32,
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(u32::MAX, |state, byte| {
        binbook_fw::crc32::update_byte(state, *byte)
    })
}
