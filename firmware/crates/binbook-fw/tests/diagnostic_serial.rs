#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::MAX_FRAME_BYTES;
use binbook_fw::diag::SerialState;

#[test]
fn response_is_encoded_directly_into_persistent_tx_storage() {
    use binbook_diagnostic_protocol::{decode_frame, FrameHeader, FrameKind, Opcode, Status};

    let request = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::StoreTransferLimits,
        status: Status::Ok,
        sequence: 65534,
        payload_len: 0,
    };
    let payload = [1, 2, 3, 4];
    let mut state = SerialState::new();
    state
        .replace_tx_response(request, Status::Ok, &payload)
        .unwrap();

    let mut decoded_payload = [0u8; MAX_FRAME_BYTES];
    let (header, payload_len) = decode_frame(state.pending_tx(), &mut decoded_payload).unwrap();
    assert_eq!(header.kind, FrameKind::Response);
    assert_eq!(header.opcode, Opcode::StoreTransferLimits);
    assert_eq!(header.sequence, 65534);
    assert_eq!(&decoded_payload[..payload_len], payload);
}

#[test]
fn maximum_bulk_event_is_encoded_directly_with_one_delimiter() {
    use binbook_diagnostic_protocol::{decode_frame, FrameKind, Opcode, Status, FRAME_DELIMITER};

    let payload = [0xff; 3_596];
    let mut state = SerialState::new();
    state
        .replace_tx_event(Opcode::StoreBulkData, &payload)
        .unwrap();

    assert_eq!(state.pending_tx().last(), Some(&FRAME_DELIMITER));
    assert_eq!(
        state
            .pending_tx()
            .iter()
            .filter(|byte| **byte == FRAME_DELIMITER)
            .count(),
        1
    );
    let mut decoded = [0u8; MAX_FRAME_BYTES];
    let (header, payload_len) = decode_frame(state.pending_tx(), &mut decoded).unwrap();
    assert_eq!(header.kind, FrameKind::Event);
    assert_eq!(header.opcode, Opcode::StoreBulkData);
    assert_eq!(header.status, Status::Ok);
    assert_eq!(header.sequence, 0);
    assert_eq!(&decoded[..payload_len], payload);
}

#[test]
fn diag_serial_keeps_partial_frame_until_delimiter() {
    use binbook_diagnostic_protocol::{
        encode_frame, FrameHeader, FrameKind, Opcode, Status, MAX_FRAME_BYTES,
    };
    let mut state = SerialState::new();
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Status,
        status: Status::Ok,
        sequence: 1,
        payload_len: 0,
    };
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();

    let half = len / 2;
    state.feed_rx(&buf[..half]);
    let mut out = [0u8; MAX_FRAME_BYTES];
    assert!(
        state.next_frame(&mut out).is_none(),
        "should not yield frame before delimiter"
    );

    state.feed_rx(&buf[half..]);
    assert!(
        state.next_frame(&mut out).is_some(),
        "should yield frame after delimiter"
    );
}

#[test]
fn maximum_frame_in_64_byte_fragments_dispatches_only_after_delimiter() {
    let mut serial = SerialState::new();
    let mut frame = vec![1u8; MAX_FRAME_BYTES];
    frame[MAX_FRAME_BYTES - 1] = 0;

    for (index, fragment) in frame.chunks(64).enumerate() {
        let result = serial.feed_rx(fragment);
        assert_eq!(result.consumed, fragment.len());
        if index + 1 == frame.len().div_ceil(64) {
            assert!(result.frame_ready);
            assert!(serial.frame_ready());
        } else {
            assert!(!result.frame_ready);
            assert!(!serial.frame_ready());
        }
    }

    let mut decoded = [0u8; MAX_FRAME_BYTES];
    assert_eq!(serial.next_frame(&mut decoded), Some(MAX_FRAME_BYTES));
    assert_eq!(decoded, frame.as_slice());
    assert!(!serial.frame_ready());
    assert_eq!(serial.protocol_error_count(), 0);
}

#[test]
#[cfg(not(feature = "usb-rx-isr-buffer"))]
fn two_maximum_frames_fill_existing_ring_without_drop_or_reorder() {
    let mut first = vec![1u8; MAX_FRAME_BYTES];
    first[MAX_FRAME_BYTES - 1] = 0;
    let mut second = vec![2u8; MAX_FRAME_BYTES];
    second[MAX_FRAME_BYTES - 1] = 0;
    let mut serial = SerialState::new();
    for fragment in first
        .iter()
        .chain(second.iter())
        .copied()
        .collect::<Vec<_>>()
        .chunks(64)
    {
        serial.feed_rx(fragment);
    }

    let mut decoded = [0u8; MAX_FRAME_BYTES];
    assert_eq!(serial.next_frame(&mut decoded), Some(MAX_FRAME_BYTES));
    assert_eq!(decoded, first.as_slice());
    assert_eq!(serial.next_frame(&mut decoded), Some(MAX_FRAME_BYTES));
    assert_eq!(decoded, second.as_slice());
    assert_eq!(serial.protocol_error_count(), 0);
}

