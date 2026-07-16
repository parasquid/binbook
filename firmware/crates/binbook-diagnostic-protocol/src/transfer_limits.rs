use crate::{ProtocolError, MAX_PAYLOAD_BYTES};

pub const TRANSFER_LIMITS_SCHEMA_VERSION: u8 = 1;
pub const TRANSFER_LIMITS_BYTES: usize = 16;
pub const TRANSFER_LIMIT_UPLOAD_BULK: u8 = 1 << 0;
pub const TRANSFER_LIMIT_READ_BULK: u8 = 1 << 1;
pub const TRANSFER_LIMIT_FLAGS: u8 = TRANSFER_LIMIT_UPLOAD_BULK | TRANSFER_LIMIT_READ_BULK;
const BULK_DATA_HEADER_BYTES: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferLimits {
    pub flags: u8,
    pub max_data_bytes: u16,
    pub legacy_upload_window: u8,
    pub legacy_read_window: u8,
    pub bulk_slots: u8,
    pub rx_capacity_bytes: u16,
    pub max_frame_bytes: u16,
    pub idle_timeout_ms: u32,
}

fn validate(value: &TransferLimits) -> Result<(), ProtocolError> {
    if value.flags & !TRANSFER_LIMIT_FLAGS != 0
        || value.max_data_bytes == 0
        || usize::from(value.max_data_bytes) > MAX_PAYLOAD_BYTES - BULK_DATA_HEADER_BYTES
        || value.legacy_upload_window == 0
        || value.legacy_read_window == 0
        || value.rx_capacity_bytes == 0
        || value.max_frame_bytes == 0
        || value.max_data_bytes > value.max_frame_bytes
        || value.idle_timeout_ms == 0
        || (value.flags != 0 && value.bulk_slots == 0)
        || (value.flags == 0 && value.bulk_slots != 0)
    {
        return Err(ProtocolError::InvalidValue);
    }
    Ok(())
}

pub fn encode_transfer_limits(
    value: &TransferLimits,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    validate(value)?;
    if out.len() < TRANSFER_LIMITS_BYTES {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = TRANSFER_LIMITS_SCHEMA_VERSION;
    out[1] = value.flags;
    out[2..4].copy_from_slice(&value.max_data_bytes.to_le_bytes());
    out[4] = value.legacy_upload_window;
    out[5] = value.legacy_read_window;
    out[6] = value.bulk_slots;
    out[7] = 0;
    out[8..10].copy_from_slice(&value.rx_capacity_bytes.to_le_bytes());
    out[10..12].copy_from_slice(&value.max_frame_bytes.to_le_bytes());
    out[12..16].copy_from_slice(&value.idle_timeout_ms.to_le_bytes());
    Ok(TRANSFER_LIMITS_BYTES)
}

pub fn decode_transfer_limits(payload: &[u8]) -> Result<TransferLimits, ProtocolError> {
    if payload.len() != TRANSFER_LIMITS_BYTES {
        return Err(ProtocolError::BadPayloadLength);
    }
    if payload[0] != TRANSFER_LIMITS_SCHEMA_VERSION || payload[7] != 0 {
        return Err(ProtocolError::InvalidValue);
    }
    let value = TransferLimits {
        flags: payload[1],
        max_data_bytes: u16::from_le_bytes([payload[2], payload[3]]),
        legacy_upload_window: payload[4],
        legacy_read_window: payload[5],
        bulk_slots: payload[6],
        rx_capacity_bytes: u16::from_le_bytes([payload[8], payload[9]]),
        max_frame_bytes: u16::from_le_bytes([payload[10], payload[11]]),
        idle_timeout_ms: u32::from_le_bytes([payload[12], payload[13], payload[14], payload[15]]),
    };
    validate(&value)?;
    Ok(value)
}
