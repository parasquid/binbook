use binbook_cli::protocol::{delete_command, list_command, upload_command};
use binbook_diagnostic_protocol::{
    decode_frame, encode_frame, FrameHeader, FrameKind, KeyCode, Opcode,
    PageAction as ProtoPageAction, Status,
};
use clap::Parser;

#[test]
fn formats_serial_protocol_commands() {
    assert_eq!(list_command(), "LIST\n");
    assert_eq!(delete_command("sample.binbook"), "DELETE sample.binbook\n");
    assert_eq!(
        upload_command("sample.binbook", 12345),
        "UPLOAD sample.binbook 12345\n",
    );
}

#[test]
fn diag_hello_request_encodes_valid_frame() {
    let frame = binbook_cli::diag_protocol::hello_request(1);
    let mut payload_buf = [0u8; 128];
    let (header, _payload_len) = decode_frame(&frame, &mut payload_buf).unwrap();
    assert_eq!(header.kind, FrameKind::Request);
    assert_eq!(header.opcode, Opcode::Hello);
    assert_eq!(header.status, Status::Ok);
    assert_eq!(header.sequence, 1);
}

#[test]
fn diag_key_request_encodes_valid_frame() {
    let frame = binbook_cli::diag_protocol::key_request(2, KeyCode::Right);
    let mut payload_buf = [0u8; 128];
    let (header, payload_len) = decode_frame(&frame, &mut payload_buf).unwrap();
    assert_eq!(header.kind, FrameKind::Request);
    assert_eq!(header.opcode, Opcode::Key);
    assert_eq!(header.sequence, 2);
    assert_eq!(payload_len, 2);
    assert_eq!(payload_buf[0], KeyCode::Right as u8);
    assert_eq!(payload_buf[1], 0x01); // KeyAction::Press
}

#[test]
fn diag_status_request_encodes_valid_frame() {
    let frame = binbook_cli::diag_protocol::status_request(4);
    let mut payload_buf = [0u8; 128];
    let (header, _payload_len) = decode_frame(&frame, &mut payload_buf).unwrap();
    assert_eq!(header.kind, FrameKind::Request);
    assert_eq!(header.opcode, Opcode::Status);
    assert_eq!(header.sequence, 4);
}

#[test]
fn cli_page_goto_encodes_full_u32() {
    let frame = binbook_cli::diag_protocol::page_goto_request(10, 0x0102_0304);
    let mut payload_buf = [0u8; 128];
    let (header, payload_len) = decode_frame(&frame, &mut payload_buf).unwrap();
    assert_eq!(header.opcode, Opcode::Page);
    assert_eq!(header.sequence, 10);
    assert_eq!(payload_len, 5);
    assert_eq!(payload_buf[0], ProtoPageAction::Goto as u8);
    assert_eq!(payload_buf[1..5], 0x0102_0304u32.to_le_bytes());
}

#[test]
fn cli_page_next_and_previous_use_page_action_values() {
    let next = binbook_cli::diag_protocol::page_action_request(20, ProtoPageAction::Next);
    let prev = binbook_cli::diag_protocol::page_action_request(21, ProtoPageAction::Previous);
    let mut buf = [0u8; 128];

    let (_, nl) = decode_frame(&next, &mut buf).unwrap();
    assert_eq!(nl, 1);
    assert_eq!(buf[0], ProtoPageAction::Next as u8);

    let (_, pl) = decode_frame(&prev, &mut buf).unwrap();
    assert_eq!(pl, 1);
    assert_eq!(buf[0], ProtoPageAction::Previous as u8);
}

#[test]
fn cli_page_first_last_and_current_use_page_action_values() {
    let first = binbook_cli::diag_protocol::page_action_request(30, ProtoPageAction::First);
    let last = binbook_cli::diag_protocol::page_action_request(31, ProtoPageAction::Last);
    let current = binbook_cli::diag_protocol::page_action_request(32, ProtoPageAction::Current);
    let mut buf = [0u8; 128];

    let (_, fl) = decode_frame(&first, &mut buf).unwrap();
    assert_eq!(fl, 1);
    assert_eq!(buf[0], ProtoPageAction::First as u8);

    let (_, ll) = decode_frame(&last, &mut buf).unwrap();
    assert_eq!(ll, 1);
    assert_eq!(buf[0], ProtoPageAction::Last as u8);

    let (_, cl) = decode_frame(&current, &mut buf).unwrap();
    assert_eq!(cl, 1);
    assert_eq!(buf[0], ProtoPageAction::Current as u8);
}