#[test]
fn explicit_rx_overflow_discards_until_delimiter_then_recovers() {
    let mut serial = binbook_fw::diag::SerialState::new();
    serial.feed_rx(&[1, 2, 3]);
    serial.note_rx_overflow();
    assert_eq!(serial.protocol_error_count(), 1);
    serial.feed_rx(&[4, 5, 0, 7, 0]);
    let mut decoded = [0; MAX_FRAME_BYTES];
    assert_eq!(serial.next_frame(&mut decoded), Some(2));
    assert_eq!(&decoded[..2], &[7, 0]);
}

#[test]
fn completion_without_usb_input_requires_immediate_service() {
    let serial = SerialState::new();
    assert!(!serial.needs_service(false));
    assert!(serial.needs_service(true));
}

#[test]
fn diag_serial_yields_two_batched_frames_in_order() {
    use binbook_diagnostic_protocol::{
        decode_frame, encode_frame, FrameHeader, FrameKind, Opcode, Status, MAX_FRAME_BYTES,
    };
    use binbook_fw::diag::SerialState;

    let mut state = SerialState::new();
    let mut combined = [0u8; MAX_FRAME_BYTES * 2];
    let mut offset = 0;

    for seq in [41u16, 42] {
        let header = FrameHeader {
            kind: FrameKind::Request,
            opcode: Opcode::Status,
            status: Status::Ok,
            sequence: seq,
            payload_len: 0,
        };
        let mut buf = [0u8; MAX_FRAME_BYTES];
        let len = encode_frame(&header, &[], &mut buf).unwrap();
        combined[offset..offset + len].copy_from_slice(&buf[..len]);
        offset += len;
    }

    state.feed_rx(&combined[..offset]);

    let mut out = [0u8; MAX_FRAME_BYTES];
    let f1 = state.next_frame(&mut out);
    assert!(f1.is_some());
    let mut payload = [0u8; 496];
    let (h1, _) = decode_frame(&out[..f1.unwrap()], &mut payload).unwrap();
    assert_eq!(h1.sequence, 41);

    let f2 = state.next_frame(&mut out);
    assert!(f2.is_some());
    let (h2, _) = decode_frame(&out[..f2.unwrap()], &mut payload).unwrap();
    assert_eq!(h2.sequence, 42);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_serial_counts_oversized_frame_and_continues() {
    use binbook_diagnostic_protocol::{
        encode_frame, FrameHeader, FrameKind, Opcode, Status, MAX_FRAME_BYTES,
    };
    use binbook_fw::diag::SerialState;

    let mut state = SerialState::new();

    // Feed > MAX_FRAME_BYTES of data before a delimiter — transport should detect this
    let mut oversized = [0xAA; MAX_FRAME_BYTES + 64];
    oversized[MAX_FRAME_BYTES + 63] = 0x00; // delimiter at end
    state.feed_rx(&oversized);

    // next_frame detects the oversized frame and increments the error counter
    let mut out = [0u8; MAX_FRAME_BYTES];
    assert!(state.next_frame(&mut out).is_none());
    assert_eq!(state.protocol_error_count(), 1);

    // Recovery: a valid frame after delimiter should still work
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Status,
        status: Status::Ok,
        sequence: 99,
        payload_len: 0,
    };
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();
    state.feed_rx(&buf[..len]);

    assert!(state.next_frame(&mut out).is_some());
    assert_eq!(state.protocol_error_count(), 1); // still 1, not 2
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_serial_counts_content_invalid_frame_and_continues() {
    use binbook_diagnostic_protocol::{
        decode_frame, encode_frame, FrameHeader, FrameKind, Opcode, Status, MAX_FRAME_BYTES,
    };
    use binbook_fw::diag::SerialState;

    let mut state = SerialState::new();

    // Feed a valid-COBS frame that is transport-legal but contains garbage content
    let garbage = [0x01, 0x02, 0x03, 0x04, 0x05, 0x00];
    state.feed_rx(&garbage);

    // Transport layer accepts it (no error at transport level)
    let mut out = [0u8; MAX_FRAME_BYTES];
    let f = state.next_frame(&mut out);
    assert!(f.is_some());
    // Transport did not count an error — content validation is decode_frame's job
    assert_eq!(state.protocol_error_count(), 0);

    // Recovery: a valid frame after should still work
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Status,
        status: Status::Ok,
        sequence: 50,
        payload_len: 0,
    };
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();
    state.feed_rx(&buf[..len]);

    let f2 = state.next_frame(&mut out);
    assert!(f2.is_some());
    let mut payload = [0u8; 496];
    let (h, _) = decode_frame(&out[..f2.unwrap()], &mut payload).unwrap();
    assert_eq!(h.sequence, 50);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_serial_partial_tx_preserves_unsent_suffix() {
    use binbook_diagnostic_protocol::{
        encode_frame, FrameHeader, FrameKind, Opcode, Status, MAX_FRAME_BYTES,
    };
    use binbook_fw::diag::SerialState;

    let mut state = SerialState::new();

    let header = FrameHeader {
        kind: FrameKind::Response,
        opcode: Opcode::Status,
        status: Status::Ok,
        sequence: 1,
        payload_len: 10,
    };
    let payload = [0xAA; 10];
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &payload, &mut buf).unwrap();
    state.queue_tx(&buf[..len]).unwrap();

    assert_eq!(state.pending_tx().len(), len);

    state.consume_tx(3);
    assert_eq!(state.pending_tx().len(), len - 3);
    assert_eq!(state.pending_tx()[0], buf[3]);
}
