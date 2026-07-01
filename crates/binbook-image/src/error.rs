#[derive(Debug)]
pub enum ImageError {
    UnsupportedFormat,
    AnimatedImage,
    Decode,
    InvalidDimensions,
    InvalidPage,
    BufferTooSmall,
    Compression,
    Format,
    Encode,
}

impl From<gray2_render::RenderError> for ImageError {
    fn from(_: gray2_render::RenderError) -> Self {
        Self::Format
    }
}

impl From<binbook_decompress::DecodeError> for ImageError {
    fn from(_: binbook_decompress::DecodeError) -> Self {
        Self::Compression
    }
}
