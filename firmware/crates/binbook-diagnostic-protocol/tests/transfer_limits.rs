use binbook_diagnostic_protocol::{
    decode_transfer_limits, encode_transfer_limits, Opcode, ProtocolError, TransferLimits,
    CAP_STORAGE_BULK_V1, CAP_STORAGE_TRANSFER_LIMITS, CAP_STORAGE_TRANSFER_METRICS,
    TRANSFER_LIMITS_BYTES,
};

#[test]
fn phase2_capabilities_and_opcodes_are_stable() {
    assert_eq!(CAP_STORAGE_TRANSFER_LIMITS, 1 << 10);
    assert_eq!(CAP_STORAGE_TRANSFER_METRICS, 1 << 11);
    assert_eq!(CAP_STORAGE_BULK_V1, 1 << 12);

    assert_eq!(Opcode::StoreTransferLimits as u8, 0x12);
    assert_eq!(Opcode::StoreTransferTrace as u8, 0x13);
    assert_eq!(Opcode::StoreBulkOpen as u8, 0x14);
    assert_eq!(Opcode::StoreBulkData as u8, 0x15);
    assert_eq!(Opcode::StoreBulkCredit as u8, 0x16);
    assert_eq!(Opcode::StoreBulkStatus as u8, 0x17);
    assert_eq!(Opcode::StoreBulkFinish as u8, 0x18);
    assert_eq!(Opcode::StoreBulkAbort as u8, 0x19);

    for value in 0x12..=0x19 {
        assert_eq!(
            Opcode::from_u8(value).map(|opcode| opcode as u8),
            Some(value)
        );
    }
}

#[test]
fn transfer_limits_have_an_exact_little_endian_layout() {
    let limits = TransferLimits {
        flags: 0b11,
        max_data_bytes: 3584,
        legacy_upload_window: 2,
        legacy_read_window: 2,
        bulk_slots: 4,
        rx_capacity_bytes: 8252,
        max_frame_bytes: 4126,
        idle_timeout_ms: 60_000,
    };
    let expected = [
        1, 3, 0x00, 0x0e, 2, 2, 4, 0, 0x3c, 0x20, 0x1e, 0x10, 0x60, 0xea, 0, 0,
    ];
    let mut out = [0xa5; TRANSFER_LIMITS_BYTES];

    assert_eq!(
        encode_transfer_limits(&limits, &mut out),
        Ok(TRANSFER_LIMITS_BYTES)
    );
    assert_eq!(out, expected);
    assert_eq!(decode_transfer_limits(&out), Ok(limits));
}

#[test]
fn transfer_limits_reject_short_output_and_malformed_payloads() {
    let limits = TransferLimits {
        flags: 0b11,
        max_data_bytes: 3584,
        legacy_upload_window: 2,
        legacy_read_window: 2,
        bulk_slots: 4,
        rx_capacity_bytes: 8252,
        max_frame_bytes: 4126,
        idle_timeout_ms: 60_000,
    };
    assert_eq!(
        encode_transfer_limits(&limits, &mut [0; TRANSFER_LIMITS_BYTES - 1]),
        Err(ProtocolError::OutputTooSmall)
    );
    assert_eq!(
        decode_transfer_limits(&[0; TRANSFER_LIMITS_BYTES - 1]),
        Err(ProtocolError::BadPayloadLength)
    );

    let mut valid = [0; TRANSFER_LIMITS_BYTES];
    encode_transfer_limits(&limits, &mut valid).unwrap();
    for (index, replacement) in [
        (0, 2),     // schema
        (1, 4),     // unknown flags
        (2, 0),     // zero chunk, with high byte also zero
        (4, 0),     // zero upload window
        (5, 0),     // zero read window
        (6, 0),     // zero slots while bulk is advertised
        (7, 1),     // reserved
        (10, 0xff), // frame budget below chunk after pair mutation below
    ] {
        let mut malformed = valid;
        malformed[index] = replacement;
        if index == 2 {
            malformed[3] = 0;
        }
        if index == 10 {
            malformed[11] = 0;
        }
        assert!(decode_transfer_limits(&malformed).is_err(), "index {index}");
    }
}

#[test]
fn transfer_limits_require_capability_consistency() {
    let base = TransferLimits {
        flags: 0,
        max_data_bytes: 3584,
        legacy_upload_window: 1,
        legacy_read_window: 1,
        bulk_slots: 0,
        rx_capacity_bytes: 4126,
        max_frame_bytes: 4126,
        idle_timeout_ms: 60_000,
    };
    let mut bytes = [0; TRANSFER_LIMITS_BYTES];
    encode_transfer_limits(&base, &mut bytes).unwrap();
    assert_eq!(decode_transfer_limits(&bytes), Ok(base));

    let inconsistent = TransferLimits {
        flags: 1,
        bulk_slots: 0,
        ..base
    };
    assert_eq!(
        encode_transfer_limits(&inconsistent, &mut bytes),
        Err(ProtocolError::InvalidValue)
    );
}
