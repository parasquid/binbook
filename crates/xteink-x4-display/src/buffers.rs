use crate::{DisplayError, DisplayResult};

pub struct RenderBuffers<'a> {
    pub compressed: &'a mut [u8],
    pub decoded: &'a mut [u8],
    pub black: &'a mut [u8],
    pub red: &'a mut [u8],
}

impl<'a> RenderBuffers<'a> {
    pub fn new(
        compressed: &'a mut [u8],
        decoded: &'a mut [u8],
        black: &'a mut [u8],
        red: &'a mut [u8],
    ) -> Self {
        Self {
            compressed,
            decoded,
            black,
            red,
        }
    }

    pub fn require_streaming(&self) -> DisplayResult<()> {
        require_nonempty(self.compressed)?;
        require_nonempty(self.decoded)
    }

    pub fn require_planes(&self, required: usize) -> DisplayResult<()> {
        require(self.black, required)?;
        require(self.red, required)
    }
}

fn require_nonempty(buffer: &[u8]) -> DisplayResult<()> {
    if buffer.is_empty() {
        Err(DisplayError::BufferTooSmall {
            required: 1,
            provided: 0,
        })
    } else {
        Ok(())
    }
}

fn require(buffer: &[u8], required: usize) -> DisplayResult<()> {
    if buffer.len() < required {
        Err(DisplayError::BufferTooSmall {
            required,
            provided: buffer.len(),
        })
    } else {
        Ok(())
    }
}

pub(crate) fn split_three(buffer: &mut [u8]) -> DisplayResult<(&mut [u8], &mut [u8], &mut [u8])> {
    if buffer.len() < 3 {
        return Err(DisplayError::BufferTooSmall {
            required: 3,
            provided: buffer.len(),
        });
    }
    let third = buffer.len() / 3;
    let (first, rest) = buffer.split_at_mut(third);
    let (second, third_buffer) = rest.split_at_mut(third);
    Ok((first, second, third_buffer))
}

pub(crate) fn row_triplet(buffer: &mut [u8]) -> DisplayResult<(&mut [u8], &mut [u8], &mut [u8])> {
    const REQUIRED: usize = crate::profile::ROW_BYTES * 3;
    if buffer.len() < REQUIRED {
        return Err(DisplayError::BufferTooSmall {
            required: REQUIRED,
            provided: buffer.len(),
        });
    }
    let (first, rest) = buffer.split_at_mut(crate::profile::ROW_BYTES);
    let (second, rest) = rest.split_at_mut(crate::profile::ROW_BYTES);
    Ok((first, second, &mut rest[..crate::profile::ROW_BYTES]))
}

pub(crate) fn first_input(buffer: &mut [u8]) -> DisplayResult<&mut [u8]> {
    if buffer.is_empty() {
        Err(DisplayError::BufferTooSmall {
            required: 1,
            provided: 0,
        })
    } else {
        Ok(buffer)
    }
}

pub(crate) fn first_row(buffer: &mut [u8]) -> DisplayResult<&mut [u8]> {
    require(buffer, crate::profile::ROW_BYTES)?;
    Ok(&mut buffer[..crate::profile::ROW_BYTES])
}

pub(crate) fn require_row(buffer: &[u8]) -> DisplayResult<()> {
    require(buffer, crate::profile::ROW_BYTES)
}
