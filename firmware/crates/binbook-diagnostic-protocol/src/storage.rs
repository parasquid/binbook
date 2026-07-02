use crate::{ProtocolError, StorageBackend};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreListRequest<'a> {
    pub backend: StorageBackend,
    pub path: &'a str,
}

pub fn encode_store_list_request(
    value: &StoreListRequest<'_>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let path_bytes = value.path.as_bytes();
    let total = 1 + 2 + path_bytes.len();
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = value.backend as u8;
    out[1..3].copy_from_slice(&(path_bytes.len() as u16).to_le_bytes());
    out[3..total].copy_from_slice(path_bytes);
    Ok(total)
}

pub fn decode_store_list_request(payload: &[u8]) -> Result<StoreListRequest<'_>, ProtocolError> {
    if payload.len() < 3 {
        return Err(ProtocolError::BadPayloadLength);
    }
    let backend = StorageBackend::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)?;
    let path_len = u16::from_le_bytes([payload[1], payload[2]]) as usize;
    if payload.len() != 3 + path_len {
        return Err(ProtocolError::BadPayloadLength);
    }
    let path = core::str::from_utf8(&payload[3..]).map_err(|_| ProtocolError::InvalidValue)?;
    Ok(StoreListRequest { backend, path })
}

pub const STORE_LIST_ENTRY_TYPE_BYTES: usize = 1;
pub const STORE_LIST_ENTRY_NAME_LEN_BYTES: usize = 2;
pub const STORE_LIST_ENTRY_SIZE_BYTES: usize = 4;
pub const STORE_LIST_ENTRY_HEADER_BYTES: usize =
    STORE_LIST_ENTRY_TYPE_BYTES + STORE_LIST_ENTRY_NAME_LEN_BYTES + STORE_LIST_ENTRY_SIZE_BYTES;

pub const STORE_LIST_ENTRY_NAME_MAX: usize = 128;

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
    entry_type: u8,
    name: &[u8],
    size: u32,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let total = STORE_LIST_ENTRY_HEADER_BYTES + name.len();
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = entry_type;
    out[1..3].copy_from_slice(&(name.len() as u16).to_le_bytes());
    out[3..3 + name.len()].copy_from_slice(name);
    out[3 + name.len()..3 + name.len() + 4].copy_from_slice(&size.to_le_bytes());
    Ok(total)
}

pub fn decode_store_list_entry(payload: &[u8]) -> Result<(u8, &[u8], u32), ProtocolError> {
    if payload.len() < STORE_LIST_ENTRY_HEADER_BYTES {
        return Err(ProtocolError::BadPayloadLength);
    }
    let entry_type = payload[0];
    let name_len = u16::from_le_bytes([payload[1], payload[2]]) as usize;
    if name_len > STORE_LIST_ENTRY_NAME_MAX {
        return Err(ProtocolError::InvalidValue);
    }
    if payload.len() < STORE_LIST_ENTRY_HEADER_BYTES + name_len {
        return Err(ProtocolError::BadPayloadLength);
    }
    let name = &payload[3..3 + name_len];
    let size_offset = STORE_LIST_ENTRY_HEADER_BYTES + name_len - STORE_LIST_ENTRY_SIZE_BYTES;
    let size = u32::from_le_bytes([
        payload[size_offset],
        payload[size_offset + 1],
        payload[size_offset + 2],
        payload[size_offset + 3],
    ]);
    Ok((entry_type, name, size))
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
    pub path: &'a str,
}

pub fn encode_store_read_request(
    value: &StoreReadRequest<'_>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let path_bytes = value.path.as_bytes();
    let total = 1 + 2 + path_bytes.len();
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = value.backend as u8;
    out[1..3].copy_from_slice(&(path_bytes.len() as u16).to_le_bytes());
    out[3..total].copy_from_slice(path_bytes);
    Ok(total)
}

