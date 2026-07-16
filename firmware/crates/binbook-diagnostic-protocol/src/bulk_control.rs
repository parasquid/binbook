use crate::{ProtocolError, StoreUploadCommitResult, TransferDirection};

pub const STORE_BULK_ID_BYTES: usize = 4;
pub const STORE_BULK_STATUS_RESPONSE_BYTES: usize = 20;
pub const STORE_BULK_FINISH_RESPONSE_BYTES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StoreBulkState {
    Receiving = 0,
    Finishing = 1,
    Complete = 2,
    Failed = 3,
}

impl StoreBulkState {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Receiving),
            1 => Some(Self::Finishing),
            2 => Some(Self::Complete),
            3 => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreBulkStatusResponse {
    pub stream_id: u32,
    pub direction: TransferDirection,
    pub state: StoreBulkState,
    pub available_credits: u8,
    pub durable_offset: u32,
    pub total_size: u32,
    pub running_crc32: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreBulkFinishResponse {
    pub stream_id: u32,
    pub result: StoreUploadCommitResult,
    pub total_bytes: u32,
    pub computed_crc32: u32,
}

fn encode_id(stream_id: u32, out: &mut [u8]) -> Result<usize, ProtocolError> {
    if stream_id == 0 {
        return Err(ProtocolError::InvalidValue);
    }
    if out.len() < STORE_BULK_ID_BYTES {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[..4].copy_from_slice(&stream_id.to_le_bytes());
    Ok(STORE_BULK_ID_BYTES)
}

fn decode_id(payload: &[u8]) -> Result<u32, ProtocolError> {
    if payload.len() != STORE_BULK_ID_BYTES {
        return Err(ProtocolError::BadPayloadLength);
    }
    let value = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    if value == 0 {
        return Err(ProtocolError::InvalidValue);
    }
    Ok(value)
}

pub fn encode_store_bulk_status_request(id: u32, out: &mut [u8]) -> Result<usize, ProtocolError> {
    encode_id(id, out)
}

pub fn decode_store_bulk_status_request(payload: &[u8]) -> Result<u32, ProtocolError> {
    decode_id(payload)
}

pub fn encode_store_bulk_finish_request(id: u32, out: &mut [u8]) -> Result<usize, ProtocolError> {
    encode_id(id, out)
}

pub fn decode_store_bulk_finish_request(payload: &[u8]) -> Result<u32, ProtocolError> {
    decode_id(payload)
}

pub fn encode_store_bulk_abort_request(id: u32, out: &mut [u8]) -> Result<usize, ProtocolError> {
    encode_id(id, out)
}

pub fn decode_store_bulk_abort_request(payload: &[u8]) -> Result<u32, ProtocolError> {
    decode_id(payload)
}

fn validate_status(value: &StoreBulkStatusResponse) -> Result<(), ProtocolError> {
    if value.stream_id == 0
        || value.direction == TransferDirection::Unspecified
        || value.available_credits == 0
        || value.total_size == 0
        || value.durable_offset > value.total_size
        || (value.durable_offset == 0 && value.running_crc32 != 0)
    {
        return Err(ProtocolError::InvalidValue);
    }
    Ok(())
}

pub fn encode_store_bulk_status_response(
    value: &StoreBulkStatusResponse,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    validate_status(value)?;
    if out.len() < STORE_BULK_STATUS_RESPONSE_BYTES {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&value.stream_id.to_le_bytes());
    out[4] = value.direction as u8;
    out[5] = value.state as u8;
    out[6] = value.available_credits;
    out[7] = 0;
    out[8..12].copy_from_slice(&value.durable_offset.to_le_bytes());
    out[12..16].copy_from_slice(&value.total_size.to_le_bytes());
    out[16..20].copy_from_slice(&value.running_crc32.to_le_bytes());
    Ok(STORE_BULK_STATUS_RESPONSE_BYTES)
}

pub fn decode_store_bulk_status_response(
    payload: &[u8],
) -> Result<StoreBulkStatusResponse, ProtocolError> {
    if payload.len() != STORE_BULK_STATUS_RESPONSE_BYTES {
        return Err(ProtocolError::BadPayloadLength);
    }
    if payload[7] != 0 {
        return Err(ProtocolError::InvalidValue);
    }
    let value = StoreBulkStatusResponse {
        stream_id: u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]),
        direction: TransferDirection::from_u8(payload[4]).ok_or(ProtocolError::InvalidValue)?,
        state: StoreBulkState::from_u8(payload[5]).ok_or(ProtocolError::InvalidValue)?,
        available_credits: payload[6],
        durable_offset: u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]),
        total_size: u32::from_le_bytes([payload[12], payload[13], payload[14], payload[15]]),
        running_crc32: u32::from_le_bytes([payload[16], payload[17], payload[18], payload[19]]),
    };
    validate_status(&value)?;
    Ok(value)
}

pub fn encode_store_bulk_finish_response(
    value: &StoreBulkFinishResponse,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    if value.stream_id == 0 || value.total_bytes == 0 {
        return Err(ProtocolError::InvalidValue);
    }
    if out.len() < STORE_BULK_FINISH_RESPONSE_BYTES {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&value.stream_id.to_le_bytes());
    out[4] = value.result as u8;
    out[5..8].fill(0);
    out[8..12].copy_from_slice(&value.total_bytes.to_le_bytes());
    out[12..16].copy_from_slice(&value.computed_crc32.to_le_bytes());
    Ok(STORE_BULK_FINISH_RESPONSE_BYTES)
}

pub fn decode_store_bulk_finish_response(
    payload: &[u8],
) -> Result<StoreBulkFinishResponse, ProtocolError> {
    if payload.len() != STORE_BULK_FINISH_RESPONSE_BYTES {
        return Err(ProtocolError::BadPayloadLength);
    }
    if payload[5..8] != [0, 0, 0] {
        return Err(ProtocolError::InvalidValue);
    }
    let value = StoreBulkFinishResponse {
        stream_id: u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]),
        result: StoreUploadCommitResult::from_u8(payload[4]).ok_or(ProtocolError::InvalidValue)?,
        total_bytes: u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]),
        computed_crc32: u32::from_le_bytes([payload[12], payload[13], payload[14], payload[15]]),
    };
    if value.stream_id == 0 || value.total_bytes == 0 {
        return Err(ProtocolError::InvalidValue);
    }
    Ok(value)
}
