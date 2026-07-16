#![cfg(feature = "serial-device")]

use binbook::transfer_negotiation::{RequestedMode, RequestedWindow, SelectedMode};
use binbook::transfer_probe::probe_transfer;
use binbook::upload::{UploadError, UploadPacket, UploadTransport};
use binbook_diagnostic_protocol::{
    decode_frame, encode_frame, encode_hello_response, encode_transfer_limits, FrameHeader,
    FrameKind, HelloResponse, Opcode, Status, TransferDirection, TransferLimits,
    CAP_STORAGE_BULK_V1, CAP_STORAGE_TRANSFER_LIMITS, CAP_STORAGE_UPLOAD_WINDOW_2, MAX_FRAME_BYTES,
    MAX_PAYLOAD_BYTES, PROTOCOL_VERSION, TRANSFER_LIMIT_UPLOAD_BULK,
};

struct ProbePeer {
    capabilities: u32,
    limits: Option<TransferLimits>,
    opcodes: Vec<Opcode>,
    hello_transport_failures: u8,
}

impl UploadTransport for ProbePeer {
    fn exchange(
        &mut self,
        request: &[u8],
        opcode: Opcode,
        sequence: u16,
    ) -> Result<Vec<u8>, UploadError> {
        let mut request_payload = [0; MAX_PAYLOAD_BYTES];
        let (header, request_len) = decode_frame(request, &mut request_payload).unwrap();
        assert_eq!(header.opcode, opcode);
        assert_eq!(header.sequence, sequence);
        assert_eq!(request_len, 0);
        self.opcodes.push(opcode);
        if opcode == Opcode::Hello && self.hello_transport_failures != 0 {
            self.hello_transport_failures -= 1;
            return Err(UploadError::Transport {
                message: "injected HELLO transport failure".into(),
                cleanup_confirmed: false,
            });
        }
        let mut payload = [0; MAX_PAYLOAD_BYTES];
        let len = match opcode {
            Opcode::Hello => encode_hello_response(
                &HelloResponse {
                    protocol_version: PROTOCOL_VERSION,
                    max_frame_bytes: MAX_FRAME_BYTES as u16,
                    capabilities: self.capabilities,
                    firmware_name: "fw",
                    target: "x4",
                },
                &mut payload,
            )
            .unwrap(),
            Opcode::StoreTransferLimits => {
                encode_transfer_limits(&self.limits.unwrap(), &mut payload).unwrap()
            }
            _ => panic!("unexpected probe opcode"),
        };
        let mut frame = [0; MAX_FRAME_BYTES];
        let wrote = encode_frame(
            &FrameHeader {
                kind: FrameKind::Response,
                opcode,
                status: Status::Ok,
                sequence,
                payload_len: len as u16,
            },
            &payload[..len],
            &mut frame,
        )
        .unwrap();
        Ok(frame[..wrote].to_vec())
    }

    fn exchange_batch(&mut self, _requests: &[UploadPacket]) -> Result<Vec<Vec<u8>>, UploadError> {
        unreachable!()
    }
}

#[test]
fn probe_queries_limits_and_selects_clamped_bulk_only_with_complete_contract() {
    let mut peer = ProbePeer {
        capabilities: CAP_STORAGE_TRANSFER_LIMITS | CAP_STORAGE_BULK_V1,
        limits: Some(TransferLimits {
            flags: TRANSFER_LIMIT_UPLOAD_BULK,
            max_data_bytes: 3_584,
            legacy_upload_window: 2,
            legacy_read_window: 2,
            bulk_slots: 4,
            rx_capacity_bytes: 16_504,
            max_frame_bytes: 4_126,
            idle_timeout_ms: 60_000,
        }),
        opcodes: Vec::new(),
        hello_transport_failures: 0,
    };
    let selected = probe_transfer(
        &mut peer,
        TransferDirection::Upload,
        RequestedMode::Auto,
        RequestedWindow::Eight,
    )
    .unwrap();
    assert_eq!(selected.mode, SelectedMode::Bulk);
    assert_eq!(selected.window, 4);
    assert_eq!(selected.limits, peer.limits.unwrap());
    assert_eq!(peer.opcodes, [Opcode::Hello, Opcode::StoreTransferLimits]);
}

#[test]
fn probe_old_peer_skips_limits_and_preserves_two_frame_legacy_fallback() {
    let mut peer = ProbePeer {
        capabilities: CAP_STORAGE_UPLOAD_WINDOW_2,
        limits: None,
        opcodes: Vec::new(),
        hello_transport_failures: 0,
    };
    let selected = probe_transfer(
        &mut peer,
        TransferDirection::Upload,
        RequestedMode::Auto,
        RequestedWindow::Auto,
    )
    .unwrap();
    assert_eq!(selected.mode, SelectedMode::Legacy);
    assert_eq!(selected.window, 2);
    assert_eq!(selected.limits.legacy_upload_window, 2);
    assert_eq!(selected.limits.bulk_slots, 0);
    assert_eq!(peer.opcodes, [Opcode::Hello]);
}

#[test]
fn probe_retries_one_read_only_hello_after_transport_failure() {
    let mut peer = ProbePeer {
        capabilities: CAP_STORAGE_UPLOAD_WINDOW_2,
        limits: None,
        opcodes: Vec::new(),
        hello_transport_failures: 1,
    };

    let selected = probe_transfer(
        &mut peer,
        TransferDirection::Upload,
        RequestedMode::Auto,
        RequestedWindow::Auto,
    )
    .unwrap();

    assert_eq!(selected.mode, SelectedMode::Legacy);
    assert_eq!(peer.opcodes, [Opcode::Hello, Opcode::Hello]);
}

#[test]
fn probe_does_not_retry_hello_after_two_transport_failures() {
    let mut peer = ProbePeer {
        capabilities: CAP_STORAGE_UPLOAD_WINDOW_2,
        limits: None,
        opcodes: Vec::new(),
        hello_transport_failures: 2,
    };

    let error = probe_transfer(
        &mut peer,
        TransferDirection::Upload,
        RequestedMode::Auto,
        RequestedWindow::Auto,
    )
    .unwrap_err();

    assert!(matches!(error, UploadError::Transport { .. }));
    assert_eq!(peer.opcodes, [Opcode::Hello, Opcode::Hello]);
}
