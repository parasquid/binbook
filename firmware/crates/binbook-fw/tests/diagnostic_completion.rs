#![cfg(feature = "diagnostic-console")]

use binbook_fw::input::PageTurn;
#[test]
fn diagnostic_page_response_is_queued_only_after_action_completion() {
    use binbook_diagnostic_protocol::{
        decode_frame, encode_frame, encode_page_payload, FrameHeader, FrameKind, Opcode,
        PageAction, Status, MAX_FRAME_BYTES,
    };
    use binbook_fw::diag::{
        complete_pending_command, poll_pending_command, PendingAction, SerialState,
    };
    use binbook_fw::diag_log::DiagLog;

    let mut payload = [0u8; 5];
    let payload_len = encode_page_payload(PageAction::Goto, Some(0), &mut payload).unwrap();
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Page,
        status: Status::Ok,
        sequence: 0x1234,
        payload_len: payload_len as u16,
    };
    let mut encoded = [0u8; MAX_FRAME_BYTES];
    let encoded_len = encode_frame(&header, &payload[..payload_len], &mut encoded).unwrap();

    let mut serial = SerialState::new();
    let mut log = DiagLog::<8>::new();
    serial.feed_rx(&encoded[..encoded_len]);
    let pending = poll_pending_command(&mut serial, 3, 8, 0, 0, &mut log, 100)
        .expect("page command should require execution");
    assert_eq!(pending.action, PendingAction::RenderPage { target_page: 0 });
    assert!(
        serial.pending_tx().is_empty(),
        "success must not be sent before render"
    );

    complete_pending_command(&mut serial, pending, Status::Ok, 0, &[]).unwrap();
    let mut decoded_payload = [0u8; 32];
    let (response, response_len) = decode_frame(serial.pending_tx(), &mut decoded_payload).unwrap();
    assert_eq!(response.sequence, 0x1234);
    assert_eq!(response.opcode, Opcode::Page);
    assert_eq!(response.status, Status::Ok);
    assert_eq!(response_len, 4);
    assert_eq!(
        u32::from_le_bytes(decoded_payload[..4].try_into().unwrap()),
        0
    );
}

#[cfg(feature = "diagnostic-console")]
struct AsyncDiagHarness {
    current_page: u32,
    page_count: u32,
    serial: binbook_fw::diag::SerialState,
    log: binbook_fw::diag_log::DiagLog<8>,
    pending: Vec<binbook_fw::diag::PendingCommand>,
    received_turns: Vec<PageTurn>,
    response_sequences: Vec<u16>,
}

#[cfg(feature = "diagnostic-console")]
impl AsyncDiagHarness {
    fn on_page(current_page: u32, page_count: u32) -> Self {
        Self {
            current_page,
            page_count,
            serial: binbook_fw::diag::SerialState::new(),
            log: binbook_fw::diag_log::DiagLog::<8>::new(),
            pending: Vec::new(),
            received_turns: Vec::new(),
            response_sequences: Vec::new(),
        }
    }

    fn receive_key(&mut self, sequence: u16, key: binbook_diagnostic_protocol::KeyCode) {
        let turn = match key {
            binbook_diagnostic_protocol::KeyCode::Right
            | binbook_diagnostic_protocol::KeyCode::Down => PageTurn::Next,
            binbook_diagnostic_protocol::KeyCode::Left
            | binbook_diagnostic_protocol::KeyCode::Up => PageTurn::Previous,
            other => panic!("unexpected key for page turn test: {:?}", other),
        };
        self.received_turns.push(turn);

        let mut payload = [0u8; 2];
        let payload_len = binbook_diagnostic_protocol::encode_key_payload(
            key,
            binbook_diagnostic_protocol::KeyAction::Press,
            &mut payload,
        )
        .unwrap();
        let header = binbook_diagnostic_protocol::FrameHeader {
            kind: binbook_diagnostic_protocol::FrameKind::Request,
            opcode: binbook_diagnostic_protocol::Opcode::Key,
            status: binbook_diagnostic_protocol::Status::Ok,
            sequence,
            payload_len: payload_len as u16,
        };
        let mut encoded = [0u8; binbook_diagnostic_protocol::MAX_FRAME_BYTES];
        let encoded_len = binbook_diagnostic_protocol::encode_frame(
            &header,
            &payload[..payload_len],
            &mut encoded,
        )
        .unwrap();
        self.serial.feed_rx(&encoded[..encoded_len]);
        let pending = binbook_fw::diag::poll_pending_command(
            &mut self.serial,
            self.current_page,
            self.page_count,
            0,
            0,
            &mut self.log,
            sequence as u32,
        )
        .expect("directional key should queue a render");
        self.response_sequences.push(pending.header.sequence);
        self.pending.push(pending);
    }

