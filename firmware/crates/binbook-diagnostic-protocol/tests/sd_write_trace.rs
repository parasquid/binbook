use binbook_diagnostic_protocol::{
    decode_sd_write_trace_request, decode_sd_write_trace_response, encode_sd_write_trace_request,
    encode_sd_write_trace_response, ProtocolError, SdSectorPurpose, SdWriteTraceAction,
    SdWriteTraceRecord, SdWriteTraceRequest,
};

#[test]
fn request_validation_is_exact() {
    let request = SdWriteTraceRequest {
        action: SdWriteTraceAction::Get,
        start_index: 127,
        max_records: 127,
    };
    let mut bytes = [0; 8];
    assert_eq!(encode_sd_write_trace_request(&request, &mut bytes), Ok(8));
    assert_eq!(decode_sd_write_trace_request(&bytes), Ok(request));
    bytes[7] = 1;
    assert_eq!(
        decode_sd_write_trace_request(&bytes),
        Err(ProtocolError::InvalidValue)
    );
}

#[test]
fn response_roundtrips_records_and_rejects_reserved_bytes() {
    let records = [SdWriteTraceRecord {
        start_block: 0x1234_5678,
        blocks: 8,
        command: 25,
        flags: 1,
        command_setup_us: 2,
        data_spi_us: 3,
        busy_wait_us: 4,
        total_us: 5,
        gap_us: 6,
        purpose: SdSectorPurpose::Payload,
    }];
    let mut bytes = [0; 48];
    assert_eq!(
        encode_sd_write_trace_response(9, 512, 8, true, 7, &records, &mut bytes),
        Ok(48)
    );
    let mut decoded = std::vec::Vec::new();
    assert_eq!(
        decode_sd_write_trace_response(&bytes, |record| {
            decoded.push(record);
            Ok(())
        }),
        Ok((9, 512, 8, 1, true, 7))
    );
    assert_eq!(decoded, records);
    bytes[47] = 1;
    assert_eq!(
        decode_sd_write_trace_response(&bytes, |_| Ok(())),
        Err(ProtocolError::InvalidValue)
    );
}

#[test]
fn empty_response_is_valid() {
    let mut bytes = [0; 48];
    assert_eq!(
        encode_sd_write_trace_response(0, 256, 0, false, 0, &[], &mut bytes),
        Ok(16)
    );
}
