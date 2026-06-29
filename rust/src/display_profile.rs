use crate::header::read_le16;
use crate::Error;

pub const WAVEFORM_UNKNOWN: u16 = 0;
pub const WAVEFORM_SSD1677_ABSOLUTE_GRAY2: u16 = 1;
pub const WAVEFORM_SSD1677_STAGED_GRAY2: u16 = 2;

pub const DISPLAY_PROFILE_REQUIRED_BYTES: usize = 56;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayProfileInfo {
    pub physical_width: u16,
    pub physical_height: u16,
    pub waveform_hint: u16,
}

pub(crate) fn parse_display_profile(bytes: &[u8]) -> Result<DisplayProfileInfo, Error> {
    if bytes.len() < DISPLAY_PROFILE_REQUIRED_BYTES {
        return Err(Error::InvalidDisplayProfile);
    }
    let waveform_hint = read_le16(bytes, 53);
    if !matches!(
        waveform_hint,
        WAVEFORM_UNKNOWN | WAVEFORM_SSD1677_ABSOLUTE_GRAY2 | WAVEFORM_SSD1677_STAGED_GRAY2
    ) {
        return Err(Error::UnsupportedWaveformHint(waveform_hint));
    }
    Ok(DisplayProfileInfo {
        physical_width: read_le16(bytes, 28),
        physical_height: read_le16(bytes, 30),
        waveform_hint,
    })
}
