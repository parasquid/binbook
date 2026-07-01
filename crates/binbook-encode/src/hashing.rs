use sha2::{Digest, Sha256};

#[must_use]
pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

#[must_use]
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    crc ^ 0xffff_ffff
}

pub(crate) fn set_section_hash(bytes: &mut [u8], offset: usize) -> [u8; 32] {
    let hash = sha256(bytes);
    bytes[offset..offset + 32].copy_from_slice(&hash);
    hash
}
