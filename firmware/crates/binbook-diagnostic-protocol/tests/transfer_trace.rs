use binbook_diagnostic_protocol::{
    decode_transfer_trace_request, decode_transfer_trace_snapshot, encode_transfer_trace_request,
    encode_transfer_trace_snapshot, ProtocolError, TransferDirection, TransferTraceAction,
    TransferTraceRequest, TransferTraceSnapshot, TransferTraceState, TRANSFER_TRACE_REQUEST_BYTES,
    TRANSFER_TRACE_SNAPSHOT_BYTES,
};

fn snapshot() -> TransferTraceSnapshot {
    TransferTraceSnapshot {
        direction: TransferDirection::Readback,
        state: TransferTraceState::Succeeded,
        selected_window: 4,
        trace_id: 0x4433_2211,
        chunk_size: 3584,
        peak_rx_bytes: 8192,
        logical_bytes: 1,
        frames: 2,
        usb_rx_bytes: 3,
        usb_tx_bytes: 4,
        usb_rx_us: 5,
        usb_tx_us: 6,
        framing_us: 7,
        frame_crc_us: 8,
        owner_queue_us: 9,
        owner_execute_us: 10,
        sd_read_calls: 11,
        sd_write_calls: 12,
        sd_read_blocks: 13,
        sd_write_blocks: 14,
        sd_read_logical_bytes: 15,
        sd_write_logical_bytes: 16,
        sd_read_us: 17,
        sd_write_us: 18,
        fat_allocation_us: 19,
        upload_prepare_us: 20,
        data_io_us: 21,
        crc_us: 22,
        commit_us: 23,
        library_refresh_us: 24,
        data_phase_us: 25,
        end_to_end_us: 26,
        usb_sd_overlap_us: 27,
        peak_slots: 3,
        flags: 0b11,
        queue_stalls: 0x6655,
        protocol_errors_start: 28,
        protocol_errors_end: 29,
    }
}

#[test]
fn trace_request_has_exact_layout_and_round_trips() {
    let request = TransferTraceRequest {
        action: TransferTraceAction::Get,
        direction: TransferDirection::Upload,
        trace_id: 0x4433_2211,
    };
    let expected = [1, 0, 0, 0, 0x11, 0x22, 0x33, 0x44];
    let mut out = [0xa5; TRANSFER_TRACE_REQUEST_BYTES];
    assert_eq!(
        encode_transfer_trace_request(&request, &mut out),
        Ok(TRANSFER_TRACE_REQUEST_BYTES)
    );
    assert_eq!(out, expected);
    assert_eq!(decode_transfer_trace_request(&out), Ok(request));
}

#[test]
fn trace_request_rejects_invalid_action_direction_id_and_reserved_bytes() {
    for payload in [
        [3, 0, 0, 0, 1, 0, 0, 0],
        [0, 2, 0, 0, 0, 0, 0, 0],
        [0, 0, 1, 0, 0, 0, 0, 0],
        [0, 0, 0, 1, 0, 0, 0, 0],
        [0, 0, 0, 0, 1, 0, 0, 0],
        [1, 0, 0, 0, 0, 0, 0, 0],
        [2, 255, 0, 0, 0, 0, 0, 0],
    ] {
        assert_eq!(
            decode_transfer_trace_request(&payload),
            Err(ProtocolError::InvalidValue)
        );
    }
    assert_eq!(
        decode_transfer_trace_request(&[0; TRANSFER_TRACE_REQUEST_BYTES - 1]),
        Err(ProtocolError::BadPayloadLength)
    );
}

#[test]
fn trace_snapshot_has_exact_offsets_and_round_trips() {
    let value = snapshot();
    let mut out = [0xa5; TRANSFER_TRACE_SNAPSHOT_BYTES];
    assert_eq!(
        encode_transfer_trace_snapshot(&value, &mut out),
        Ok(TRANSFER_TRACE_SNAPSHOT_BYTES)
    );
    assert_eq!(
        &out[0..12],
        &[1, 1, 1, 4, 0x11, 0x22, 0x33, 0x44, 0, 14, 0, 32]
    );
    for (offset, expected) in (12..=116).step_by(4).zip(1u32..=27) {
        assert_eq!(
            &out[offset..offset + 4],
            &expected.to_le_bytes(),
            "offset {offset}"
        );
    }
    assert_eq!(&out[120..124], &[3, 3, 0x55, 0x66]);
    assert_eq!(&out[124..128], &28u32.to_le_bytes());
    assert_eq!(&out[128..132], &29u32.to_le_bytes());
    assert_eq!(decode_transfer_trace_snapshot(&out), Ok(value));
}

#[test]
fn trace_snapshot_rejects_short_unknown_or_inconsistent_fields() {
    let mut bytes = [0; TRANSFER_TRACE_SNAPSHOT_BYTES];
    encode_transfer_trace_snapshot(&snapshot(), &mut bytes).unwrap();
    assert_eq!(
        decode_transfer_trace_snapshot(&bytes[..131]),
        Err(ProtocolError::BadPayloadLength)
    );
    for (offset, invalid) in [(0, 2), (1, 2), (2, 4), (3, 0), (121, 4)] {
        let mut malformed = bytes;
        malformed[offset] = invalid;
        assert_eq!(
            decode_transfer_trace_snapshot(&malformed),
            Err(ProtocolError::InvalidValue),
            "offset {offset}"
        );
    }
}