#[test]
fn cli_logs_since_encodes_cursor_and_budget() {
    let frame = binbook_cli::diag_protocol::log_get_request(50, 42, 256);
    let mut payload_buf = [0u8; 128];
    let (header, payload_len) = decode_frame(&frame, &mut payload_buf).unwrap();
    assert_eq!(header.opcode, Opcode::LogGet);
    assert_eq!(header.sequence, 50);
    assert_eq!(payload_len, 6);
    assert_eq!(payload_buf[0..4], 42u32.to_le_bytes());
    assert_eq!(payload_buf[4..6], 256u16.to_le_bytes());
}

#[test]
fn cli_logs_clear_sends_log_clear() {
    let frame = binbook_cli::diag_protocol::log_clear_request(51);
    let mut payload_buf = [0u8; 128];
    let (header, _) = decode_frame(&frame, &mut payload_buf).unwrap();
    assert_eq!(header.opcode, Opcode::LogClear);
    assert_eq!(header.sequence, 51);
}

#[test]
fn cli_crash_get_sends_crash_get() {
    let frame = binbook_cli::diag_protocol::crash_get_request(52);
    let mut payload_buf = [0u8; 128];
    let (header, _) = decode_frame(&frame, &mut payload_buf).unwrap();
    assert_eq!(header.opcode, Opcode::CrashGet);
    assert_eq!(header.sequence, 52);
}

#[test]
fn cli_crash_clear_sends_crash_clear() {
    let frame = binbook_cli::diag_protocol::crash_clear_request(53);
    let mut payload_buf = [0u8; 128];
    let (header, _) = decode_frame(&frame, &mut payload_buf).unwrap();
    assert_eq!(header.opcode, Opcode::CrashClear);
    assert_eq!(header.sequence, 53);
}

#[test]
fn cli_probe_supports_all_three_probe_codes() {
    let probes = [
        (
            binbook_cli::diag_protocol::ProbeChoice::FullRefreshCurrent,
            0x01u8,
        ),
        (binbook_cli::diag_protocol::ProbeChoice::ClearWhite, 0x02u8),
        (
            binbook_cli::diag_protocol::ProbeChoice::WindowCorners,
            0x03u8,
        ),
    ];
    for (i, (choice, expected_code)) in probes.iter().enumerate() {
        let frame = binbook_cli::diag_protocol::display_probe_request(60 + i as u16, *choice);
        let mut payload_buf = [0u8; 128];
        let (header, payload_len) = decode_frame(&frame, &mut payload_buf).unwrap();
        assert_eq!(header.opcode, Opcode::DisplayProbe);
        assert_eq!(header.sequence, 60 + i as u16);
        assert_eq!(payload_len, 1);
        assert_eq!(payload_buf[0], *expected_code);
    }
}

#[test]
fn cli_status_decodes_canonical_u32_layout() {
    let payload = binbook_diagnostic_protocol::StatusPayload {
        current_page: 70_001,
        page_count: 80_002,
        panel_mode: binbook_diagnostic_protocol::PanelModeCode::Grayscale,
        dropped_log_count: 90_003,
        protocol_error_count: 100_004,
        last_error: -12,
    };
    let header = FrameHeader {
        kind: FrameKind::Response,
        opcode: Opcode::Status,
        status: Status::Ok,
        sequence: 99,
        payload_len: 0,
    };
    let mut payload_buf = [0u8; 64];
    let plen =
        binbook_diagnostic_protocol::encode_status_payload(payload, &mut payload_buf).unwrap();

    let mut frame_buf = [0u8; 512];
    let mut hdr = header;
    hdr.payload_len = plen as u16;
    let frame_len = encode_frame(&hdr, &payload_buf[..plen], &mut frame_buf).unwrap();

    let decoded =
        binbook_cli::diag_protocol::decode_status_response(&frame_buf[..frame_len]).unwrap();
    assert_eq!(decoded.current_page, 70_001);
    assert_eq!(decoded.page_count, 80_002);
    assert_eq!(decoded.dropped_log_count, 90_003);
    assert_eq!(decoded.protocol_error_count, 100_004);
    assert_eq!(decoded.panel_mode, 1);
    assert_eq!(decoded.last_error, -12);
}

#[test]
fn cli_hello_formats_identity_and_capabilities() {
    let mut payload = [0u8; 64];
    let payload_len = binbook_diagnostic_protocol::encode_hello_response(
        &binbook_diagnostic_protocol::HelloResponse {
            protocol_version: 1,
            max_frame_bytes: 512,
            capabilities: binbook_diagnostic_protocol::ALL_CAPABILITIES,
            firmware_name: "binbook-fw",
            target: "xteink-x4",
        },
        &mut payload,
    )
    .unwrap();
    let frame = response_frame(Opcode::Hello, 7, Status::Ok, &payload[..payload_len]);
    let text = binbook_cli::diag_protocol::format_response(&frame, Opcode::Hello, 7).unwrap();
    assert!(text.contains("firmware=binbook-fw"));
    assert!(text.contains("target=xteink-x4"));
    assert!(text.contains("KEY,PAGE,STATUS,LOG,CRASH,DISPLAY_PROBE"));
}