pub fn decode_store_read_request(payload: &[u8]) -> Result<StoreReadRequest<'_>, ProtocolError> {
    if payload.len() < 3 {
        return Err(ProtocolError::BadPayloadLength);
    }
    let backend = StorageBackend::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)?;
    let path_len = u16::from_le_bytes([payload[1], payload[2]]) as usize;
    if payload.len() != 3 + path_len {
        return Err(ProtocolError::BadPayloadLength);
    }
    let path = core::str::from_utf8(&payload[3..]).map_err(|_| ProtocolError::InvalidValue)?;
    Ok(StoreReadRequest { backend, path })
}

pub fn encode_store_read_response(data: &[u8], out: &mut [u8]) -> Result<usize, ProtocolError> {
    let total = 4 + data.len();
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0..4].copy_from_slice(&(data.len() as u32).to_le_bytes());
    out[4..total].copy_from_slice(data);
    Ok(total)
}

pub fn decode_store_read_response(payload: &[u8]) -> Result<&[u8], ProtocolError> {
    if payload.len() < 4 {
        return Err(ProtocolError::BadPayloadLength);
    }
    let data_len = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
    if payload.len() != 4 + data_len {
        return Err(ProtocolError::BadPayloadLength);
    }
    Ok(&payload[4..])
}

// ---------------------------------------------------------------------------
// StoreUploadBegin
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreUploadBeginRequest<'a> {
    pub backend: StorageBackend,
    pub path: &'a str,
    pub file_size: u32,
    pub expected_crc32: u32,
}

pub fn encode_store_upload_begin_request(
    value: &StoreUploadBeginRequest<'_>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let path_bytes = value.path.as_bytes();
    let total = 1 + 2 + path_bytes.len() + 4 + 4;
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    let mut pos = 0;
    out[pos] = value.backend as u8;
    pos += 1;
    out[pos..pos + 2].copy_from_slice(&(path_bytes.len() as u16).to_le_bytes());
    pos += 2;
    out[pos..pos + path_bytes.len()].copy_from_slice(path_bytes);
    pos += path_bytes.len();
    out[pos..pos + 4].copy_from_slice(&value.file_size.to_le_bytes());
    pos += 4;
    out[pos..pos + 4].copy_from_slice(&value.expected_crc32.to_le_bytes());
    pos += 4;
    Ok(pos)
}

pub fn decode_store_upload_begin_request(
    payload: &[u8],
) -> Result<StoreUploadBeginRequest<'_>, ProtocolError> {
    if payload.len() < 11 {
        return Err(ProtocolError::BadPayloadLength);
    }
    let backend = StorageBackend::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)?;
    let path_len = u16::from_le_bytes([payload[1], payload[2]]) as usize;
    let required = 1 + 2 + path_len + 4 + 4;
    if payload.len() != required {
        return Err(ProtocolError::BadPayloadLength);
    }
    let path =
        core::str::from_utf8(&payload[3..3 + path_len]).map_err(|_| ProtocolError::InvalidValue)?;
    let mut pos = 3 + path_len;
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
        path,
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
    pub path: &'a str,
}

pub fn encode_store_delete_request(
    value: &StoreDeleteRequest<'_>,
    out: &mut [u8],
) -> Result<usize, ProtocolError> {
    let path_bytes = value.path.as_bytes();
    let total = 1 + 2 + path_bytes.len();
    if out.len() < total {
        return Err(ProtocolError::OutputTooSmall);
    }
    out[0] = value.backend as u8;
    out[1..3].copy_from_slice(&(path_bytes.len() as u16).to_le_bytes());
    out[3..total].copy_from_slice(path_bytes);
    Ok(total)
}

pub fn decode_store_delete_request(
    payload: &[u8],
) -> Result<StoreDeleteRequest<'_>, ProtocolError> {
    if payload.len() < 3 {
        return Err(ProtocolError::BadPayloadLength);
    }
    let backend = StorageBackend::from_u8(payload[0]).ok_or(ProtocolError::InvalidValue)?;
    let path_len = u16::from_le_bytes([payload[1], payload[2]]) as usize;
    if payload.len() != 3 + path_len {
        return Err(ProtocolError::BadPayloadLength);
    }
    let path = core::str::from_utf8(&payload[3..]).map_err(|_| ProtocolError::InvalidValue)?;
    Ok(StoreDeleteRequest { backend, path })
}
