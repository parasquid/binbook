use crate::ProtocolError;

pub const TRANSFER_TRACE_REQUEST_BYTES: usize = 8;
pub const TRANSFER_TRACE_SNAPSHOT_BYTES: usize = 132;
pub const TRANSFER_TRACE_FLAG_LIBRARY_PENDING: u8 = 1 << 0;
pub const TRANSFER_TRACE_FLAG_SATURATED: u8 = 1 << 1;
pub const TRANSFER_TRACE_FLAGS: u8 =
    TRANSFER_TRACE_FLAG_LIBRARY_PENDING | TRANSFER_TRACE_FLAG_SATURATED;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransferDirection {
    Upload = 0,
    Readback = 1,
    Unspecified = 255,
}

impl TransferDirection {
    pub(crate) fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Upload),
            1 => Some(Self::Readback),
            255 => Some(Self::Unspecified),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransferTraceAction {
    Start = 0,
    Get = 1,
    Clear = 2,
}

impl TransferTraceAction {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Start),
            1 => Some(Self::Get),
            2 => Some(Self::Clear),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransferTraceState {
    Active = 0,
    Succeeded = 1,
    Failed = 2,
    Aborted = 3,
}

impl TransferTraceState {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Active),
            1 => Some(Self::Succeeded),
            2 => Some(Self::Failed),
            3 => Some(Self::Aborted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferTraceRequest {
    pub action: TransferTraceAction,
    pub direction: TransferDirection,
    pub trace_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferTraceSnapshot {
    pub direction: TransferDirection,
    pub state: TransferTraceState,
    pub selected_window: u8,
    pub trace_id: u32,
    pub chunk_size: u16,
    pub peak_rx_bytes: u16,
    pub logical_bytes: u32,
    pub frames: u32,
    pub usb_rx_bytes: u32,
    pub usb_tx_bytes: u32,
    pub usb_rx_us: u32,
    pub usb_tx_us: u32,
    pub framing_us: u32,
    pub frame_crc_us: u32,
    pub owner_queue_us: u32,
    pub owner_execute_us: u32,
    pub sd_read_calls: u32,
    pub sd_write_calls: u32,
    pub sd_read_blocks: u32,
    pub sd_write_blocks: u32,
    pub sd_read_logical_bytes: u32,
    pub sd_write_logical_bytes: u32,
    pub sd_read_us: u32,
    pub sd_write_us: u32,
    pub fat_allocation_us: u32,
    pub upload_prepare_us: u32,
    pub data_io_us: u32,
    pub crc_us: u32,
    pub commit_us: u32,
    pub library_refresh_us: u32,
    pub data_phase_us: u32,
    pub end_to_end_us: u32,
    pub usb_sd_overlap_us: u32,
    pub peak_slots: u8,
    pub flags: u8,
    pub queue_stalls: u16,
    pub protocol_errors_start: u32,
    pub protocol_errors_end: u32,
}

pub fn encode_transfer_trace_request(
    value: &TransferTraceRequest,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    validate_request(value)?;
    if out.len() < TRANSFER_TRACE_REQUEST_BYTES {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[..4].copy_from_slice(&[value.action as u8, value.direction as u8, 0, 0]);
    out[4..8].copy_from_slice(&value.trace_id.to_le_bytes());
    Ok(TRANSFER_TRACE_REQUEST_BYTES)
}

pub fn decode_transfer_trace_request(
    payload: &[u8],
) -> Result<TransferTraceRequest, ProtocolError> {
    if payload.len() != TRANSFER_TRACE_REQUEST_BYTES {
        return Err(ProtocolError::BadPayloadLength);
    }
    if payload[2] != 0 || payload[3] != 0 {
        return Err(ProtocolError::InvalidValue);
    }
    let value = TransferTraceRequest {
        action: TransferTraceAction::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)?,
        direction: TransferDirection::from_u8(payload[1]).ok_or(ProtocolError::InvalidValue)?,
        trace_id: u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]),
    };
    validate_request(&value)?;
    Ok(value)
}

fn validate_request(value: &TransferTraceRequest) -> Result<(), ProtocolError> {
    let valid = match value.action {
        TransferTraceAction::Start => {
            value.trace_id == 0 && value.direction != TransferDirection::Unspecified
        }
        TransferTraceAction::Get | TransferTraceAction::Clear => value.trace_id != 0,
    };
    if valid {
        Ok(())
    } else {
        Err(ProtocolError::InvalidValue)
    }
}

