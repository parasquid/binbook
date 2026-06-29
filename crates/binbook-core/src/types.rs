use crate::FormatError;

macro_rules! u32_identifier {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(u32);

        impl $name {
            pub(crate) const fn from_validated(value: u32) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn get(self) -> u32 {
                self.0
            }
        }
    };
}

u32_identifier!(PageNumber);
u32_identifier!(NavNumber);
u32_identifier!(ChapterNumber);
u32_identifier!(ChunkRecordNumber);
u32_identifier!(TransitionNumber);

impl PageNumber {
    pub const fn new(value: u32, page_count: u32) -> Result<Self, FormatError> {
        if value < page_count {
            Ok(Self(value))
        } else {
            Err(FormatError::PageOutOfRange)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChunkIndex(u8);

impl ChunkIndex {
    pub const fn new(value: u8, count: u8) -> Result<Self, FormatError> {
        if value < count {
            Ok(Self(value))
        } else {
            Err(FormatError::ChunkOutOfRange)
        }
    }

    pub(crate) const fn from_validated(value: u8) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaneSlot {
    OverlayMsb,
    OverlayLsb,
    FastBase,
    Reserved,
}

impl PlaneSlot {
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::OverlayMsb => 0,
            Self::OverlayLsb => 1,
            Self::FastBase => 2,
            Self::Reserved => 3,
        }
    }
}

impl TryFrom<u8> for PlaneSlot {
    type Error = FormatError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::OverlayMsb),
            1 => Ok(Self::OverlayLsb),
            2 => Ok(Self::FastBase),
            3 => Ok(Self::Reserved),
            _ => Err(FormatError::PlaneSlotOutOfRange),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileOffset(u64);

impl FileOffset {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn from_validated(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteLength(u32);

impl ByteLength {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub(crate) const fn from_validated(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionMethod {
    None,
    RlePackBits,
    Lz4,
}

impl TryFrom<u16> for CompressionMethod {
    type Error = FormatError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::RlePackBits),
            2 => Ok(Self::Lz4),
            other => Err(FormatError::UnsupportedCompression(other)),
        }
    }
}

impl TryFrom<u8> for CompressionMethod {
    type Error = FormatError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::try_from(u16::from(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Gray1Packed,
    Gray2Packed,
    Gray4Packed,
    Rgb565,
    Rgb888,
    Rgba8888,
}

impl TryFrom<u16> for PixelFormat {
    type Error = FormatError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Gray1Packed),
            2 => Ok(Self::Gray2Packed),
            4 => Ok(Self::Gray4Packed),
            8 => Ok(Self::Rgb565),
            16 => Ok(Self::Rgb888),
            32 => Ok(Self::Rgba8888),
            other => Err(FormatError::UnsupportedPixelFormat(other)),
        }
    }
}
