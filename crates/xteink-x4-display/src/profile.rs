use binbook_core::{
    DisplayProfile, PageInfo, PixelFormat, PlaneSlot, WAVEFORM_SSD1677_STAGED_GRAY2,
};

use crate::{DisplayError, DisplayResult};

pub const LOGICAL_WIDTH: u16 = 480;
pub const LOGICAL_HEIGHT: u16 = 800;
pub const PHYSICAL_WIDTH: u16 = 800;
pub const PHYSICAL_HEIGHT: u16 = 480;
pub const ROTATION_DEGREES: i16 = 270;
pub const CHUNK_ROWS: u16 = 16;
pub const CHUNK_COUNT: u8 = 30;
pub const ROW_BYTES: usize = 100;
pub const PLANE_BYTES: usize = ROW_BYTES * PHYSICAL_HEIGHT as usize;

#[must_use]
pub const fn logical_to_physical(logical_x: u16, logical_y: u16) -> (u16, u16) {
    (LOGICAL_HEIGHT - 1 - logical_y, logical_x)
}

pub fn logical_gray2_to_physical_packed(logical: &[u8], output: &mut [u8]) -> DisplayResult<()> {
    let required_input = usize::from(LOGICAL_WIDTH) * usize::from(LOGICAL_HEIGHT);
    let required_output = usize::from(PHYSICAL_WIDTH) * usize::from(PHYSICAL_HEIGHT) / 4;
    if logical.len() != required_input {
        return Err(DisplayError::InvalidPage);
    }
    if output.len() < required_output {
        return Err(DisplayError::BufferTooSmall {
            required: required_output,
            provided: output.len(),
        });
    }
    if logical.iter().any(|value| *value > 3) {
        return Err(DisplayError::Render);
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

pub fn validate_profile(profile: &DisplayProfile) -> DisplayResult<()> {
    if profile.logical_width != LOGICAL_WIDTH
        || profile.logical_height != LOGICAL_HEIGHT
        || profile.physical_width != PHYSICAL_WIDTH
        || profile.physical_height != PHYSICAL_HEIGHT
        || profile.logical_to_physical_rotation != ROTATION_DEGREES
        || profile.waveform_hint != WAVEFORM_SSD1677_STAGED_GRAY2
    {
        return Err(DisplayError::InvalidProfile);
    }
    Ok(())
}

pub fn validate_page(page: &PageInfo) -> DisplayResult<()> {
    let planes = page.planes;
    if page.pixel_format != PixelFormat::Gray2Packed
        || page.stored_width != PHYSICAL_WIDTH
        || page.stored_height != PHYSICAL_HEIGHT
        || planes.bitmap() != 0b0000_0111
        || planes.get(PlaneSlot::OverlayMsb).is_none()
        || planes.get(PlaneSlot::OverlayLsb).is_none()
        || planes.get(PlaneSlot::FastBase).is_none()
    {
        return Err(DisplayError::InvalidPage);
    }
    Ok(())
}
