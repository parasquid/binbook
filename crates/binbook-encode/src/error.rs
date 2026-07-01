use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelError {
    NoPages,
    TooManyRecords,
    InvalidPlaneSlot,
    DuplicatePlaneSlot,
    EmptyPlane,
    EmptyChunk,
    InvalidNavigation,
    InvalidFont,
    LengthOverflow,
}

#[derive(Debug)]
pub enum EncodeError {
    Io(io::Error),
    InvalidModel(ModelError),
    Wire(binbook_core::EncodeError),
}

impl From<io::Error> for EncodeError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ModelError> for EncodeError {
    fn from(value: ModelError) -> Self {
        Self::InvalidModel(value)
    }
}

impl From<binbook_core::EncodeError> for EncodeError {
    fn from(value: binbook_core::EncodeError) -> Self {
        Self::Wire(value)
    }
}
