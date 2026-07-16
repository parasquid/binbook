use binbook_diagnostic_protocol::{
    decode_transfer_trace_snapshot, encode_transfer_trace_request, Opcode, TransferDirection,
    TransferTraceAction, TransferTraceRequest, TransferTraceSnapshot, TransferTraceState,
};

use crate::{
    upload::{UploadError, UploadTransport},
    upload_protocol::{checked_payload, encode_request},
};

const TRACE_START_SEQUENCE: u16 = u16::MAX - 2;
const TRACE_GET_SEQUENCE: u16 = u16::MAX - 3;
const TRACE_CLEAR_SEQUENCE: u16 = u16::MAX - 4;

pub fn start_trace<T: UploadTransport>(
    transport: &mut T,
    direction: TransferDirection,
) -> Result<TransferTraceSnapshot, UploadError> {
    exchange_snapshot(
        transport,
        TRACE_START_SEQUENCE,
        TransferTraceRequest {
            action: TransferTraceAction::Start,
            direction,
            trace_id: 0,
        },
        direction,
        None,
        Some(TransferTraceState::Active),
    )
}

pub fn get_trace<T: UploadTransport>(
    transport: &mut T,
    direction: TransferDirection,
    trace_id: u32,
) -> Result<TransferTraceSnapshot, UploadError> {
    exchange_snapshot(
        transport,
        TRACE_GET_SEQUENCE,
        TransferTraceRequest {
            action: TransferTraceAction::Get,
            direction,
            trace_id,
        },
        direction,
        Some(trace_id),
        None,
    )
}

pub fn clear_trace<T: UploadTransport>(
    transport: &mut T,
    direction: TransferDirection,
    trace_id: u32,
) -> Result<(), UploadError> {
    let request = TransferTraceRequest {
        action: TransferTraceAction::Clear,
        direction,
        trace_id,
    };
    let frame = encode_request(
        TRACE_CLEAR_SEQUENCE,
        Opcode::StoreTransferTrace,
        |payload| encode_transfer_trace_request(&request, payload),
    )?;
    let response = transport.exchange(&frame, Opcode::StoreTransferTrace, TRACE_CLEAR_SEQUENCE)?;
    let payload = checked_payload(&response, Opcode::StoreTransferTrace, TRACE_CLEAR_SEQUENCE)?;
    if payload.is_empty() {
        Ok(())
    } else {
        Err(UploadError::Response(
            "transfer trace clear returned a nonempty payload".into(),
        ))
    }
}

fn exchange_snapshot<T: UploadTransport>(
    transport: &mut T,
    sequence: u16,
    request: TransferTraceRequest,
    expected_direction: TransferDirection,
    expected_id: Option<u32>,
    expected_state: Option<TransferTraceState>,
) -> Result<TransferTraceSnapshot, UploadError> {
    let frame = encode_request(sequence, Opcode::StoreTransferTrace, |payload| {
        encode_transfer_trace_request(&request, payload)
    })?;
    let response = transport.exchange(&frame, Opcode::StoreTransferTrace, sequence)?;
    let payload = checked_payload(&response, Opcode::StoreTransferTrace, sequence)?;
    let snapshot = decode_transfer_trace_snapshot(&payload)?;
    if snapshot.direction != expected_direction
        || expected_id.is_some_and(|value| snapshot.trace_id != value)
        || expected_state.is_some_and(|value| snapshot.state != value)
    {
        return Err(UploadError::Response(
            "transfer trace response did not match the request".into(),
        ));
    }
    Ok(snapshot)
}
