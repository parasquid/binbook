#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("document has no linear content")]
    EmptyDocument,
    #[error("font data is invalid")]
    InvalidFont,
    #[error("raster page could not be compiled")]
    Raster,
}
