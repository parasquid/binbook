use crate::FormatError;

pub(crate) const RENDITION_IDENTITY_LENGTH: u64 = 312;
pub(crate) const RENDITION_HASH_LENGTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenditionIdentity {
    pub rendition_hash: [u8; RENDITION_HASH_LENGTH],
}

pub(crate) fn parse(bytes: &[u8]) -> Result<RenditionIdentity, FormatError> {
    let rendition_hash: [u8; RENDITION_HASH_LENGTH] = bytes
        .get(..RENDITION_HASH_LENGTH)
        .ok_or(FormatError::InvalidSection)?
        .try_into()
        .map_err(|_| FormatError::InvalidSection)?;
    if rendition_hash.iter().all(|byte| *byte == 0) {
        return Err(FormatError::InvalidSection);
    }
    Ok(RenditionIdentity { rendition_hash })
}