#[test]
fn cli_logs_formats_event_names_and_sequences() {
    let mut payload = [0u8; 64];
    let header_len = binbook_diagnostic_protocol::encode_log_response_header(
        binbook_diagnostic_protocol::LogResponseHeader {
            next_cursor: 12,
            dropped_log_count: 3,
            record_count: 1,
        },
        &mut payload,
    )
    .unwrap();
    let record_len = binbook_diagnostic_protocol::encode_log_record(
        binbook_diagnostic_protocol::LogRecordPayload {
            sequence: 11,
            tick_ms: 1234,
            level: 2,
            subsystem: 3,
            event: binbook_diagnostic_protocol::EVT_RENDER_SUCCESS,
            arg0: 4,
            arg1: 0,
            arg2: 0,
        },
        &mut payload[header_len..],
    )
    .unwrap();
    let frame = response_frame(
        Opcode::LogGet,
        8,
        Status::Ok,
        &payload[..header_len + record_len],
    );
    let text = binbook_cli::diag_protocol::format_response(&frame, Opcode::LogGet, 8).unwrap();
    assert!(text.contains("seq=11"));
    assert!(text.contains("RENDER_SUCCESS"));
    assert!(text.contains("next_cursor=12"));

    let mut queued_payload = [0u8; 64];
    let queued_header_len = binbook_diagnostic_protocol::encode_log_response_header(
        binbook_diagnostic_protocol::LogResponseHeader {
            next_cursor: 13,
            dropped_log_count: 4,
            record_count: 1,
        },
        &mut queued_payload,
    )
    .unwrap();
    let queued_record_len = binbook_diagnostic_protocol::encode_log_record(
        binbook_diagnostic_protocol::LogRecordPayload {
            sequence: 12,
            tick_ms: 2345,
            level: 2,
            subsystem: 2,
            event: binbook_diagnostic_protocol::EVT_TURN_QUEUED,
            arg0: 1,
            arg1: 0,
            arg2: 0,
        },
        &mut queued_payload[queued_header_len..],
    )
    .unwrap();
    let queued_frame = response_frame(
        Opcode::LogGet,
        12,
        Status::Ok,
        &queued_payload[..queued_header_len + queued_record_len],
    );
    let queued_text =
        binbook_cli::diag_protocol::format_response(&queued_frame, Opcode::LogGet, 12).unwrap();
    assert!(queued_text.contains("TURN_QUEUED"));
}

#[test]
fn cli_crash_formats_empty_and_present_distinctly() {
    let empty = response_frame(Opcode::CrashGet, 9, Status::Ok, &[0]);
    assert!(
        binbook_cli::diag_protocol::format_response(&empty, Opcode::CrashGet, 9)
            .unwrap()
            .contains("crash=empty")
    );
    let mut payload = [0u8; 129];
    payload[0] = 1;
    payload[1..5].copy_from_slice(b"BBCR");
    payload[5] = 1;
    let present = response_frame(Opcode::CrashGet, 10, Status::Ok, &payload);
    assert!(
        binbook_cli::diag_protocol::format_response(&present, Opcode::CrashGet, 10)
            .unwrap()
            .contains("crash=present")
    );
}

fn response_frame(opcode: Opcode, sequence: u16, status: Status, payload: &[u8]) -> Vec<u8> {
    let header = FrameHeader {
        kind: FrameKind::Response,
        opcode,
        status,
        sequence,
        payload_len: payload.len() as u16,
    };
    let mut frame = [0u8; 512];
    let len = encode_frame(&header, payload, &mut frame).unwrap();
    frame[..len].to_vec()
}

#[test]
fn cli_probe_window_corners_subcommand_parses() {
    let cli = binbook_cli::Cli::try_parse_from([
        "binbook-cli",
        "diag",
        "probe",
        "--port",
        "/dev/ttyACM0",
        "window-corners",
    ]);
    assert!(
        cli.is_ok(),
        "diag probe window-corners should parse: {:?}",
        cli.err()
    );
}

#[test]
fn cli_probe_clear_white_subcommand_parses() {
    let cli = binbook_cli::Cli::try_parse_from([
        "binbook-cli",
        "diag",
        "probe",
        "--port",
        "/dev/ttyACM0",
        "clear-white",
    ]);
    assert!(
        cli.is_ok(),
        "diag probe clear-white should parse: {:?}",
        cli.err()
    );
}

