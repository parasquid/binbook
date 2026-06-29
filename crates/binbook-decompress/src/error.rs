use core::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    MalformedRun,
    OutputTooShort { expected: usize, actual: usize },
    OutputTooLong { expected: usize },
    Lz4Disabled,
    Lz4Failure,
}

impl Display for DecodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
