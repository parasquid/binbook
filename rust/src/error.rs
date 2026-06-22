#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    InvalidMagic,
    UnsupportedVersion,
    InvalidHeader,
    MissingSection(u16),
    InvalidSection,
    InvalidPageIndex,
    InvalidNavIndex,
    InvalidChapterIndex,
    InvalidStringRef,
    PageOutOfRange,
    NavOutOfRange,
    ChapterOutOfRange,
    UnsupportedPixelFormat(u16),
    UnsupportedCompression(u16),
    DecompressFailed,
    OutputBufferTooSmall,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidMagic => write!(f, "invalid binbook magic"),
            Self::UnsupportedVersion => write!(f, "unsupported binbook version"),
            Self::InvalidHeader => write!(f, "invalid binbook header"),
            Self::MissingSection(id) => write!(f, "missing required section {id}"),
            Self::InvalidSection => write!(f, "invalid section entry"),
            Self::InvalidPageIndex => write!(f, "invalid page index"),
            Self::InvalidNavIndex => write!(f, "invalid nav index"),
            Self::InvalidChapterIndex => write!(f, "invalid chapter index"),
            Self::InvalidStringRef => write!(f, "invalid string reference"),
            Self::PageOutOfRange => write!(f, "page index out of range"),
            Self::NavOutOfRange => write!(f, "nav index out of range"),
            Self::ChapterOutOfRange => write!(f, "chapter index out of range"),
            Self::UnsupportedPixelFormat(pf) => write!(f, "unsupported pixel format {pf}"),
            Self::UnsupportedCompression(cm) => write!(f, "unsupported compression method {cm}"),
            Self::DecompressFailed => write!(f, "decompression failed"),
            Self::OutputBufferTooSmall => write!(f, "output buffer too small"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}
