#![cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]

use binbook_diagnostic_protocol::{
    encode_frame, encode_store_bulk_open_request, FrameHeader, FrameKind, Opcode, Status,
    StorageBackend, StoreBulkOpenRequest, TransferDirection, MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES,
};
use binbook_fw::{
    bulk_protocol_dispatch::BulkProtocolAction,
    bulk_slots::{TRANSFER_DATA_BYTES, TRANSFER_SLOT_COUNT},
    bulk_transfer_coordinator::BulkTransferCoordinator,
    diag::{
        poll_runtime_command_bulk_owner_storage, DiagnosticSnapshot, RuntimeCommand, SerialState,
    },
};

fn buffers() -> [&'static mut [u8; TRANSFER_DATA_BYTES]; TRANSFER_SLOT_COUNT] {
    core::array::from_fn(|_| Box::leak(Box::new([0u8; TRANSFER_DATA_BYTES])))
}

#[test]
fn diagnostic_decode_routes_bulk_open_to_owner_without_queuing_a_response() {
    let mut payload = [0; MAX_PAYLOAD_BYTES];
    let payload_len = encode_store_bulk_open_request(
        &StoreBulkOpenRequest {
            direction: TransferDirection::Upload,
            backend: StorageBackend::Sd,
            requested_window: 4,
            requested_chunk: 512,
            name: "book.binbook",
            total_size: 512,
            expected_crc32: 0,
        },
        &mut payload,
    )
    .unwrap();
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::StoreBulkOpen,
        status: Status::Ok,
        sequence: 17,
        payload_len: payload_len as u16,
    };
    let mut frame = [0; MAX_FRAME_BYTES];
    let frame_len = encode_frame(&header, &payload[..payload_len], &mut frame).unwrap();
    let mut serial = SerialState::new();
    serial.feed_rx(&frame[..frame_len]);
    let mut transfer = [0; 4096];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let mut bulk = BulkTransferCoordinator::new(buffers());

    let command = poll_runtime_command_bulk_owner_storage(
        &mut serial,
        DiagnosticSnapshot {
            current_page: 0,
            page_count: 1,
            panel_mode: binbook_diagnostic_protocol::PanelModeCode::Unknown,
            refresh_mode: binbook_diagnostic_protocol::RefreshModeCode::Balanced,
            dropped_log_count: 0,
            protocol_error_count: 0,
            last_error: 0,
        },
        &mut storage,
        &mut transfer,
        &mut bulk,
    )
    .unwrap();
    let RuntimeCommand::Bulk {
        action:
            BulkProtocolAction::Owner {
                reply_to: Some(reply),
                ..
            },
        ..
    } = command
    else {
        panic!("bulk open must route to non-blocking owner action")
    };
    assert_eq!(reply.sequence, 17);
    assert!(serial.pending_tx().is_empty());
    assert_eq!(bulk.pending_owner(), 1);
}
