#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderError {
    InvalidPackedRowLength,
    InvalidDimensions,
    InvalidPixelValue,
    BufferTooSmall { required: usize, provided: usize },
}
