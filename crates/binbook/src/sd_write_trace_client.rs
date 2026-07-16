use std::io::{self, Write};
use std::path::Path;

use binbook_diagnostic_protocol::{
    decode_sd_write_trace_response, encode_sd_write_trace_request, Opcode, SdWriteTraceAction,
    SdWriteTraceRecord, SdWriteTraceRequest, SD_WRITE_TRACE_MAX_PAGE_RECORDS,
};
use serde_json::json;

use crate::{
    atomic_output::write_atomic_io,
    upload::{UploadError, UploadTransport},
    upload_protocol::{checked_payload, encode_request},
};

const CLEAR_SEQUENCE: u16 = u16::MAX - 10;
const GET_SEQUENCE_START: u16 = u16::MAX - 11;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdWriteTraceCapture {
    pub capacity: u16,
    pub overflowed: bool,
    pub anomalies: u32,
    pub records: Vec<SdWriteTraceRecord>,
}

pub fn clear<T: UploadTransport>(transport: &mut T) -> Result<(), UploadError> {
    control(transport, SdWriteTraceAction::Clear, CLEAR_SEQUENCE)
}

pub fn freeze<T: UploadTransport>(transport: &mut T) -> Result<(), UploadError> {
    control(
        transport,
        SdWriteTraceAction::Freeze,
        CLEAR_SEQUENCE.wrapping_sub(1),
    )
}

fn control<T: UploadTransport>(
    transport: &mut T,
    action: SdWriteTraceAction,
    sequence: u16,
) -> Result<(), UploadError> {
    let request = SdWriteTraceRequest {
        action,
        start_index: 0,
        max_records: 0,
    };
    let frame = encode_request(sequence, Opcode::StoreSdWriteTrace, |payload| {
        encode_sd_write_trace_request(&request, payload)
    })?;
    let response = transport.exchange(&frame, Opcode::StoreSdWriteTrace, sequence)?;
    let payload = checked_payload(&response, Opcode::StoreSdWriteTrace, sequence)?;
    if payload.is_empty() {
        Ok(())
    } else {
        Err(UploadError::Response(
            "SD write trace clear returned a nonempty payload".into(),
        ))
    }
}

pub fn get_all<T: UploadTransport>(transport: &mut T) -> Result<SdWriteTraceCapture, UploadError> {
    let mut records = Vec::new();
    let mut expected_total = None;
    let mut expected_capacity = None;
    let mut expected_overflow = None;
    let mut expected_anomalies = None;
    let mut page = 0u16;
    loop {
        let start_index = u16::try_from(records.len())
            .map_err(|_| UploadError::Response("SD write trace exceeds u16 records".into()))?;
        let sequence = GET_SEQUENCE_START.wrapping_sub(page);
        let request = SdWriteTraceRequest {
            action: SdWriteTraceAction::Get,
            start_index,
            max_records: SD_WRITE_TRACE_MAX_PAGE_RECORDS,
        };
        let frame = encode_request(sequence, Opcode::StoreSdWriteTrace, |payload| {
            encode_sd_write_trace_request(&request, payload)
        })?;
        let response = transport.exchange(&frame, Opcode::StoreSdWriteTrace, sequence)?;
        let payload = checked_payload(&response, Opcode::StoreSdWriteTrace, sequence)?;
        let mut page_records = Vec::new();
        let decoded = decode_sd_write_trace_response(&payload, |record| {
            page_records.push(record);
            Ok(())
        });
        let (total, capacity, returned_start, count, overflowed, anomalies) =
            decoded.map_err(|error| {
                UploadError::Response(format!(
                    "invalid SD write trace page ({error:?}, {} bytes): {:02x?}",
                    payload.len(),
                    &payload[..payload.len().min(64)]
                ))
            })?;
        if returned_start != start_index
            || usize::from(count) != page_records.len()
            || expected_total.is_some_and(|value| value != total)
            || expected_capacity.is_some_and(|value| value != capacity)
            || expected_overflow.is_some_and(|value| value != overflowed)
            || expected_anomalies.is_some_and(|value| value != anomalies)
        {
            return Err(UploadError::Response(
                "SD write trace changed or returned the wrong page".into(),
            ));
        }
        expected_total = Some(total);
        expected_capacity = Some(capacity);
        expected_overflow = Some(overflowed);
        expected_anomalies = Some(anomalies);
        records.extend(page_records);
        if records.len() == usize::from(total) {
            return Ok(SdWriteTraceCapture {
                capacity,
                overflowed,
                anomalies,
                records,
            });
        }
        if count == 0 {
            return Err(UploadError::Response(
                "SD write trace ended before total records".into(),
            ));
        }
        page = page.wrapping_add(1);
    }
}

impl SdWriteTraceCapture {
    pub fn write_atomic(&self, path: &Path) -> io::Result<()> {
        write_atomic_io(path, |file| {
            let records: Vec<_> = self
                .records
                .iter()
                .map(|value| {
                    json!({
                        "start_block": value.start_block,
                        "blocks": value.blocks,
                        "command": value.command,
                        "flags": value.flags,
                        "command_setup_us": value.command_setup_us,
                        "data_spi_us": value.data_spi_us,
                        "busy_wait_us": value.busy_wait_us,
                        "total_us": value.total_us,
                        "gap_us": value.gap_us,
                        "purpose": format!("{:?}", value.purpose),
                    })
                })
                .collect();
            serde_json::to_writer_pretty(
                &mut *file,
                &json!({
                    "schema_version": 2,
                    "capacity": self.capacity,
                    "overflowed": self.overflowed,
                    "anomalies": self.anomalies,
                    "records": records,
                }),
            )?;
            file.write_all(b"\n")
        })
    }
}
