use crate::RenderError;

pub fn pack_gray1_row(pixels: &[u8], output: &mut [u8]) -> Result<(), RenderError> {
    pack_row(pixels, output, 8, 1)
}

pub fn pack_gray2_row(pixels: &[u8], output: &mut [u8]) -> Result<(), RenderError> {
    pack_row(pixels, output, 4, 3)
}

fn pack_row(
    pixels: &[u8],
    output: &mut [u8],
    pixels_per_byte: usize,
    maximum: u8,
) -> Result<(), RenderError> {
    if pixels.iter().any(|value| *value > maximum) {
        return Err(RenderError::InvalidPixelValue);
    }
    let required = pixels.len().div_ceil(pixels_per_byte);
    if output.len() < required {
        return Err(RenderError::BufferTooSmall {
            required,
            provided: output.len(),
        });
    }
    output[..required].fill(0);
    let bits = 8 / pixels_per_byte;
    for (index, pixel) in pixels.iter().copied().enumerate() {
        let shift = 8 - bits * (index % pixels_per_byte + 1);
        output[index / pixels_per_byte] |= pixel << shift;
    }
    Ok(())
}
