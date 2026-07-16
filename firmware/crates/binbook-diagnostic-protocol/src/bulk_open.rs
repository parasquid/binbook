use crate::{storage::validate_name, ProtocolError, StorageBackend, TransferDirection};

pub const STORE_BULK_OPEN_HEADER_BYTES: usize = 16;
pub const STORE_BULK_OPEN_RESPONSE_BYTES: usize = 16;
pub const STORE_BULK_MAX_DATA_BYTES: u16 = 3584;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreBulkOpenRequest<'a> {
    pub direction: TransferDirection,
    pub backend: StorageBackend,
    pub requested_window: u8,
    pub requested_chunk: u16,
    pub name: &'a str,
    pub total_size: u32,
    pub expected_crc32: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreBulkOpenResponse {
    pub stream_id: u32,
    pub selected_chunk: u16,
    pub initial_credits: u8,
    pub direction: TransferDirection,
    pub durable_offset: u32,
    pub total_size: u32,
}

fn valid_requested_window(value: u8) -> bool {
    matches!(value, 0 | 1 | 2 | 4 | 8)
}

fn validate_request(value: &StoreBulkOpenRequest<'_>) -> Result<(), ProtocolError> {
    validate_name(value.name)?;
    let metadata_valid = match value.direction {
        TransferDirection::Upload => value.total_size != 0,
        TransferDirection::Readback => value.total_size == 0 && value.expected_crc32 == 0,
        TransferDirection::Unspecified => false,
    };
    if !valid_requested_window(value.requested_window)
        || (value.requested_chunk != 0 && value.requested_chunk < 512)
        || !metadata_valid
    {
        return Err(ProtocolError::InvalidValue);
    }
    Ok(())
}

pub fn encode_store_bulk_open_request(
    value: &StoreBulkOpenRequest<'_>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    validate_request(value)?;
    let name = value.name.as_bytes();
    let total = STORE_BULK_OPEN_HEADER_BYTES + name.len();
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = value.direction as u8;
    out[1] = value.backend as u8;
    out[2] = value.requested_window;
    out[3] = 0;
    out[4..6].copy_from_slice(&value.requested_chunk.to_le_bytes());
    out[6] = name.len() as u8;
    out[7] = 0;
    out[8..12].copy_from_slice(&value.total_size.to_le_bytes());
    out[12..16].copy_from_slice(&value.expected_crc32.to_le_bytes());
    out[16..total].copy_from_slice(name);
    Ok(total)
}

pub fn decode_store_bulk_open_request(
    payload: &[u8],
) -> Result<StoreBulkOpenRequest<'_>, ProtocolError> {
    if payload.len() < STORE_BULK_OPEN_HEADER_BYTES {
        return Err(ProtocolError::BadPayloadLength);
    }
    if payload[3] != 0 || payload[7] != 0 {
        return Err(ProtocolError::InvalidValue);
    }
    let name_len = payload[6] as usize;
    if payload.len() != STORE_BULK_OPEN_HEADER_BYTES + name_len {
        return Err(ProtocolError::BadPayloadLength);
    }
    let name = core::str::from_utf8(&payload[16..]).map_err(|_| ProtocolError::InvalidValue)?;
    let value = StoreBulkOpenRequest {
        direction: TransferDirection::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)?,
        backend: StorageBackend::from_u8(payload[1]).ok_or(ProtocolError::InvalidValue)?,
        requested_window: payload[2],
        requested_chunk: u16::from_le_bytes([payload[4], payload[5]]),
        name,
        total_size: u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]),
        expected_crc32: u32::from_le_bytes([payload[12], payload[13], payload[14], payload[15]]),
    };
    validate_request(&value)?;
    Ok(value)
}

fn validate_response(value: &StoreBulkOpenResponse) -> Result<(), ProtocolError> {
    if value.stream_id == 0
        || value.direction == TransferDirection::Unspecified
        || value.selected_chunk < 512
        || value.selected_chunk > STORE_BULK_MAX_DATA_BYTES
        || !value.selected_chunk.is_multiple_of(512)
        || value.initial_credits == 0
        || value.total_size == 0
        || value.durable_offset > value.total_size
    {
        return Err(ProtocolError::InvalidValue);
    }
    Ok(())
}

pub fn encode_store_bulk_open_response(
    value: &StoreBulkOpenResponse,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    validate_response(value)?;
    if out.len() < STORE_BULK_OPEN_RESPONSE_BYTES {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&value.stream_id.to_le_bytes());
    out[4..6].copy_from_slice(&value.selected_chunk.to_le_bytes());
    out[6] = value.initial_credits;
    out[7] = value.direction as u8;
    out[8..12].copy_from_slice(&value.durable_offset.to_le_bytes());
    out[12..16].copy_from_slice(&value.total_size.to_le_bytes());
    Ok(STORE_BULK_OPEN_RESPONSE_BYTES)
}

pub fn decode_store_bulk_open_response(
    payload: &[u8],
) -> Result<StoreBulkOpenResponse, ProtocolError> {
    if payload.len() != STORE_BULK_OPEN_RESPONSE_BYTES {
        return Err(ProtocolError::BadPayloadLength);
    }
    let value = StoreBulkOpenResponse {
        stream_id: u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]),
        selected_chunk: u16::from_le_bytes([payload[4], payload[5]]),
        initial_credits: payload[6],
        direction: TransferDirection::from_u8(payload[7]).ok_or(ProtocolError::InvalidValue)?,
        durable_offset: u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]),
        total_size: u32::from_le_bytes([payload[12], payload[13], payload[14], payload[15]]),
    };
    validate_response(&value)?;
    Ok(value)
}
