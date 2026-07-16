#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::{
    decode_transfer_trace_snapshot, TransferDirection, TransferTraceAction, TransferTraceRequest,
    TransferTraceState,
};
use binbook_fw::{
    legacy_transfer_trace::LegacyTraceLifecycle, transfer_trace_command::TransferTraceCommand,
};

#[test]
fn legacy_readback_eof_and_upload_commit_close_their_data_phase() {
    for direction in [TransferDirection::Readback, TransferDirection::Upload] {
        let mut trace = TransferTraceCommand::new();
        let started = trace
            .handle(
                TransferTraceRequest {
                    action: TransferTraceAction::Start,
                    direction,
                    trace_id: 0,
                },
                0,
                10,
            )
            .unwrap()
            .unwrap();
        let id = decode_transfer_trace_snapshot(&started).unwrap().trace_id;
        let mut lifecycle = LegacyTraceLifecycle::new();
        lifecycle.start_data(direction, &mut trace, 20);
        lifecycle.finish(
            direction,
            TransferTraceState::Succeeded,
            0,
            70,
            direction == TransferDirection::Upload,
            &mut trace,
        );
        let payload = trace
            .handle(
                TransferTraceRequest {
                    action: TransferTraceAction::Get,
                    direction: TransferDirection::Unspecified,
                    trace_id: id,
                },
                0,
                80,
            )
            .unwrap()
            .unwrap();
        let snapshot = decode_transfer_trace_snapshot(&payload).unwrap();
        assert_eq!(snapshot.state, TransferTraceState::Succeeded);
        assert_eq!(snapshot.data_phase_us, 50);
    }
}

#[test]
fn failed_legacy_readback_closes_trace_and_allows_the_next_transfer() {
    let mut trace = TransferTraceCommand::new();
    let started = trace
        .handle(
            TransferTraceRequest {
                action: TransferTraceAction::Start,
                direction: TransferDirection::Readback,
                trace_id: 0,
            },
            4,
            10,
        )
        .unwrap()
        .unwrap();
    let id = decode_transfer_trace_snapshot(&started).unwrap().trace_id;
    let mut lifecycle = LegacyTraceLifecycle::new();
    lifecycle.start_data(TransferDirection::Readback, &mut trace, 20);
    lifecycle.finish(
        TransferDirection::Readback,
        TransferTraceState::Failed,
        5,
        35,
        false,
        &mut trace,
    );

    let failed = trace
        .handle(
            TransferTraceRequest {
                action: TransferTraceAction::Get,
                direction: TransferDirection::Unspecified,
                trace_id: id,
            },
            5,
            40,
        )
        .unwrap()
        .unwrap();
    let snapshot = decode_transfer_trace_snapshot(&failed).unwrap();
    assert_eq!(snapshot.state, TransferTraceState::Failed);
    assert_eq!(snapshot.data_phase_us, 15);
    assert_eq!(snapshot.protocol_errors_start, 4);
    assert_eq!(snapshot.protocol_errors_end, 5);

    trace
        .handle(
            TransferTraceRequest {
                action: TransferTraceAction::Clear,
                direction: TransferDirection::Unspecified,
                trace_id: id,
            },
            5,
            41,
        )
        .unwrap();
    assert!(trace
        .handle(
            TransferTraceRequest {
                action: TransferTraceAction::Start,
                direction: TransferDirection::Upload,
                trace_id: 0,
            },
            5,
            42,
        )
        .is_ok());
}
