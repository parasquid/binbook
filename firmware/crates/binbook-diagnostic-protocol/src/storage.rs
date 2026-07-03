use crate::{ProtocolError, StorageBackend};

pub const STORE_NAME_MAX: usize = 64;

fn validate_name(name: &str) -> Result<&[u8], ProtocolError> {
    let bytes = name.as_bytes();
    if bytes.is_empty()
        || bytes.len() > STORE_NAME_MAX
        || bytes.iter().any(|b| matches!(*b, b'/' | b'\\'))
    {
        return Err(ProtocolError::InvalidValue);
    }
    Ok(bytes)
}

fn read_name<'a>(payload: &'a [u8], pos: &mut usize) -> Result<&'a str, ProtocolError> {
    if *pos >= payload.len() {
        return Err(ProtocolError::BadPayloadLength);
    }
    let name_len = payload[*pos] as usize;
    *pos += 1;
    if name_len == 0 || name_len > STORE_NAME_MAX {
        return Err(ProtocolError::InvalidValue);
    }
    let end = pos
        .checked_add(name_len)
        .ok_or(ProtocolError::BadPayloadLength)?;
    if end > payload.len() {
        return Err(ProtocolError::BadPayloadLength);
    }
    let name =
        core::str::from_utf8(&payload[*pos..end]).map_err(|_| ProtocolError::InvalidValue)?;
    validate_name(name)?;
    *pos = end;
    Ok(name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreListRequest {
    pub backend: StorageBackend,
}

pub fn encode_store_list_request(
    value: &StoreListRequest,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    if out.is_empty() {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = value.backend as u8;
    Ok(1)
}

pub fn decode_store_list_request(payload: &[u8]) -> Result<StoreListRequest, ProtocolError> {
    if payload.len() != 1 {
        return Err(ProtocolError::BadPayloadLength);
    }
    let backend = StorageBackend::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)?;
    Ok(StoreListRequest { backend })
}

pub const STORE_LIST_ENTRY_NAME_LEN_BYTES: usize = 1;
pub const STORE_LIST_ENTRY_SIZE_BYTES: usize = 8;
pub const STORE_LIST_ENTRY_PAGE_COUNT_BYTES: usize = 4;
pub const STORE_LIST_ENTRY_HEADER_BYTES: usize = STORE_LIST_ENTRY_NAME_LEN_BYTES
    + STORE_LIST_ENTRY_SIZE_BYTES
    + STORE_LIST_ENTRY_PAGE_COUNT_BYTES;

pub const STORE_LIST_ENTRY_NAME_MAX: usize = STORE_NAME_MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreListEntry<'a> {
    pub name: &'a str,
    pub file_size: u64,
    pub page_count: u32,
}

pub fn encode_store_list_entries(
    out: &mut [u8],
    mut push_entry: impl FnMut(&mut [u8]) -> Result<usize, ProtocolError>,
    entry_count: u16,
) -> Result<usize, ProtocolError> {
    if out.len() < 2 {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..2].copy_from_slice(&entry_count.to_le_bytes());
    let mut pos = 2;
    for _ in 0..entry_count {
        let entry_bytes = push_entry(&mut out[pos..])?;
        pos += entry_bytes;
        if pos > out.len() {
            return Err(ProtocolError::OutputTooSmall);
        }
    }
    Ok(pos)
}

pub fn encode_store_entry(
    name: &[u8],
    file_size: u64,
    page_count: u32,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let name = core::str::from_utf8(name).map_err(|_| ProtocolError::InvalidValue)?;
    let name = validate_name(name)?;
    let total = STORE_LIST_ENTRY_HEADER_BYTES + name.len();
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = name.len() as u8;
    out[1..1 + name.len()].copy_from_slice(name);
    let mut pos = 1 + name.len();
    out[pos..pos + 8].copy_from_slice(&file_size.to_le_bytes());
    pos += 8;
    out[pos..pos + 4].copy_from_slice(&page_count.to_le_bytes());
    Ok(total)
}

pub fn decode_store_list_entry(payload: &[u8]) -> Result<StoreListEntry<'_>, ProtocolError> {
    if payload.is_empty() {
        return Err(ProtocolError::BadPayloadLength);
    }
    let name_len = payload[0] as usize;
    if name_len == 0 || name_len > STORE_LIST_ENTRY_NAME_MAX {
        return Err(ProtocolError::InvalidValue);
    }
    if payload.len() < STORE_LIST_ENTRY_HEADER_BYTES + name_len {
        return Err(ProtocolError::BadPayloadLength);
    }
    let name =
        core::str::from_utf8(&payload[1..1 + name_len]).map_err(|_| ProtocolError::InvalidValue)?;
    validate_name(name)?;
    let mut pos = 1 + name_len;
    let file_size = u64::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
        payload[pos + 4],
        payload[pos + 5],
        payload[pos + 6],
        payload[pos + 7],
    ]);
    pos += 8;
    let page_count = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    Ok(StoreListEntry {
        name,
        file_size,
        page_count,
    })
}

