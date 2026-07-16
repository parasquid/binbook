#![cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]

use binbook_diagnostic_protocol::{
    decode_transfer_trace_snapshot, FrameDecodeEvent, Status, TransferDirection,
    TransferTraceAction, TransferTraceRequest,
};
use binbook_fw::transfer_trace_command::TransferTraceCommand;
use binbook_fw::{
    storage_owner::StorageClient,
    transfer_block_io::BlockIoAggregate,
    transfer_owner_timing::{OwnerTimingEvent, OwnerTimingPhase},
};
use embedded_sd_storage::{BlockIoEvent, BlockIoOperation, BlockIoPhase};

#[test]
fn trace_command_start_get_and_clear_return_exact_control_results() {
    let mut command = TransferTraceCommand::new();
    let started = command
        .handle(
            TransferTraceRequest {
                action: TransferTraceAction::Start,
                direction: TransferDirection::Upload,
                trace_id: 0,
            },
            0,
            10,
        )
        .unwrap()
        .unwrap();
    let snapshot = decode_transfer_trace_snapshot(&started).unwrap();
    assert_ne!(snapshot.trace_id, 0);
    assert_eq!(snapshot.direction, TransferDirection::Upload);
    assert_eq!(snapshot.selected_window, 2);
    assert_eq!(snapshot.chunk_size, 3_584);

    let got = command
        .handle(
            TransferTraceRequest {
                action: TransferTraceAction::Get,
                direction: TransferDirection::Unspecified,
                trace_id: snapshot.trace_id,
            },
            0,
            20,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        decode_transfer_trace_snapshot(&got).unwrap().trace_id,
        snapshot.trace_id
    );
    assert_eq!(
        command.handle(
            TransferTraceRequest {
                action: TransferTraceAction::Clear,
                direction: TransferDirection::Unspecified,
                trace_id: snapshot.trace_id,
            },
            0,
            30
        ),
        Err(Status::BadRequest)
    );
}

#[test]
fn negotiated_bulk_shape_replaces_trace_defaults() {
    let mut command = TransferTraceCommand::new();
    let started = command
        .handle(
            TransferTraceRequest {
                action: TransferTraceAction::Start,
                direction: TransferDirection::Readback,
                trace_id: 0,
            },
            0,
            10,
        )
        .unwrap()
        .unwrap();
    let trace_id = decode_transfer_trace_snapshot(&started).unwrap().trace_id;

    command.set_negotiated_shape(4, 3_072);
    let snapshot = command
        .handle(
            TransferTraceRequest {
                action: TransferTraceAction::Get,
                direction: TransferDirection::Unspecified,
                trace_id,
            },
            0,
            20,
        )
        .unwrap()
        .map(|payload| decode_transfer_trace_snapshot(&payload).unwrap())
        .unwrap();

    assert_eq!(snapshot.selected_window, 4);
    assert_eq!(snapshot.chunk_size, 3_072);
}

#[test]
fn aggregated_block_io_records_long_bursts_without_phase_event_storage() {
    let mut command = TransferTraceCommand::new();
    let started = command
        .handle(
            TransferTraceRequest {
                action: TransferTraceAction::Start,
                direction: TransferDirection::Upload,
                trace_id: 0,
            },
            0,
            10,
        )
        .unwrap()
        .unwrap();
    let trace_id = decode_transfer_trace_snapshot(&started).unwrap().trace_id;
    command.observe_block_io_aggregate(BlockIoAggregate {
        read_calls: 40,
        read_blocks: 50,
        read_us: 700,
        write_calls: 100,
        write_blocks: 800,
        write_us: 1_100,
        anomalies: 0,
    });
    let snapshot = command
        .handle(
            TransferTraceRequest {
                action: TransferTraceAction::Get,
                direction: TransferDirection::Unspecified,
                trace_id,
            },
            0,
            2_000,
        )
        .unwrap()
        .map(|payload| decode_transfer_trace_snapshot(&payload).unwrap())
        .unwrap();
    assert_eq!((snapshot.sd_read_calls, snapshot.sd_read_blocks), (40, 50));
    assert_eq!(
        (snapshot.sd_write_calls, snapshot.sd_write_blocks),
        (100, 800)
    );
    assert_eq!((snapshot.sd_read_us, snapshot.sd_write_us), (700, 1_100));
    assert_eq!(snapshot.sd_write_logical_bytes, 800 * 512);
}

