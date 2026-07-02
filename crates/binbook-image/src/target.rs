use crate::ImageError;

pub(crate) const LOGICAL_WIDTH: u16 = 480;
pub(crate) const LOGICAL_HEIGHT: u16 = 800;
pub(crate) const PHYSICAL_WIDTH: u16 = 800;
pub(crate) const PHYSICAL_HEIGHT: u16 = 480;
pub(crate) const CHUNK_ROWS: u16 = 16;
pub(crate) const ROW_BYTES: usize = 100;

#[must_use]
pub(crate) const fn logical_to_physical(logical_x: u16, logical_y: u16) -> (u16, u16) {
    (LOGICAL_HEIGHT - 1 - logical_y, logical_x)
}

pub(crate) fn logical_gray2_unpacked_to_physical_packed(
    logical: &[u8],
    output: &mut [u8],
) -> Result<(), ImageError> {
    let required_input = usize::from(LOGICAL_WIDTH) * usize::from(LOGICAL_HEIGHT);
    let required_output = usize::from(PHYSICAL_WIDTH) * usize::from(PHYSICAL_HEIGHT) / 4;
    if logical.len() != required_input {
        return Err(ImageError::InvalidDimensions);
    }
    if output.len() < required_output {
        return Err(ImageError::InvalidDimensions);
    }
    if logical.iter().any(|value| *value > 3) {
        return Err(ImageError::Format);
    }

    output[..required_output].fill(0);
    for logical_y in 0..LOGICAL_HEIGHT {
        for logical_x in 0..LOGICAL_WIDTH {
            let logical_index =
                usize::from(logical_y) * usize::from(LOGICAL_WIDTH) + usize::from(logical_x);
            let (physical_x, physical_y) = logical_to_physical(logical_x, logical_y);
            let physical_index =
                usize::from(physical_y) * usize::from(PHYSICAL_WIDTH) + usize::from(physical_x);
            let shift = 6 - (physical_index % 4) * 2;
            output[physical_index / 4] |= logical[logical_index] << shift;
        }
    }
    Ok(())
}
