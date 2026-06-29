use crate::FormatError;

pub(crate) const HEADER_SIZE: usize = 256;
const MAGIC: &[u8; 8] = b"BINBOOK\0";

#[derive(Debug, Clone, Copy)]
pub(crate) struct Header {
    pub file_length: u64,
    pub section_table_offset: u64,
    pub section_table_length: u32,
    pub section_count: u16,
    pub page_data_offset: u64,
    pub page_data_length: u64,
}

pub(crate) fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, FormatError> {
    let raw: [u8; 2] = bytes
        .get(offset..offset + 2)
        .ok_or(FormatError::InvalidHeader)?
        .try_into()
        .map_err(|_| FormatError::InvalidHeader)?;
    Ok(u16::from_le_bytes(raw))
}

pub(crate) fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, FormatError> {
    let raw: [u8; 4] = bytes
        .get(offset..offset + 4)
        .ok_or(FormatError::InvalidHeader)?
        .try_into()
        .map_err(|_| FormatError::InvalidHeader)?;
    Ok(u32::from_le_bytes(raw))
}

pub(crate) fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, FormatError> {
    let raw: [u8; 8] = bytes
        .get(offset..offset + 8)
        .ok_or(FormatError::InvalidHeader)?
        .try_into()
        .map_err(|_| FormatError::InvalidHeader)?;
    Ok(u64::from_le_bytes(raw))
}

pub(crate) fn parse(bytes: &[u8], source_length: u64) -> Result<Header, FormatError> {
    if bytes.len() < HEADER_SIZE {
        return Err(FormatError::InvalidHeader);
    }
    if bytes.get(..8) != Some(MAGIC) {
        return Err(FormatError::InvalidMagic);
    }
    if read_u16(bytes, 12)? != 256
        || read_u16(bytes, 36)? != 40
        || read_u16(bytes, 40)? != 128
        || read_u16(bytes, 42)? != 48
    {
        return Err(FormatError::UnsupportedVersion);
    }
    let declared_length = read_u64(bytes, 16)?;
    if declared_length != 0 && declared_length > source_length {
        return Err(FormatError::FileOutOfBounds);
    }
    let file_length = if declared_length == 0 {
        source_length
    } else {
        declared_length
    };
    let header = Header {
        file_length,
        section_table_offset: read_u64(bytes, 24)?,
        section_table_length: read_u32(bytes, 32)?,
        section_count: read_u16(bytes, 38)?,
        page_data_offset: read_u64(bytes, 44)?,
        page_data_length: read_u64(bytes, 52)?,
    };
    let required_table_length = u32::from(header.section_count)
        .checked_mul(40)
        .ok_or(FormatError::InvalidHeader)?;
    if header.section_count == 0
        || header.section_table_offset < 256
        || header.section_table_length < required_table_length
    {
        return Err(FormatError::InvalidHeader);
    }
    Ok(header)
}
