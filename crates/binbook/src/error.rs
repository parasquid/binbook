use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("I/O error for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("unsupported input: {0}")]
    UnsupportedInput(PathBuf),
    #[error("input format does not match --input-format")]
    FormatMismatch,
    #[error("image directory contains no encodable pages")]
    NoEncodablePages,
    #[error("compiler failed: {0}")]
    Compile(#[from] binbook_compiler::CompileError),
    #[error("invalid BinBook")]
    InvalidBook,
    #[error("page {0} is out of range")]
    PageOutOfRange(u32),
    #[error("PNG encoding failed")]
    Png,
    #[error("output path has no file name")]
    InvalidOutputPath,
}

pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> CliError {
    CliError::Io {
        path: path.into(),
        source,
    }
}
