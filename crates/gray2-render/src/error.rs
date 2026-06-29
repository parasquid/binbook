#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderError {
    InvalidPackedRowLength,
    BufferTooSmall { required: usize, provided: usize },
}
