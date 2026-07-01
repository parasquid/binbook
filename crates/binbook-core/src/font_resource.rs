use crate::header::{read_u16, read_u32};
use crate::{FormatError, StringRef};

pub const FONT_RESOURCE_RECORD_SIZE: usize = 80;
const KNOWN_FLAGS: u16 = 0x0f;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum FontSourceKind {
    Bundled = 1,
    Epub = 2,
}

impl TryFrom<u16> for FontSourceKind {
    type Error = FormatError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Bundled),
            2 => Ok(Self::Epub),
            _ => Err(FormatError::InvalidFontResource),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FontStyle {
    Normal = 0,
    Italic = 1,
    Oblique = 2,
}

impl TryFrom<u8> for FontStyle {
    type Error = FormatError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Normal),
            1 => Ok(Self::Italic),
            2 => Ok(Self::Oblique),
            _ => Err(FormatError::InvalidFontResource),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontResourceIndexEntry {
    pub font_index: u32,
    pub source_kind: FontSourceKind,
    pub flags: u16,
    pub weight: u16,
    pub stretch_milli: u16,
    pub style: FontStyle,
    pub family: StringRef,
    pub source_path: StringRef,
    pub sha256: [u8; 32],
    pub face_index: u32,
}

impl FontResourceIndexEntry {
    pub fn parse(
        bytes: &[u8],
        expected_index: u32,
        string_table_length: u64,
    ) -> Result<Self, FormatError> {
        let record = bytes
            .get(..FONT_RESOURCE_RECORD_SIZE)
            .ok_or(FormatError::InvalidFontResource)?;
        let font_index = read_u32(record, 0)?;
        let flags = read_u16(record, 6)?;
        if font_index != expected_index
            || flags & !KNOWN_FLAGS != 0
            || record[13] != 0
            || record[14..16] != [0, 0]
            || record[68..80].iter().any(|byte| *byte != 0)
        {
            return Err(FormatError::InvalidFontResource);
        }
        let family = StringRef::parse(record, 16)?;
        let source_path = StringRef::parse(record, 24)?;
        family.validate(string_table_length)?;
        source_path.validate(string_table_length)?;
        let mut sha256 = [0_u8; 32];
        sha256.copy_from_slice(&record[32..64]);
        Ok(Self {
            font_index,
            source_kind: FontSourceKind::try_from(read_u16(record, 4)?)?,
            flags,
            weight: read_u16(record, 8)?,
            stretch_milli: read_u16(record, 10)?,
            style: FontStyle::try_from(record[12])?,
            family,
            source_path,
            sha256,
            face_index: read_u32(record, 64)?,
        })
    }
}
