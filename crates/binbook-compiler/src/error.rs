#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("source contains no inputs")]
    EmptySource,
    #[error("image decoding or compilation failed")]
    Image,
    #[error("EPUB package parsing failed")]
    Epub,
    #[error("document rendering failed")]
    Render,
    #[error("BinBook assembly failed")]
    Assemble,
    #[error("assembled BinBook failed strict validation")]
    Validate,
    #[error("output sink failed")]
    Output(#[source] std::io::Error),
}
