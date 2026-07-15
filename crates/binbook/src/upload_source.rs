use std::io::{Read, Seek, SeekFrom};

use crate::upload_types::UploadError;

pub const UPLOAD_CHUNK_BYTES: usize =
    ((binbook_diagnostic_protocol::MAX_PAYLOAD_BYTES - 8) / 512) * 512;

pub(crate) fn measure_source<R: Read + Seek>(source: &mut R) -> Result<(u64, u32), UploadError> {
    source.seek(SeekFrom::Start(0))?;
    let mut buffer = [0u8; UPLOAD_CHUNK_BYTES];
    let mut size = 0u64;
    let mut crc = 0xFFFF_FFFFu32;
    loop {
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size += count as u64;
        crc = crc32_update(crc, &buffer[..count]);
    }
    Ok((size, !crc))
}

fn crc32_update(mut crc: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ 0xEDB8_8320
            };
        }
    }
    crc
}