    fn pending_turns(&self) -> [PageTurn; 3] {
        self.received_turns
            .as_slice()
            .try_into()
            .expect("test harness expected exactly three queued turns")
    }

    fn rendered_pages_after_completion(&mut self) -> [u32; 3] {
        let mut rendered_pages = [0u32; 3];

        for (index, pending) in self.pending.drain(..).enumerate() {
            let target_page = match pending.action {
                binbook_fw::diag::PendingAction::RenderPage { target_page } => target_page,
                binbook_fw::diag::PendingAction::RenderTurn { turn } => {
                    binbook_fw::input::apply_page_turn(self.current_page, self.page_count, turn)
                }
                other => panic!("expected render action, got {:?}", other),
            };

            binbook_fw::diag::complete_pending_command(
                &mut self.serial,
                pending,
                binbook_diagnostic_protocol::Status::Ok,
                target_page,
                &[],
            )
            .unwrap();
            rendered_pages[index] = target_page;
            self.current_page = target_page;
        }

        rendered_pages
    }

    fn response_sequences(&self) -> [u16; 3] {
        self.response_sequences
            .as_slice()
            .try_into()
            .expect("test harness expected exactly three pending responses")
    }
}

#[test]
fn batched_key_presses_are_resolved_when_dequeued() {
    let mut harness = AsyncDiagHarness::on_page(1, 4);
    harness.receive_key(10, binbook_diagnostic_protocol::KeyCode::Right);
    harness.receive_key(11, binbook_diagnostic_protocol::KeyCode::Right);
    harness.receive_key(12, binbook_diagnostic_protocol::KeyCode::Left);

    assert_eq!(
        harness.pending_turns(),
        [PageTurn::Next, PageTurn::Next, PageTurn::Previous]
    );
    assert_eq!(harness.rendered_pages_after_completion(), [2, 3, 2]);
    assert_eq!(harness.response_sequences(), [10, 11, 12]);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diagnostic_pending_queue_rejects_the_seventeenth_command_without_evicting_old_requests() {
    use binbook_fw::diag::{DiagnosticPendingQueue, PendingAction, PendingCommand};

    let mut queue = DiagnosticPendingQueue::<16>::new();

    for sequence in 0..16u16 {
        let pending = PendingCommand {
            header: binbook_diagnostic_protocol::FrameHeader {
                kind: binbook_diagnostic_protocol::FrameKind::Request,
                opcode: binbook_diagnostic_protocol::Opcode::Key,
                status: binbook_diagnostic_protocol::Status::Ok,
                sequence,
                payload_len: 0,
            },
            action: PendingAction::RenderPage {
                target_page: sequence as u32,
            },
        };
        queue.try_push(pending).unwrap();
    }

    let overflow = PendingCommand {
        header: binbook_diagnostic_protocol::FrameHeader {
            kind: binbook_diagnostic_protocol::FrameKind::Request,
            opcode: binbook_diagnostic_protocol::Opcode::Key,
            status: binbook_diagnostic_protocol::Status::Ok,
            sequence: 16,
            payload_len: 0,
        },
        action: PendingAction::RenderPage { target_page: 16 },
    };

    assert_eq!(queue.try_push(overflow), Err(overflow));
    assert_eq!(queue.len(), 16);
    assert_eq!(queue.front().unwrap().header.sequence, 0);
}
