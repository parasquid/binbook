use crate::ProtocolError;

pub const SD_WRITE_TRACE_REQUEST_BYTES: usize = 8;
pub const SD_WRITE_TRACE_RESPONSE_HEADER_BYTES: usize = 16;
pub const SD_WRITE_TRACE_RECORD_BYTES: usize = 32;
pub const SD_WRITE_TRACE_MAX_PAGE_RECORDS: u16 = 127;
pub const SD_WRITE_TRACE_FLAG_OVERFLOW: u8 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum SdSectorPurpose {
    #[default]
    Unknown = 0,
    Reserved = 1,
    Fat0 = 2,
    Fat1 = 3,
    Directory = 4,
    Journal = 5,
    Payload = 6,
    DataOther = 7,
    OutsideVolume = 8,
}

impl TryFrom<u8> for SdSectorPurpose {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Unknown),
            1 => Ok(Self::Reserved),
            2 => Ok(Self::Fat0),
            3 => Ok(Self::Fat1),
            4 => Ok(Self::Directory),
            5 => Ok(Self::Journal),
            6 => Ok(Self::Payload),
            7 => Ok(Self::DataOther),
            8 => Ok(Self::OutsideVolume),
            _ => Err(ProtocolError::InvalidValue),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SdWriteTraceAction {
    Clear = 0,
    Get = 1,
    Freeze = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SdWriteTraceRequest {
    pub action: SdWriteTraceAction,
    pub start_index: u16,
    pub max_records: u16,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SdWriteTraceRecord {
    pub start_block: u32,
    pub blocks: u16,
    pub command: u8,
    pub flags: u8,
    pub command_setup_us: u32,
    pub data_spi_us: u32,
    pub busy_wait_us: u32,
    pub total_us: u32,
    pub gap_us: u32,
    pub purpose: SdSectorPurpose,
}

pub fn encode_sd_write_trace_request(
    value: &SdWriteTraceRequest,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    validate_request(value)?;
    if out.len() < SD_WRITE_TRACE_REQUEST_BYTES {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = value.action as u8;
    out[1] = 0;
    out[2..4].copy_from_slice(&value.start_index.to_le_bytes());
    out[4..6].copy_from_slice(&value.max_records.to_le_bytes());
    out[6..8].fill(0);
    Ok(SD_WRITE_TRACE_REQUEST_BYTES)
}

pub fn decode_sd_write_trace_request(payload: &[u8]) -> Result<SdWriteTraceRequest, ProtocolError> {
    if payload.len() != SD_WRITE_TRACE_REQUEST_BYTES {
        return Err(ProtocolError::BadPayloadLength);
    }
    if payload[1] != 0 || payload[6] != 0 || payload[7] != 0 {
        return Err(ProtocolError::InvalidValue);
    }
    let action = match payload[0] {
        0 => SdWriteTraceAction::Clear,
        1 => SdWriteTraceAction::Get,
        2 => SdWriteTraceAction::Freeze,
        _ => return Err(ProtocolError::InvalidValue),
    };
    let value = SdWriteTraceRequest {
        action,
        start_index: u16::from_le_bytes([payload[2], payload[3]]),
        max_records: u16::from_le_bytes([payload[4], payload[5]]),
    };
    validate_request(&value)?;
    Ok(value)
}

fn validate_request(value: &SdWriteTraceRequest) -> Result<(), ProtocolError> {
    let valid = match value.action {
        SdWriteTraceAction::Clear | SdWriteTraceAction::Freeze => {
            value.start_index == 0 && value.max_records == 0
        }
        SdWriteTraceAction::Get => {
            value.max_records > 0 && value.max_records <= SD_WRITE_TRACE_MAX_PAGE_RECORDS
        }
    };
    valid.then_some(()).ok_or(ProtocolError::InvalidValue)
}

pub fn encode_sd_write_trace_response(
    total_records: u16,
    capacity: u16,
    start_index: u16,
    overflowed: bool,
    anomalies: u32,
    records: &[SdWriteTraceRecord],
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    encode_sd_write_trace_response_with(
        total_records,
        capacity,
        start_index,
        overflowed,
        anomalies,
        records.len() as u16,
        |index| {
            records
                .get(index)
                .copied()
                .ok_or(ProtocolError::InvalidValue)
        },
        out,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "paged zero-copy encoding keeps trace metadata and the record callback explicit"
)]
pub fn encode_sd_write_trace_response_with(
    total_records: u16,
    capacity: u16,
    start_index: u16,
    overflowed: bool,
    anomalies: u32,
    record_count: u16,
    mut get_record: impl FnMut(usize) -> Result<SdWriteTraceRecord, ProtocolError>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    if record_count > SD_WRITE_TRACE_MAX_PAGE_RECORDS
        || start_index > total_records
        || start_index.saturating_add(record_count) > total_records
    {
        return Err(ProtocolError::InvalidValue);
    }
    let total = SD_WRITE_TRACE_RESPONSE_HEADER_BYTES
        + usize::from(record_count) * SD_WRITE_TRACE_RECORD_BYTES;
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = 1;
    out[1] = if overflowed {
        SD_WRITE_TRACE_FLAG_OVERFLOW
    } else {
        0
    };
    out[2..4].copy_from_slice(&total_records.to_le_bytes());
    out[4..6].copy_from_slice(&capacity.to_le_bytes());
    out[6..8].copy_from_slice(&start_index.to_le_bytes());
    out[8..10].copy_from_slice(&record_count.to_le_bytes());
    out[10..14].copy_from_slice(&anomalies.to_le_bytes());
    out[14..16].fill(0);
    for index in 0..usize::from(record_count) {
        let value = get_record(index)?;
        let at = SD_WRITE_TRACE_RESPONSE_HEADER_BYTES + index * SD_WRITE_TRACE_RECORD_BYTES;
        let bytes = &mut out[at..at + SD_WRITE_TRACE_RECORD_BYTES];
        bytes[0..4].copy_from_slice(&value.start_block.to_le_bytes());
        bytes[4..6].copy_from_slice(&value.blocks.to_le_bytes());
        bytes[6] = value.command;
        bytes[7] = value.flags;
        for (field_index, field) in [
            value.command_setup_us,
            value.data_spi_us,
            value.busy_wait_us,
            value.total_us,
            value.gap_us,
        ]
        .iter()
        .enumerate()
        {
            let offset = 8 + field_index * 4;
            bytes[offset..offset + 4].copy_from_slice(&field.to_le_bytes());
        }
        bytes[28] = value.purpose as u8;
        bytes[29..32].fill(0);
    }
    Ok(total)
}

pub fn decode_sd_write_trace_response(
    payload: &[u8],
    mut push: impl FnMut(SdWriteTraceRecord) -> Result<(), ProtocolError>,
) -> Result<(u16, u16, u16, u16, bool, u32), ProtocolError> {
    if payload.len() < SD_WRITE_TRACE_RESPONSE_HEADER_BYTES || payload[0] != 1 {
        return Err(ProtocolError::BadPayloadLength);
    }
    if payload[1] & !SD_WRITE_TRACE_FLAG_OVERFLOW != 0 || payload[14] != 0 || payload[15] != 0 {
        return Err(ProtocolError::InvalidValue);
    }
    let total = u16::from_le_bytes([payload[2], payload[3]]);
    let capacity = u16::from_le_bytes([payload[4], payload[5]]);
    let start = u16::from_le_bytes([payload[6], payload[7]]);
    let count = u16::from_le_bytes([payload[8], payload[9]]);
    let anomalies = u32::from_le_bytes([payload[10], payload[11], payload[12], payload[13]]);
    let expected =
        SD_WRITE_TRACE_RESPONSE_HEADER_BYTES + usize::from(count) * SD_WRITE_TRACE_RECORD_BYTES;
    if payload.len() != expected || start > total || start.saturating_add(count) > total {
        return Err(ProtocolError::BadPayloadLength);
    }
    for index in 0..usize::from(count) {
        let at = SD_WRITE_TRACE_RESPONSE_HEADER_BYTES + index * SD_WRITE_TRACE_RECORD_BYTES;
        let bytes = &payload[at..at + SD_WRITE_TRACE_RECORD_BYTES];
        if bytes[29..32] != [0; 3] {
            return Err(ProtocolError::InvalidValue);
        }
        let u32_at = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        push(SdWriteTraceRecord {
            start_block: u32_at(0),
            blocks: u16::from_le_bytes([bytes[4], bytes[5]]),
            command: bytes[6],
            flags: bytes[7],
            command_setup_us: u32_at(8),
            data_spi_us: u32_at(12),
            busy_wait_us: u32_at(16),
            total_us: u32_at(20),
            gap_us: u32_at(24),
            purpose: SdSectorPurpose::try_from(bytes[28])?,
        })?;
    }
    Ok((total, capacity, start, count, payload[1] != 0, anomalies))
}
