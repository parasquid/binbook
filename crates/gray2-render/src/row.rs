use crate::{canonical_bits, unpack, RenderError};

pub fn canonical_row_to_absolute(
    packed: &[u8],
    red: &mut [u8],
    black: &mut [u8],
) -> Result<(), RenderError> {
    let required = output_length(packed)?;
    require(red, required)?;
    require(black, required)?;
    red[..required].fill(0);
    black[..required].fill(0);
    let pixel_count = packed.len() * 4;
    for (packed_index, byte) in packed.iter().copied().enumerate() {
        for (pixel_index, gray) in unpack(byte).into_iter().enumerate() {
            let ram_x = pixel_count - 1 - (packed_index * 4 + pixel_index);
            let bits = canonical_bits(gray);
            if bits.red_active {
                set_bit(red, ram_x);
            }
            if bits.black_active {
                set_bit(black, ram_x);
            }
        }
    }
    Ok(())
}

pub fn canonical_row_to_staged(
    packed: &[u8],
    msb: &mut [u8],
    lsb: &mut [u8],
    base: &mut [u8],
) -> Result<(), RenderError> {
    let required = output_length(packed)?;
    require(msb, required)?;
    require(lsb, required)?;
    require(base, required)?;
    msb[..required].fill(0);
    lsb[..required].fill(0);
    base[..required].fill(0xff);
    let pixel_count = packed.len() * 4;
    for (packed_index, byte) in packed.iter().copied().enumerate() {
        for (pixel_index, gray) in unpack(byte).into_iter().enumerate() {
            let ram_x = pixel_count - 1 - (packed_index * 4 + pixel_index);
            match gray {
                crate::CanonicalGray2::Black => clear_bit(base, ram_x),
                crate::CanonicalGray2::DarkGray => {
                    set_bit(msb, ram_x);
                    set_bit(lsb, ram_x);
                    clear_bit(base, ram_x);
                }
                crate::CanonicalGray2::LightGray => {
                    set_bit(msb, ram_x);
                    clear_bit(base, ram_x);
                }
                crate::CanonicalGray2::White => {}
            }
        }
    }
    Ok(())
}

fn output_length(packed: &[u8]) -> Result<usize, RenderError> {
    if !packed.len().is_multiple_of(2) {
        Err(RenderError::InvalidPackedRowLength)
    } else {
        Ok(packed.len() / 2)
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

fn set_bit(row: &mut [u8], x: usize) {
    row[x / 8] |= 0x80 >> (x % 8);
}

fn clear_bit(row: &mut [u8], x: usize) {
    row[x / 8] &= !(0x80 >> (x % 8));
}
