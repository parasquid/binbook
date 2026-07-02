#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::{
    encode_frame, encode_key_payload, FrameHeader, FrameKind, KeyAction, KeyCode, Opcode,
    PanelModeCode, Status, MAX_FRAME_BYTES,
};
use binbook_fw::{
    diag::{
        complete_pending_command, poll_runtime_command, DiagnosticSnapshot, PendingAction,
        RuntimeCommand, SerialState,
    },
    input::{apply_page_turn, PageTurn},
};

fn key_frame(sequence: u16, key: KeyCode) -> ([u8; MAX_FRAME_BYTES], usize) {
    let mut payload = [0u8; 2];
    let payload_len = encode_key_payload(key, KeyAction::Press, &mut payload).unwrap();
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Key,
        status: Status::Ok,
        sequence,
        payload_len: payload_len as u16,
    };
    let mut frame = [0u8; MAX_FRAME_BYTES];
    let frame_len = encode_frame(&header, &payload[..payload_len], &mut frame).unwrap();
    (frame, frame_len)
}

#[test]
fn accepted_boundary_burst_keeps_fifo_relative_intent_and_completes_every_sequence() {
    let snapshot = DiagnosticSnapshot {
        current_page: 0,
        page_count: 16,
        panel_mode: PanelModeCode::Grayscale,
        dropped_log_count: 0,
        protocol_error_count: 0,
        last_error: 0,
    };
    let keys = [
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Up,
        KeyCode::Up,
        KeyCode::Down,
    ];
    let mut serial = SerialState::new();
    let mut accepted = [None; 5];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;

    for (index, key) in keys.into_iter().enumerate() {
        let sequence = 100 + index as u16;
        let (frame, frame_len) = key_frame(sequence, key);
        serial.feed_rx(&frame[..frame_len]);
        let command = poll_runtime_command(&mut serial, snapshot, &mut storage)
            .expect("every accepted key must enter the hardware completion path");
        let RuntimeCommand::Hardware(pending) = command else {
            panic!("sequence {sequence} bypassed the hardware completion path");
        };
        accepted[index] = Some(pending);
        assert!(serial.pending_tx().is_empty());
    }

    let mut page = 0;
    let mut pages = [0; 5];
    let mut response_sequences = [0; 5];
    for (index, pending) in accepted.into_iter().flatten().enumerate() {
        let PendingAction::RenderTurn { turn } = pending.action else {
            panic!(
                "sequence {} was not accepted as a relative turn",
                pending.header.sequence
            );
        };
        page = apply_page_turn(page, snapshot.page_count, turn);
        pages[index] = page;
        complete_pending_command(&mut serial, pending, Status::Ok, page, &[]).unwrap();
        let mut response_payload = [0u8; 8];
        let (response, _) =
            binbook_diagnostic_protocol::decode_frame(serial.pending_tx(), &mut response_payload)
                .unwrap();
        response_sequences[index] = response.sequence;
        let response_len = serial.pending_tx().len();
        serial.consume_tx(response_len);
    }

    assert_eq!(pages, [0, 1, 0, 0, 1]);
    assert_eq!(response_sequences, [100, 101, 102, 103, 104]);
}

#[test]
fn boundary_key_is_not_completed_immediately_at_dispatch() {
    let snapshot = DiagnosticSnapshot {
        current_page: 0,
        page_count: 16,
        panel_mode: PanelModeCode::Grayscale,
        dropped_log_count: 0,
        protocol_error_count: 0,
        last_error: 0,
    };
    let mut serial = SerialState::new();
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let (frame, frame_len) = key_frame(77, KeyCode::Up);
    serial.feed_rx(&frame[..frame_len]);

    let command = poll_runtime_command(&mut serial, snapshot, &mut storage).unwrap();

    assert_eq!(
        command,
        RuntimeCommand::Hardware(binbook_fw::diag::PendingCommand {
            header: FrameHeader {
                kind: FrameKind::Request,
                opcode: Opcode::Key,
                status: Status::Ok,
                sequence: 77,
                payload_len: 2,
            },
            action: PendingAction::RenderTurn {
                turn: PageTurn::Previous,
            },
        })
    );
    assert!(serial.pending_tx().is_empty());
}
