use crate::{canonical_row_to_staged, RenderError};

pub fn canonical_image_to_staged(
    packed: &[u8],
    width: usize,
    height: usize,
    msb: &mut [u8],
    lsb: &mut [u8],
    base: &mut [u8],
) -> Result<(), RenderError> {
    if width == 0 || height == 0 || !width.is_multiple_of(8) {
        return Err(RenderError::InvalidDimensions);
    }
    let packed_row = width / 4;
    let plane_row = width / 8;
    let packed_required = packed_row
        .checked_mul(height)
        .ok_or(RenderError::InvalidDimensions)?;
    let plane_required = plane_row
        .checked_mul(height)
        .ok_or(RenderError::InvalidDimensions)?;
    if packed.len() != packed_required {
        return Err(RenderError::InvalidDimensions);
    }
    require(msb, plane_required)?;
    require(lsb, plane_required)?;
    require(base, plane_required)?;
    for row in 0..height {
        let input = &packed[row * packed_row..(row + 1) * packed_row];
        let start = row * plane_row;
        canonical_row_to_staged(
            input,
            &mut msb[start..start + plane_row],
            &mut lsb[start..start + plane_row],
            &mut base[start..start + plane_row],
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct PlaneChunks<'a> {
    plane: &'a [u8],
    chunk_bytes: usize,
    offset: usize,
}

impl<'a> PlaneChunks<'a> {
    pub fn new(plane: &'a [u8], row_bytes: usize, chunk_rows: usize) -> Result<Self, RenderError> {
        let chunk_bytes = row_bytes
            .checked_mul(chunk_rows)
            .ok_or(RenderError::InvalidDimensions)?;
        if chunk_bytes == 0 || !plane.len().is_multiple_of(chunk_bytes) {
            return Err(RenderError::InvalidDimensions);
        }
        Ok(Self {
            plane,
            chunk_bytes,
            offset: 0,
        })
    }
}

impl<'a> Iterator for PlaneChunks<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        let end = self.offset.checked_add(self.chunk_bytes)?;
        let chunk = self.plane.get(self.offset..end)?;
        self.offset = end;
        Some(chunk)
    }
}

fn require(buffer: &[u8], required: usize) -> Result<(), RenderError> {
    if buffer.len() < required {
        Err(RenderError::BufferTooSmall {
            required,
            provided: buffer.len(),
        })
    } else {
        Ok(())
    }
}