pub fn encode_transfer_trace_snapshot(
    value: &TransferTraceSnapshot,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    validate_snapshot(value)?;
    if out.len() < TRANSFER_TRACE_SNAPSHOT_BYTES {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&[
        1,
        value.direction as u8,
        value.state as u8,
        value.selected_window,
    ]);
    out[4..8].copy_from_slice(&value.trace_id.to_le_bytes());
    out[8..10].copy_from_slice(&value.chunk_size.to_le_bytes());
    out[10..12].copy_from_slice(&value.peak_rx_bytes.to_le_bytes());
    let fields = [
        value.logical_bytes,
        value.frames,
        value.usb_rx_bytes,
        value.usb_tx_bytes,
        value.usb_rx_us,
        value.usb_tx_us,
        value.framing_us,
        value.frame_crc_us,
        value.owner_queue_us,
        value.owner_execute_us,
        value.sd_read_calls,
        value.sd_write_calls,
        value.sd_read_blocks,
        value.sd_write_blocks,
        value.sd_read_logical_bytes,
        value.sd_write_logical_bytes,
        value.sd_read_us,
        value.sd_write_us,
        value.fat_allocation_us,
        value.upload_prepare_us,
        value.data_io_us,
        value.crc_us,
        value.commit_us,
        value.library_refresh_us,
        value.data_phase_us,
        value.end_to_end_us,
        value.usb_sd_overlap_us,
    ];
    for (index, field) in fields.iter().enumerate() {
        let offset = 12 + index * 4;
        out[offset..offset + 4].copy_from_slice(&field.to_le_bytes());
    }
    out[120] = value.peak_slots;
    out[121] = value.flags;
    out[122..124].copy_from_slice(&value.queue_stalls.to_le_bytes());
    out[124..128].copy_from_slice(&value.protocol_errors_start.to_le_bytes());
    out[128..132].copy_from_slice(&value.protocol_errors_end.to_le_bytes());
    Ok(TRANSFER_TRACE_SNAPSHOT_BYTES)
}

pub fn decode_transfer_trace_snapshot(
    payload: &[u8],
) -> Result<TransferTraceSnapshot, ProtocolError> {
    if payload.len() != TRANSFER_TRACE_SNAPSHOT_BYTES {
        return Err(ProtocolError::BadPayloadLength);
    }
    if payload[0] != 1 {
        return Err(ProtocolError::InvalidValue);
    }
    let u16_at = |offset| u16::from_le_bytes([payload[offset], payload[offset + 1]]);
    let u32_at = |offset| u32::from_le_bytes(payload[offset..offset + 4].try_into().unwrap());
    let mut fields = [0; 27];
    for (index, field) in fields.iter_mut().enumerate() {
        *field = u32_at(12 + index * 4);
    }
    let value = TransferTraceSnapshot {
        direction: TransferDirection::from_u8(payload[1]).ok_or(ProtocolError::InvalidValue)?,
        state: TransferTraceState::from_u8(payload[2]).ok_or(ProtocolError::InvalidValue)?,
        selected_window: payload[3],
        trace_id: u32_at(4),
        chunk_size: u16_at(8),
        peak_rx_bytes: u16_at(10),
        logical_bytes: fields[0],
        frames: fields[1],
        usb_rx_bytes: fields[2],
        usb_tx_bytes: fields[3],
        usb_rx_us: fields[4],
        usb_tx_us: fields[5],
        framing_us: fields[6],
        frame_crc_us: fields[7],
        owner_queue_us: fields[8],
        owner_execute_us: fields[9],
        sd_read_calls: fields[10],
        sd_write_calls: fields[11],
        sd_read_blocks: fields[12],
        sd_write_blocks: fields[13],
        sd_read_logical_bytes: fields[14],
        sd_write_logical_bytes: fields[15],
        sd_read_us: fields[16],
        sd_write_us: fields[17],
        fat_allocation_us: fields[18],
        upload_prepare_us: fields[19],
        data_io_us: fields[20],
        crc_us: fields[21],
        commit_us: fields[22],
        library_refresh_us: fields[23],
        data_phase_us: fields[24],
        end_to_end_us: fields[25],
        usb_sd_overlap_us: fields[26],
        peak_slots: payload[120],
        flags: payload[121],
        queue_stalls: u16_at(122),
        protocol_errors_start: u32_at(124),
        protocol_errors_end: u32_at(128),
    };
    validate_snapshot(&value)?;
    Ok(value)
}

fn validate_snapshot(value: &TransferTraceSnapshot) -> Result<(), ProtocolError> {
    if value.direction == TransferDirection::Unspecified
        || value.selected_window == 0
        || value.trace_id == 0
        || value.chunk_size == 0
        || value.flags & !TRANSFER_TRACE_FLAGS != 0
    {
        return Err(ProtocolError::InvalidValue);
    }
    Ok(())
}
