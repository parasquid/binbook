use crate::{ReadAt, ValidationError};

pub(crate) fn crc_range<R: ReadAt>(
    source: &mut R,
    offset: u64,
    length: u64,
    scratch: &mut [u8],
) -> Result<u32, ValidationError<R::Error>> {
    let mut crc = Crc32::new();
    crc.update_range(source, offset, length, scratch)?;
    Ok(crc.finish())
}

pub(crate) struct Crc32(u32);

impl Crc32 {
    pub(crate) const fn new() -> Self {
        Self(0xffff_ffff)
    }

    fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u32::from(*byte);
            for _ in 0..8 {
                self.0 = (self.0 >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(self.0 & 1));
            }
        }
    }

    pub(crate) fn update_range<R: ReadAt>(
        &mut self,
        source: &mut R,
        mut offset: u64,
        mut length: u64,
        scratch: &mut [u8],
    ) -> Result<(), ValidationError<R::Error>> {
        if scratch.is_empty() {
            return Err(ValidationError::BufferTooSmall {
                required: 1,
                provided: 0,
            });
        }
        while length != 0 {
            let count = usize::try_from(length.min(scratch.len() as u64)).unwrap_or(scratch.len());
            source
                .read_exact_at(offset, &mut scratch[..count])
                .map_err(ValidationError::Source)?;
            self.update(&scratch[..count]);
            offset = offset.saturating_add(count as u64);
            length -= count as u64;
        }
        Ok(())
    }

    pub(crate) const fn finish(self) -> u32 {
        self.0 ^ 0xffff_ffff
    }
}
