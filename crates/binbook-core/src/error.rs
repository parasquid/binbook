use core::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatError {
    InvalidMagic,
    UnsupportedVersion,
    InvalidHeader,
    FileOutOfBounds,
    MissingSection(u16),
    DuplicateSection(u16),
    InvalidSection,
    InvalidPage,
    InvalidNavigation,
    InvalidChapter,
    InvalidChunk,
    InvalidTransition,
    InvalidFontResource,
    InvalidDisplayProfile,
    InvalidBookMetadata,
    InvalidStringRef,
    PageOutOfRange,
    NavOutOfRange,
    ChapterOutOfRange,
    ChunkOutOfRange,
    TransitionOutOfRange,
    PlaneSlotOutOfRange,
    UnsupportedPixelFormat(u16),
    UnsupportedCompression(u16),
    UnsupportedWaveformHint(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error<E> {
    Source(E),
    Format(FormatError),
    BufferTooSmall { required: usize, provided: usize },
}

impl<E> From<FormatError> for Error<E> {
    fn from(value: FormatError) -> Self {
        Self::Format(value)
    }
}

impl Display for FormatError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl<E: Display> Display for Error<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Source(error) => write!(f, "source error: {error}"),
            Self::Format(error) => Display::fmt(error, f),
            Self::BufferTooSmall { required, provided } => {
                write!(
                    f,
                    "buffer too small: required {required}, provided {provided}"
                )
            }
        }
    }
}
