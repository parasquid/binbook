#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayError {
    Source,
    Format,
    Decode,
    Render,
    Controller,
    InvalidProfile,
    InvalidPage,
    InvalidState,
    BufferTooSmall { required: usize, provided: usize },
}

pub type DisplayResult<T> = Result<T, DisplayError>;

impl From<binbook_decompress::DecodeError> for DisplayError {
    fn from(_: binbook_decompress::DecodeError) -> Self {
        Self::Decode
    }
}

impl From<gray2_render::RenderError> for DisplayError {
    fn from(_: gray2_render::RenderError) -> Self {
        Self::Render
    }
}

impl From<ssd1677_driver::Error> for DisplayError {
    fn from(_: ssd1677_driver::Error) -> Self {
        Self::Controller
    }
}