#[test]
fn trace_command_rejects_restarts_and_wrong_ids_without_replacing_active_trace() {
    let mut command = TransferTraceCommand::new();
    let request = TransferTraceRequest {
        action: TransferTraceAction::Start,
        direction: TransferDirection::Readback,
        trace_id: 0,
    };
    command.handle(request, 4, 1).unwrap();
    assert_eq!(command.handle(request, 4, 2), Err(Status::BadRequest));
    assert_eq!(
        command.handle(
            TransferTraceRequest {
                action: TransferTraceAction::Get,
                direction: TransferDirection::Unspecified,
                trace_id: 999,
            },
            4,
            3
        ),
        Err(Status::NotFound)
    );
}

#[test]
fn decode_events_record_separate_framing_crc_and_frame_bytes() {
    let mut command = TransferTraceCommand::new();
    let started = command
        .handle(
            TransferTraceRequest {
                action: TransferTraceAction::Start,
                direction: TransferDirection::Upload,
                trace_id: 0,
            },
            0,
            100,
        )
        .unwrap()
        .unwrap();
    let trace_id = decode_transfer_trace_snapshot(&started).unwrap().trace_id;

    command.observe_decode(FrameDecodeEvent::FramingStart, 100);
    command.observe_decode(FrameDecodeEvent::FramingEnd, 105);
    command.observe_decode(FrameDecodeEvent::CrcStart, 106);
    command.observe_decode(FrameDecodeEvent::CrcEnd, 109);
    command.observe_decode(
        FrameDecodeEvent::Complete {
            logical_bytes: 512,
            encoded_bytes: 530,
        },
        109,
    );
    command.mark_commit_sequence(8);
    command.observe_owner_timing(OwnerTimingEvent {
        client: StorageClient::Diagnostic,
        sequence: 8,
        phase: OwnerTimingPhase::Queued,
        timestamp_us: 101,
    });
    command.observe_block_io(
        BlockIoEvent {
            operation: BlockIoOperation::Read,
            phase: BlockIoPhase::Start,
            start_block: 7,
            blocks: 3,
            success: false,
        },
        110,
    );
    command.observe_block_io(
        BlockIoEvent {
            operation: BlockIoOperation::Read,
            phase: BlockIoPhase::Complete,
            start_block: 7,
            blocks: 3,
            success: true,
        },
        117,
    );
    command.observe_owner_timing(OwnerTimingEvent {
        client: StorageClient::Diagnostic,
        sequence: 8,
        phase: OwnerTimingPhase::Executing,
        timestamp_us: 105,
    });
    command.observe_owner_timing(OwnerTimingEvent {
        client: StorageClient::Diagnostic,
        sequence: 8,
        phase: OwnerTimingPhase::Completed,
        timestamp_us: 108,
    });

    let payload = command
        .handle(
            TransferTraceRequest {
                action: TransferTraceAction::Get,
                direction: TransferDirection::Unspecified,
                trace_id,
            },
            0,
            120,
        )
        .unwrap()
        .unwrap();
    let snapshot = decode_transfer_trace_snapshot(&payload).unwrap();
    assert_eq!(snapshot.framing_us, 5);
    assert_eq!(snapshot.frame_crc_us, 3);
    assert_eq!(snapshot.frames, 1);
    assert_eq!(snapshot.logical_bytes, 512);
    assert_eq!(snapshot.usb_rx_bytes, 530);
    assert_eq!(snapshot.owner_queue_us, 4);
    assert_eq!(snapshot.owner_execute_us, 3);
    assert_eq!(snapshot.commit_us, 3);
    assert_eq!(snapshot.sd_read_us, 7);
    assert_eq!(snapshot.sd_read_calls, 1);
    assert_eq!(snapshot.sd_read_blocks, 3);
    assert_eq!(snapshot.sd_read_logical_bytes, 1_536);
}
