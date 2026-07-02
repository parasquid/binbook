#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::{
    decode_frame, encode_frame, FrameHeader, FrameKind, KeyAction, KeyCode, Opcode, PageAction,
    Status, ALL_CAPABILITIES, MAX_FRAME_BYTES,
};
use binbook_fw::{
    diag::{dispatch_command, CommandContext, DispatchResult, DisplayProbeKind},
    input::PageTurn,
};

struct DispatchFixture {
    context: CommandContext,
    frame: [u8; MAX_FRAME_BYTES],
    decoded_payload: [u8; binbook_diagnostic_protocol::MAX_PAYLOAD_BYTES],
    response: [u8; MAX_FRAME_BYTES],
}

impl DispatchFixture {
    fn new(current_page: u32, page_count: u32) -> Self {
        Self {
            context: CommandContext::new(current_page, page_count, 0, 0),
            frame: [0; MAX_FRAME_BYTES],
            decoded_payload: [0; binbook_diagnostic_protocol::MAX_PAYLOAD_BYTES],
            response: [0; MAX_FRAME_BYTES],
        }
    }

    fn request(&mut self, header: FrameHeader, payload: &[u8]) -> DispatchResult {
        let frame_len = encode_frame(&header, payload, &mut self.frame).unwrap();
        let (decoded_header, payload_len) =
            decode_frame(&self.frame[..frame_len], &mut self.decoded_payload).unwrap();
        let mut storage = binbook_fw::diag_storage::UnavailableStorage;
        dispatch_command(
            decoded_header,
            &self.decoded_payload[..payload_len],
            &mut self.context,
            &mut self.response,
            &mut storage,
        )
    }
}

fn request_header(opcode: Opcode, sequence: u16, payload_len: u16) -> FrameHeader {
    FrameHeader {
        kind: FrameKind::Request,
        opcode,
        status: Status::Ok,
        sequence,
        payload_len,
    }
}

#[test]
fn acceptance_hello_and_status_return_decodable_live_payloads() {
    let mut fixture = DispatchFixture::new(5, 8);

    let hello = fixture.request(request_header(Opcode::Hello, 10, 0), &[]);
    let hello_len = match hello {
        DispatchResult::Response {
            status: Status::Ok,
            payload_len,
        } => payload_len,
        other => panic!("HELLO expected Ok response, got {other:?}"),
    };
    let hello =
        binbook_diagnostic_protocol::decode_hello_response(&fixture.response[..hello_len]).unwrap();
    assert_eq!(hello.protocol_version, 2);
    assert_eq!(hello.max_frame_bytes, 4126);
    assert_eq!(hello.capabilities & ALL_CAPABILITIES, ALL_CAPABILITIES);

    let status = fixture.request(request_header(Opcode::Status, 11, 0), &[]);
    let status_len = match status {
        DispatchResult::Response {
            status: Status::Ok,
            payload_len,
        } => payload_len,
        other => panic!("STATUS expected Ok response, got {other:?}"),
    };
    let status =
        binbook_diagnostic_protocol::decode_status_payload(&fixture.response[..status_len])
            .unwrap();
    assert_eq!((status.current_page, status.page_count), (5, 8));
}

#[test]
fn acceptance_navigation_preserves_intent_without_mutating_committed_page() {
    let mut fixture = DispatchFixture::new(3, 8);
    let mut goto = [0u8; 5];
    goto[0] = PageAction::Goto as u8;
    goto[1..].copy_from_slice(&0u32.to_le_bytes());

    assert_eq!(
        fixture.request(request_header(Opcode::Page, 20, 5), &goto),
        DispatchResult::RenderPage { target_page: 0 }
    );
    assert_eq!(fixture.context.current_page, 3);
    assert_eq!(
        fixture.request(
            request_header(Opcode::Key, 21, 2),
            &[KeyCode::Right as u8, KeyAction::Press as u8],
        ),
        DispatchResult::RenderTurn {
            turn: PageTurn::Next,
        }
    );
    assert_eq!(
        fixture.request(
            request_header(Opcode::Key, 22, 2),
            &[KeyCode::Left as u8, KeyAction::Press as u8],
        ),
        DispatchResult::RenderTurn {
            turn: PageTurn::Previous,
        }
    );
}

#[test]
fn acceptance_hardware_commands_remain_deferred_actions() {
    let mut fixture = DispatchFixture::new(3, 8);

    assert_eq!(
        fixture.request(request_header(Opcode::CrashGet, 30, 0), &[]),
        DispatchResult::CrashGet
    );
    assert_eq!(
        fixture.request(request_header(Opcode::CrashClear, 31, 0), &[]),
        DispatchResult::CrashClear
    );
    for (code, expected) in [
        (0x01u8, DisplayProbeKind::FullRefreshCurrent),
        (0x02, DisplayProbeKind::ClearWhite),
        (0x03, DisplayProbeKind::WindowCorners),
    ] {
        assert_eq!(
            fixture.request(
                request_header(Opcode::DisplayProbe, 40 + u16::from(code), 1),
                &[code],
            ),
            DispatchResult::DisplayProbe(expected)
        );
    }
}

#[test]
fn acceptance_malformed_commands_are_rejected_without_actions() {
    let mut fixture = DispatchFixture::new(0, 8);
    let malformed = [
        (Opcode::Page, &[PageAction::Goto as u8, 0, 1][..]),
        (Opcode::Key, &[KeyCode::Right as u8][..]),
        (Opcode::DisplayProbe, &[][..]),
        (Opcode::DisplayProbe, &[0xff][..]),
    ];

    for (index, (opcode, payload)) in malformed.into_iter().enumerate() {
        let result = fixture.request(
            request_header(
                opcode,
                50 + u16::try_from(index).unwrap(),
                u16::try_from(payload.len()).unwrap(),
            ),
            payload,
        );
        assert!(matches!(
            result,
            DispatchResult::Response {
                status: Status::BadRequest,
                ..
            }
        ));
    }
}