pub fn decode_store_list_count(payload: &[u8]) -> Result<u16, ProtocolError> {
    if payload.len() < 2 {
        return Err(ProtocolError::BadPayloadLength);
    }
    Ok(u16::from_le_bytes([payload[0], payload[1]]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreReadRequest<'a> {
    pub backend: StorageBackend,
    pub name: &'a str,
    pub offset: u32,
    pub max_bytes: u32,
}

pub fn encode_store_read_request(
    value: &StoreReadRequest<'_>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let name = validate_name(value.name)?;
    let total = 1 + 1 + name.len() + 4 + 4;
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    let mut pos = 0;
    out[pos] = value.backend as u8;
    pos += 1;
    out[pos] = name.len() as u8;
    pos += 1;
    out[pos..pos + name.len()].copy_from_slice(name);
    pos += name.len();
    out[pos..pos + 4].copy_from_slice(&value.offset.to_le_bytes());
    pos += 4;
    out[pos..pos + 4].copy_from_slice(&value.max_bytes.to_le_bytes());
    Ok(total)
}

pub fn decode_store_read_request(payload: &[u8]) -> Result<StoreReadRequest<'_>, ProtocolError> {
    if payload.len() < 10 {
        return Err(ProtocolError::BadPayloadLength);
    }
    let mut pos = 0;
    let backend = StorageBackend::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)?;
    pos += 1;
    let name = read_name(payload, &mut pos)?;
    if payload.len() != pos + 8 {
        return Err(ProtocolError::BadPayloadLength);
    }
    let offset = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let max_bytes = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    Ok(StoreReadRequest {
        backend,
        name,
        offset,
        max_bytes,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreReadResponse<'a> {
    pub data: &'a [u8],
    pub eof: bool,
}

pub fn encode_store_read_response(
    data: &[u8],
    eof: bool,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let total = 5 + data.len();
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&(data.len() as u32).to_le_bytes());
    out[4] = u8::from(eof);
    out[5..total].copy_from_slice(data);
    Ok(total)
}

pub fn decode_store_read_response(payload: &[u8]) -> Result<StoreReadResponse<'_>, ProtocolError> {
    if payload.len() < 5 {
        return Err(ProtocolError::BadPayloadLength);
    }
    let data_len = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
    if payload[4] > 1 {
        return Err(ProtocolError::InvalidValue);
    }
    if payload.len() != 5 + data_len {
        return Err(ProtocolError::BadPayloadLength);
    }
    Ok(StoreReadResponse {
        data: &payload[5..],
        eof: payload[4] != 0,
    })
}

// ---------------------------------------------------------------------------
// StoreUploadBegin
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreUploadBeginRequest<'a> {
    pub backend: StorageBackend,
    pub name: &'a str,
    pub file_size: u32,
    pub expected_crc32: u32,
}

pub fn encode_store_upload_begin_request(
    value: &StoreUploadBeginRequest<'_>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let name = validate_name(value.name)?;
    let total = 1 + 1 + name.len() + 4 + 4;
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    let mut pos = 0;
    out[pos] = value.backend as u8;
    pos += 1;
    out[pos] = name.len() as u8;
    pos += 1;
    out[pos..pos + name.len()].copy_from_slice(name);
    pos += name.len();
    out[pos..pos + 4].copy_from_slice(&value.file_size.to_le_bytes());
    pos += 4;
    out[pos..pos + 4].copy_from_slice(&value.expected_crc32.to_le_bytes());
    pos += 4;
    Ok(pos)
}

pub fn decode_store_upload_begin_request(
    payload: &[u8],
) -> Result<StoreUploadBeginRequest<'_>, ProtocolError> {
    if payload.len() < 10 {
        return Err(ProtocolError::BadPayloadLength);
    }
    let mut pos = 0;
    let backend = StorageBackend::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)?;
    pos += 1;
    let name = read_name(payload, &mut pos)?;
    if payload.len() != pos + 8 {
        return Err(ProtocolError::BadPayloadLength);
    }
    let file_size = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    pos += 4;
    let expected_crc32 = u32::from_le_bytes([
        payload[pos],
        payload[pos + 1],
        payload[pos + 2],
        payload[pos + 3],
    ]);
    Ok(StoreUploadBeginRequest {
        backend,
        name,
        file_size,
        expected_crc32,
    })
}

pub fn encode_store_upload_begin_response(
    upload_id: u32,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    if out.len() < 4 {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&upload_id.to_le_bytes());
    Ok(4)
}

