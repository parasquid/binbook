#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::{
    encode_store_bulk_abort_request, encode_store_bulk_credit, encode_store_bulk_data,
    encode_store_bulk_open_request, encode_store_bulk_status_request, FrameHeader, FrameKind,
    Opcode, Status, StorageBackend, StoreBulkCredit, StoreBulkData, StoreBulkOpenRequest,
    TransferDirection, MAX_PAYLOAD_BYTES,
};
use binbook_fw::{
    bulk_protocol_dispatch::{
        dispatch_bulk_protocol, dispatch_bulk_protocol_profiled, BulkDispatchEvent,
        BulkProtocolAction,
    },
    bulk_slots::{TRANSFER_DATA_BYTES, TRANSFER_SLOT_COUNT},
    bulk_transfer_coordinator::{BulkOwnerEvent, BulkTransferCoordinator},
    storage_owner::{OwnedRequest, OwnedResponse, Response, ResponseEnvelope},
};

fn buffers() -> [&'static mut [u8; TRANSFER_DATA_BYTES]; TRANSFER_SLOT_COUNT] {
    core::array::from_fn(|_| Box::leak(Box::new([0u8; TRANSFER_DATA_BYTES])))
}

fn header(kind: FrameKind, opcode: Opcode, sequence: u16) -> FrameHeader {
    FrameHeader {
        kind,
        opcode,
        status: Status::Ok,
        sequence,
        payload_len: 0,
    }
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
fn protocol_admits_four_data_events_without_per_frame_responses() {
    let mut coordinator = BulkTransferCoordinator::new(buffers());
    let mut payload = [0; MAX_PAYLOAD_BYTES];
    let len = encode_store_bulk_open_request(
        &StoreBulkOpenRequest {
            direction: TransferDirection::Upload,
            backend: StorageBackend::Sd,
            requested_window: 4,
            requested_chunk: 512,
            name: "book.binbook",
            total_size: 2_048,
            expected_crc32: 0,
        },
        &mut payload,
    )
    .unwrap();
    let open = match dispatch_bulk_protocol(
        &mut coordinator,
        header(FrameKind::Request, Opcode::StoreBulkOpen, 7),
        &payload[..len],
    ) {
        BulkProtocolAction::Owner {
            request,
            reply_to: Some(reply),
            ..
        } => {
            assert_eq!(reply.sequence, 7);
            request
        }
        _ => panic!("open must defer to owner"),
    };
    let opened = match coordinator
        .complete_owner(complete(open, Response::UploadStarted(41)))
        .unwrap()
    {
        BulkOwnerEvent::Opened(opened) => opened,
        _ => panic!("expected open"),
    };

    for index in 0..4 {
        let len = encode_store_bulk_data(
            &StoreBulkData {
                stream_id: opened.stream_id,
                offset: index * 512,
                end_of_data: index == 3,
                data: &[9; 512],
            },
            &mut payload,
        )
        .unwrap();
        assert!(matches!(
            dispatch_bulk_protocol(
                &mut coordinator,
                header(FrameKind::Event, Opcode::StoreBulkData, 0),
                &payload[..len],
            ),
            BulkProtocolAction::Owner { reply_to: None, .. }
        ));
    }
    assert_eq!(coordinator.pending_owner(), 4);
    assert_eq!(coordinator.available_leases(), 0);
}

#[test]
fn wrong_frame_kind_is_rejected_before_data_admission() {
    let mut coordinator = BulkTransferCoordinator::new(buffers());
    let mut payload = [0; MAX_PAYLOAD_BYTES];
    let len = encode_store_bulk_data(
        &StoreBulkData {
            stream_id: 1,
            offset: 0,
            end_of_data: true,
            data: &[9; 512],
        },
        &mut payload,
    )
    .unwrap();
    assert!(matches!(
        dispatch_bulk_protocol(
            &mut coordinator,
            header(FrameKind::Request, Opcode::StoreBulkData, 1),
            &payload[..len],
        ),
        BulkProtocolAction::Immediate {
            status: Status::BadRequest,
            ..
        }
    ));
    assert_eq!(coordinator.pending_owner(), 0);
    assert_eq!(coordinator.available_leases(), 4);
}

#[test]
fn status_defers_but_abort_stays_serviceable_with_full_data_pool() {
    let (mut coordinator, stream_id) = full_upload();
    let mut payload = [0; MAX_PAYLOAD_BYTES];
    let status_len = encode_store_bulk_status_request(stream_id, &mut payload).unwrap();
    assert!(matches!(
        dispatch_bulk_protocol(
            &mut coordinator,
            header(FrameKind::Request, Opcode::StoreBulkStatus, 9),
            &payload[..status_len],
        ),
        BulkProtocolAction::DeferredStatus { stream_id: id } if id == stream_id
    ));
    let abort_len = encode_store_bulk_abort_request(stream_id, &mut payload).unwrap();
    assert!(matches!(
        dispatch_bulk_protocol(
            &mut coordinator,
            header(FrameKind::Request, Opcode::StoreBulkAbort, 10),
            &payload[..abort_len],
        ),
        BulkProtocolAction::Owner {
            reply_to: Some(reply),
            ..
        } if reply.sequence == 10
    ));
}

#[test]
fn readback_credit_profiles_the_durable_crc_fold_boundary() {
    let mut coordinator = BulkTransferCoordinator::new(buffers());
    let open = coordinator
        .begin_open(StoreBulkOpenRequest {
            direction: TransferDirection::Readback,
            backend: StorageBackend::Sd,
            requested_window: 1,
            requested_chunk: 512,
            name: "book.binbook",
            total_size: 0,
            expected_crc32: 0,
        })
        .unwrap();
    let opened = match coordinator
        .complete_owner(complete(open, Response::FileSize(512)))
        .unwrap()
    {
        BulkOwnerEvent::Opened(value) => value,
        _ => panic!("expected open"),
    };
    let read = coordinator.begin_read().unwrap();
    let frame = coordinator
        .complete_read_owner(complete(read, Response::Read(512)))
        .unwrap()
        .unwrap();
    assert_eq!(frame.data.len(), 512);

    let mut payload = [0; MAX_PAYLOAD_BYTES];
    let len = encode_store_bulk_credit(
        &StoreBulkCredit {
            stream_id: opened.stream_id,
            durable_offset: 512,
            credits: 1,
            status: Status::Ok,
        },
        &mut payload,
    )
    .unwrap();
    let mut events = Vec::new();
    assert!(matches!(
        dispatch_bulk_protocol_profiled(
            &mut coordinator,
            header(FrameKind::Event, Opcode::StoreBulkCredit, 0),
            &payload[..len],
            |event| events.push(event),
        ),
        BulkProtocolAction::AcceptedEvent
    ));
    assert_eq!(
        events,
        [BulkDispatchEvent::CrcStart, BulkDispatchEvent::CrcEnd]
    );
}

fn full_upload() -> (BulkTransferCoordinator, u32) {
    let mut coordinator = BulkTransferCoordinator::new(buffers());
    let mut payload = [0; MAX_PAYLOAD_BYTES];
    let len = encode_store_bulk_open_request(
        &StoreBulkOpenRequest {
            direction: TransferDirection::Upload,
            backend: StorageBackend::Sd,
            requested_window: 4,
            requested_chunk: 512,
            name: "book.binbook",
            total_size: 2_048,
            expected_crc32: 0,
        },
        &mut payload,
    )
    .unwrap();
    let BulkProtocolAction::Owner { request: open, .. } = dispatch_bulk_protocol(
        &mut coordinator,
        header(FrameKind::Request, Opcode::StoreBulkOpen, 1),
        &payload[..len],
    ) else {
        panic!("expected owner open")
    };
    let opened = match coordinator
        .complete_owner(complete(open, Response::UploadStarted(41)))
        .unwrap()
    {
        BulkOwnerEvent::Opened(value) => value,
        _ => panic!("expected open"),
    };
    for index in 0..4 {
        let len = encode_store_bulk_data(
            &StoreBulkData {
                stream_id: opened.stream_id,
                offset: index * 512,
                end_of_data: index == 3,
                data: &[9; 512],
            },
            &mut payload,
        )
        .unwrap();
        let _ = dispatch_bulk_protocol(
            &mut coordinator,
            header(FrameKind::Event, Opcode::StoreBulkData, 0),
            &payload[..len],
        );
    }
    (coordinator, opened.stream_id)
}