#[test]
fn cli_probe_full_refresh_current_subcommand_parses() {
    let cli = binbook_cli::Cli::try_parse_from([
        "binbook-cli",
        "diag",
        "probe",
        "--port",
        "/dev/ttyACM0",
        "full-refresh-current",
    ]);
    assert!(
        cli.is_ok(),
        "diag probe full-refresh-current should parse: {:?}",
        cli.err()
    );
}

#[test]
fn cli_page_goto_subcommand_parses_u32() {
    let cli = binbook_cli::Cli::try_parse_from([
        "binbook-cli",
        "diag",
        "page",
        "--port",
        "/dev/ttyACM0",
        "goto",
        "50000",
    ]);
    assert!(
        cli.is_ok(),
        "diag page goto 50000 should parse: {:?}",
        cli.err()
    );
}

#[test]
fn cli_page_first_last_current_subcommands_parse() {
    for action in &["first", "last", "current"] {
        let cli = binbook_cli::Cli::try_parse_from([
            "binbook-cli",
            "diag",
            "page",
            "--port",
            "/dev/ttyACM0",
            action,
        ]);
        assert!(
            cli.is_ok(),
            "diag page {} should parse: {:?}",
            action,
            cli.err()
        );
    }
}

#[test]
fn diag_hello_subcommand_parses() {
    let cli = binbook_cli::Cli::try_parse_from([
        "binbook-cli",
        "diag",
        "hello",
        "--port",
        "/dev/ttyACM0",
    ]);
    assert!(cli.is_ok(), "diag hello should parse: {:?}", cli.err());
}

#[test]
fn diag_key_subcommand_parses() {
    let cli = binbook_cli::Cli::try_parse_from([
        "binbook-cli",
        "diag",
        "key",
        "--port",
        "/dev/ttyACM0",
        "RIGHT",
    ]);
    assert!(cli.is_ok(), "diag key RIGHT should parse: {:?}", cli.err());
}

#[test]
fn diag_page_next_subcommand_parses() {
    let cli = binbook_cli::Cli::try_parse_from([
        "binbook-cli",
        "diag",
        "page",
        "--port",
        "/dev/ttyACM0",
        "next",
    ]);
    assert!(cli.is_ok(), "diag page next should parse: {:?}", cli.err());
}

#[test]
fn diag_page_goto_subcommand_parses() {
    let cli = binbook_cli::Cli::try_parse_from([
        "binbook-cli",
        "diag",
        "page",
        "--port",
        "/dev/ttyACM0",
        "goto",
        "3",
    ]);
    assert!(
        cli.is_ok(),
        "diag page goto 3 should parse: {:?}",
        cli.err()
    );
}

#[test]
fn diag_status_subcommand_parses() {
    let cli = binbook_cli::Cli::try_parse_from([
        "binbook-cli",
        "diag",
        "status",
        "--port",
        "/dev/ttyACM0",
    ]);
    assert!(cli.is_ok(), "diag status should parse: {:?}", cli.err());
}

#[test]
fn deferred_gray_exercise_subcommand_parses() {
    let cli = binbook_cli::Cli::try_parse_from([
        "binbook-cli",
        "diag",
        "exercise",
        "deferred-gray",
        "--port",
        "/dev/ttyACM0",
    ]);
    assert!(
        cli.is_ok(),
        "diag exercise deferred-gray should parse: {:?}",
        cli.err()
    );
}

#[test]
fn diag_logs_since_subcommand_parses() {
    let cli = binbook_cli::Cli::try_parse_from([
        "binbook-cli",
        "diag",
        "logs",
        "--port",
        "/dev/ttyACM0",
        "--since",
        "0",
    ]);
    assert!(
        cli.is_ok(),
        "diag logs --since 0 should parse: {:?}",
        cli.err()
    );
}

#[test]
fn diag_logs_clear_subcommand_parses() {
    let cli = binbook_cli::Cli::try_parse_from([
        "binbook-cli",
        "diag",
        "logs",
        "--port",
        "/dev/ttyACM0",
        "--clear",
    ]);
    assert!(
        cli.is_ok(),
        "diag logs --clear should parse: {:?}",
        cli.err()
    );
}

#[test]
fn diag_crash_subcommand_parses() {
    let cli = binbook_cli::Cli::try_parse_from([
        "binbook-cli",
        "diag",
        "crash",
        "--port",
        "/dev/ttyACM0",
    ]);
    assert!(cli.is_ok(), "diag crash should parse: {:?}", cli.err());
}

#[test]
fn diag_crash_clear_subcommand_parses() {
    let cli = binbook_cli::Cli::try_parse_from([
        "binbook-cli",
        "diag",
        "crash",
        "--port",
        "/dev/ttyACM0",
        "--clear",
    ]);
    assert!(
        cli.is_ok(),
        "diag crash --clear should parse: {:?}",
        cli.err()
    );
}
