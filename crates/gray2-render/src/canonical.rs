use crate::RenderError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CanonicalGray2 {
    Black = 0,
    DarkGray = 1,
    LightGray = 2,
    White = 3,
}

impl TryFrom<u8> for CanonicalGray2 {
    type Error = RenderError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Black),
            1 => Ok(Self::DarkGray),
            2 => Ok(Self::LightGray),
            3 => Ok(Self::White),
            _ => Err(RenderError::InvalidPackedRowLength),
        }
    }
}

impl From<CanonicalGray2> for u8 {
    fn from(value: CanonicalGray2) -> Self {
        match value {
            CanonicalGray2::Black => 0,
            CanonicalGray2::DarkGray => 1,
            CanonicalGray2::LightGray => 2,
            CanonicalGray2::White => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaneBits {
    pub red_active: bool,
    pub black_active: bool,
}

impl PlaneBits {
    #[must_use]
    pub const fn new(red_active: bool, black_active: bool) -> Self {
        Self {
            red_active,
            black_active,
        }
    }
}

#[must_use]
pub const fn canonical_bits(gray: CanonicalGray2) -> PlaneBits {
    match gray {
        CanonicalGray2::Black => PlaneBits::new(true, true),
        CanonicalGray2::DarkGray => PlaneBits::new(true, false),
        CanonicalGray2::LightGray => PlaneBits::new(false, true),
        CanonicalGray2::White => PlaneBits::new(false, false),
    }
}

#[must_use]
pub fn unpack(packed: u8) -> [CanonicalGray2; 4] {
    [
        decode(packed >> 6),
        decode(packed >> 4),
        decode(packed >> 2),
        decode(packed),
    ]
}

fn decode(value: u8) -> CanonicalGray2 {
    const LEVELS: [CanonicalGray2; 4] = [
        CanonicalGray2::Black,
        CanonicalGray2::DarkGray,
        CanonicalGray2::LightGray,
        CanonicalGray2::White,
    ];
    LEVELS[usize::from(value & 3)]
}
