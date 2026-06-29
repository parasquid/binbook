use crate::header::{read_u16, read_u32};
use crate::{FormatError, StringRef};

pub(crate) const DISPLAY_PROFILE_MIN: usize = 56;
pub(crate) const BOOK_METADATA_MIN: usize = 56;
pub const WAVEFORM_UNKNOWN: u16 = 0;
pub const WAVEFORM_SSD1677_ABSOLUTE_GRAY2: u16 = 1;
pub const WAVEFORM_SSD1677_STAGED_GRAY2: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayProfile {
    pub profile_id: StringRef,
    pub device_family: StringRef,
    pub device_model: StringRef,
    pub logical_width: u16,
    pub logical_height: u16,
    pub physical_width: u16,
    pub physical_height: u16,
    pub logical_orientation: u8,
    pub logical_to_physical_rotation: i16,
    pub scan_order_hint: u8,
    pub supported_storage_pixel_formats: u32,
    pub native_output_pixel_formats: u32,
    pub native_grayscale_levels: u16,
    pub panel_grayscale_levels: u16,
    pub framebuffer_bits_per_pixel: u8,
    pub waveform_hint: u16,
    pub dither_mode: u8,
}

impl DisplayProfile {
    pub(crate) fn parse(bytes: &[u8], string_length: u64) -> Result<Self, FormatError> {
        if bytes.len() < DISPLAY_PROFILE_MIN {
            return Err(FormatError::InvalidDisplayProfile);
        }
        let profile = Self {
            profile_id: StringRef::parse(bytes, 0)?,
            device_family: StringRef::parse(bytes, 8)?,
            device_model: StringRef::parse(bytes, 16)?,
            logical_width: read_u16(bytes, 24)?,
            logical_height: read_u16(bytes, 26)?,
            physical_width: read_u16(bytes, 28)?,
            physical_height: read_u16(bytes, 30)?,
            logical_orientation: bytes[32],
            logical_to_physical_rotation: i16::from_le_bytes([bytes[33], bytes[34]]),
            scan_order_hint: bytes[35],
            supported_storage_pixel_formats: read_u32(bytes, 36)?,
            native_output_pixel_formats: read_u32(bytes, 40)?,
            native_grayscale_levels: read_u16(bytes, 48)?,
            panel_grayscale_levels: read_u16(bytes, 50)?,
            framebuffer_bits_per_pixel: bytes[52],
            waveform_hint: read_u16(bytes, 53)?,
            dither_mode: bytes[55],
        };
        profile.profile_id.validate(string_length)?;
        profile.device_family.validate(string_length)?;
        profile.device_model.validate(string_length)?;
        if profile.logical_width == 0 || profile.logical_height == 0 {
            return Err(FormatError::InvalidDisplayProfile);
        }
        if !matches!(
            profile.waveform_hint,
            WAVEFORM_UNKNOWN | WAVEFORM_SSD1677_ABSOLUTE_GRAY2 | WAVEFORM_SSD1677_STAGED_GRAY2
        ) {
            return Err(FormatError::UnsupportedWaveformHint(profile.waveform_hint));
        }
        Ok(profile)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BookMetadata {
    pub title: StringRef,
    pub subtitle: StringRef,
    pub author: StringRef,
    pub publisher: StringRef,
    pub language: StringRef,
    pub series_name: StringRef,
    pub series_index_milli: u32,
}

impl BookMetadata {
    pub(crate) fn parse(bytes: &[u8], string_length: u64) -> Result<Self, FormatError> {
        if bytes.len() < BOOK_METADATA_MIN {
            return Err(FormatError::InvalidBookMetadata);
        }
        let metadata = Self {
            title: StringRef::parse(bytes, 0)?,
            subtitle: StringRef::parse(bytes, 8)?,
            author: StringRef::parse(bytes, 16)?,
            publisher: StringRef::parse(bytes, 24)?,
            language: StringRef::parse(bytes, 32)?,
            series_name: StringRef::parse(bytes, 40)?,
            series_index_milli: read_u32(bytes, 48)?,
        };
        for reference in [
            metadata.title,
            metadata.subtitle,
            metadata.author,
            metadata.publisher,
            metadata.language,
            metadata.series_name,
        ] {
            reference.validate(string_length)?;
        }
        Ok(metadata)
    }
}