pub fn decode_store_upload_begin_response(payload: &[u8]) -> Result<u32, ProtocolError> {
    if payload.len() != 4 {
        return Err(ProtocolError::BadPayloadLength);
    }
    Ok(u32::from_le_bytes([
        payload[0], payload[1], payload[2], payload[3],
    ]))
}

// ---------------------------------------------------------------------------
// StoreUploadWrite
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreUploadWriteRequest<'a> {
    pub upload_id: u32,
    pub offset: u32,
    pub data: &'a [u8],
}

pub fn encode_store_upload_write_request(
    value: &StoreUploadWriteRequest<'_>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let total = 4 + 4 + value.data.len();
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&value.upload_id.to_le_bytes());
    out[4..8].copy_from_slice(&value.offset.to_le_bytes());
    out[8..total].copy_from_slice(value.data);
    Ok(total)
}

pub fn decode_store_upload_write_request(
    payload: &[u8],
) -> Result<StoreUploadWriteRequest<'_>, ProtocolError> {
    if payload.len() < 8 {
        return Err(ProtocolError::BadPayloadLength);
    }
    let upload_id = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let offset = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
    let data = &payload[8..];
    Ok(StoreUploadWriteRequest {
        upload_id,
        offset,
        data,
    })
}

pub fn encode_store_upload_write_response(
    bytes_accepted: u32,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    if out.len() < 4 {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&bytes_accepted.to_le_bytes());
    Ok(4)
}

pub fn decode_store_upload_write_response(payload: &[u8]) -> Result<u32, ProtocolError> {
    if payload.len() != 4 {
        return Err(ProtocolError::BadPayloadLength);
    }
    Ok(u32::from_le_bytes([
        payload[0], payload[1], payload[2], payload[3],
    ]))
}

// ---------------------------------------------------------------------------
// StoreUploadCommit
// ---------------------------------------------------------------------------

pub fn encode_store_upload_commit_request(
    upload_id: u32,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    if out.len() < 4 {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&upload_id.to_le_bytes());
    Ok(4)
}

pub fn decode_store_upload_commit_request(payload: &[u8]) -> Result<u32, ProtocolError> {
    if payload.len() != 4 {
        return Err(ProtocolError::BadPayloadLength);
    }
    Ok(u32::from_le_bytes([
        payload[0], payload[1], payload[2], payload[3],
    ]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StoreUploadCommitResult {
    Ok = 0,
    CrcMismatch = 1,
}

impl StoreUploadCommitResult {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Ok),
            1 => Some(Self::CrcMismatch),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreUploadCommitResponse {
    pub result: StoreUploadCommitResult,
    pub computed_crc32: u32,
}

pub fn encode_store_upload_commit_response(
    result: StoreUploadCommitResult,
    computed_crc32: u32,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    if out.len() < 5 {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = result as u8;
    out[1..5].copy_from_slice(&computed_crc32.to_le_bytes());
    Ok(5)
}

pub fn decode_store_upload_commit_response(
    payload: &[u8],
) -> Result<StoreUploadCommitResponse, ProtocolError> {
    if payload.len() != 5 {
        return Err(ProtocolError::BadPayloadLength);
    }
    let result = StoreUploadCommitResult::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)?;
    let computed_crc32 = u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]);
    Ok(StoreUploadCommitResponse {
        result,
        computed_crc32,
    })
}

// ---------------------------------------------------------------------------
// StoreAbort
// ---------------------------------------------------------------------------

pub fn encode_store_abort_request(upload_id: u32, out: &mut [u8]) -> Result<usize, ProtocolError> {
    if out.len() < 4 {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&upload_id.to_le_bytes());
    Ok(4)
}

pub fn decode_store_abort_request(payload: &[u8]) -> Result<u32, ProtocolError> {
    if payload.len() != 4 {
        return Err(ProtocolError::BadPayloadLength);
    }
    Ok(u32::from_le_bytes([
        payload[0], payload[1], payload[2], payload[3],
    ]))
}

// ---------------------------------------------------------------------------
// StoreDelete
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreDeleteRequest<'a> {
    pub backend: StorageBackend,
    pub name: &'a str,
}

pub fn encode_store_delete_request(
    value: &StoreDeleteRequest<'_>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let name = validate_name(value.name)?;
    let total = 1 + 1 + name.len();
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = value.backend as u8;
    out[1] = name.len() as u8;
    out[2..total].copy_from_slice(name);
    Ok(total)
}

pub fn decode_store_delete_request(
    payload: &[u8],
) -> Result<StoreDeleteRequest<'_>, ProtocolError> {
    if payload.len() < 2 {
        return Err(ProtocolError::BadPayloadLength);
    }
    let mut pos = 0;
    let backend = StorageBackend::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)?;
    pos += 1;
    let name = read_name(payload, &mut pos)?;
    if payload.len() != pos {
        return Err(ProtocolError::BadPayloadLength);
    }
    Ok(StoreDeleteRequest { backend, name })
}
