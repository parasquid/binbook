use heapless::String;

pub const UPLOAD_JOURNAL_BYTES: usize = 148;
pub const UPLOAD_TARGET_NAME_BYTES: usize = 121;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum UploadJournalPhase {
    Clear = 0,
    Receiving = 1,
    Verified = 2,
    OldMoved = 3,
    Published = 4,
}

impl UploadJournalPhase {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Clear),
            1 => Some(Self::Receiving),
            2 => Some(Self::Verified),
            3 => Some(Self::OldMoved),
            4 => Some(Self::Published),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadJournalRecord {
    pub phase: UploadJournalPhase,
    pub generation: u32,
    pub expected_size: u32,
    pub expected_crc32: u32,
    pub target_name: String<UPLOAD_TARGET_NAME_BYTES>,
}

impl UploadJournalRecord {
    pub fn new(
        phase: UploadJournalPhase,
        generation: u32,
        expected_size: u32,
        expected_crc32: u32,
        target_name: &str,
    ) -> Result<Self, UploadJournalError> {
        if generation == 0
            || (phase != UploadJournalPhase::Clear && expected_size == 0)
            || target_name.is_empty()
            || target_name.len() > UPLOAD_TARGET_NAME_BYTES
            || target_name.contains(['/', '\\'])
        {
            return Err(UploadJournalError::InvalidValue);
        }
        Ok(Self {
            phase,
            generation,
            expected_size,
            expected_crc32,
            target_name: String::try_from(target_name)
                .map_err(|_| UploadJournalError::InvalidValue)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadJournalError {
    InvalidLength,
    InvalidValue,
    BadCrc,
}

pub fn encode_upload_journal(
    record: &UploadJournalRecord,
    out: &mut [u8],
) -> Result<(), UploadJournalError> {
    if out.len() != UPLOAD_JOURNAL_BYTES {
        return Err(UploadJournalError::InvalidLength);
    }
    let validated = UploadJournalRecord::new(
        record.phase,
        record.generation,
        record.expected_size,
        record.expected_crc32,
        record.target_name.as_str(),
    )?;
    out.fill(0);
    out[..4].copy_from_slice(b"BBXJ");
    out[4] = 1;
    out[5] = validated.phase as u8;
    out[6] = validated.target_name.len() as u8;
    out[8..12].copy_from_slice(&validated.generation.to_le_bytes());
    out[12..16].copy_from_slice(&validated.expected_size.to_le_bytes());
    out[16..20].copy_from_slice(&validated.expected_crc32.to_le_bytes());
    out[20..20 + validated.target_name.len()].copy_from_slice(validated.target_name.as_bytes());
    let crc = crc32(&out[..144]);
    out[144..148].copy_from_slice(&crc.to_le_bytes());
    Ok(())
}

pub fn decode_upload_journal(bytes: &[u8]) -> Result<UploadJournalRecord, UploadJournalError> {
    if bytes.len() != UPLOAD_JOURNAL_BYTES {
        return Err(UploadJournalError::InvalidLength);
    }
    if &bytes[..4] != b"BBXJ" || bytes[4] != 1 || bytes[7] != 0 || bytes[141..144] != [0, 0, 0] {
        return Err(UploadJournalError::InvalidValue);
    }
    let expected_crc = u32::from_le_bytes(bytes[144..148].try_into().unwrap());
    if crc32(&bytes[..144]) != expected_crc {
        return Err(UploadJournalError::BadCrc);
    }
    let name_len = usize::from(bytes[6]);
    if name_len == 0 || name_len > UPLOAD_TARGET_NAME_BYTES {
        return Err(UploadJournalError::InvalidValue);
    }
    let name = core::str::from_utf8(&bytes[20..20 + name_len])
        .map_err(|_| UploadJournalError::InvalidValue)?;
    if bytes[20 + name_len..141].iter().any(|byte| *byte != 0) {
        return Err(UploadJournalError::InvalidValue);
    }
    UploadJournalRecord::new(
        UploadJournalPhase::from_u8(bytes[5]).ok_or(UploadJournalError::InvalidValue)?,
        u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
        u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
        u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
        name,
    )
}

/// Return true only when a physical sector begins with a complete, CRC-valid
/// upload journal record. Bytes after the fixed record are filesystem padding
/// and are intentionally ignored.
pub fn is_upload_journal_sector(sector: &[u8; 512]) -> bool {
    decode_upload_journal(&sector[..UPLOAD_JOURNAL_BYTES]).is_ok()
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut state = u32::MAX;
    for byte in bytes {
        state ^= u32::from(*byte);
        for _ in 0..8 {
            state = if state & 1 == 0 {
                state >> 1
            } else {
                (state >> 1) ^ 0xedb8_8320
            };
        }
    }
    !state
}
