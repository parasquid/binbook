use crate::RenderError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbsolutePlaneByte {
    pub red: u8,
    pub black: u8,
}

#[must_use]
pub const fn staged_byte_to_absolute(msb: u8, lsb: u8, base: u8) -> AbsolutePlaneByte {
    AbsolutePlaneByte {
        red: !(base | (msb & !lsb)),
        black: !(base | lsb),
    }
}

pub fn staged_row_to_absolute(
    msb: &[u8],
    lsb: &[u8],
    base: &[u8],
    red: &mut [u8],
    black: &mut [u8],
) -> Result<(), RenderError> {
    let required = msb.len();
    if lsb.len() != required || base.len() != required {
        return Err(RenderError::InvalidPackedRowLength);
    }
    require(red, required)?;
    require(black, required)?;
    for index in 0..required {
        let absolute = staged_byte_to_absolute(msb[index], lsb[index], base[index]);
        red[index] = absolute.red;
        black[index] = absolute.black;
    }
    Ok(())
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
