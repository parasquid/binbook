use crate::{ProtocolError, Status, STORE_BULK_MAX_DATA_BYTES};

pub const STORE_BULK_DATA_HEADER_BYTES: usize = 12;
pub const STORE_BULK_CREDIT_BYTES: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreBulkData<'a> {
    pub stream_id: u32,
    pub offset: u32,
    pub end_of_data: bool,
    pub data: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreBulkCredit {
    pub stream_id: u32,
    pub durable_offset: u32,
    pub credits: u8,
    pub status: Status,
}

pub fn encode_store_bulk_data(
    value: &StoreBulkData<'_>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    if value.stream_id == 0
        || value.data.is_empty()
        || value.data.len() > usize::from(STORE_BULK_MAX_DATA_BYTES)
    {
        return Err(ProtocolError::InvalidValue);
    }
    let total = STORE_BULK_DATA_HEADER_BYTES + value.data.len();
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&value.stream_id.to_le_bytes());
    out[4..8].copy_from_slice(&value.offset.to_le_bytes());
    out[8..10].copy_from_slice(&(value.data.len() as u16).to_le_bytes());
    out[10] = u8::from(value.end_of_data);
    out[11] = 0;
    out[12..total].copy_from_slice(value.data);
    Ok(total)
}

pub fn decode_store_bulk_data(payload: &[u8]) -> Result<StoreBulkData<'_>, ProtocolError> {
    if payload.len() < STORE_BULK_DATA_HEADER_BYTES {
        return Err(ProtocolError::BadPayloadLength);
    }
    if payload[10] > 1 || payload[11] != 0 {
        return Err(ProtocolError::InvalidValue);
    }
    let data_len = u16::from_le_bytes([payload[8], payload[9]]) as usize;
    if payload.len() != STORE_BULK_DATA_HEADER_BYTES + data_len {
        return Err(ProtocolError::BadPayloadLength);
    }
    let value = StoreBulkData {
        stream_id: u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]),
        offset: u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]),
        end_of_data: payload[10] != 0,
        data: &payload[12..],
    };
    if value.stream_id == 0
        || value.data.is_empty()
        || value.data.len() > usize::from(STORE_BULK_MAX_DATA_BYTES)
    {
        return Err(ProtocolError::InvalidValue);
    }
    Ok(value)
}

pub fn encode_store_bulk_credit(
    value: &StoreBulkCredit,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    if value.stream_id == 0 {
        return Err(ProtocolError::InvalidValue);
    }
    if out.len() < STORE_BULK_CREDIT_BYTES {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&value.stream_id.to_le_bytes());
    out[4..8].copy_from_slice(&value.durable_offset.to_le_bytes());
    out[8] = value.credits;
    out[9] = value.status as u8;
    out[10..12].fill(0);
    Ok(STORE_BULK_CREDIT_BYTES)
}

pub fn decode_store_bulk_credit(payload: &[u8]) -> Result<StoreBulkCredit, ProtocolError> {
    if payload.len() != STORE_BULK_CREDIT_BYTES {
        return Err(ProtocolError::BadPayloadLength);
    }
    if payload[10] != 0 || payload[11] != 0 {
        return Err(ProtocolError::InvalidValue);
    }
    let value = StoreBulkCredit {
        stream_id: u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]),
        durable_offset: u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]),
        credits: payload[8],
        status: Status::from_u8(payload[9]).ok_or(ProtocolError::InvalidValue)?,
    };
    if value.stream_id == 0 {
        return Err(ProtocolError::InvalidValue);
    }
    Ok(value)
}
