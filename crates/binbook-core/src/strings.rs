use crate::header::read_u32;
use crate::FormatError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StringRef {
    pub offset: u32,
    pub length: u32,
}

impl StringRef {
    pub(crate) fn parse(bytes: &[u8], offset: usize) -> Result<Self, FormatError> {
        Ok(Self {
            offset: read_u32(bytes, offset)?,
            length: read_u32(bytes, offset + 4)?,
        })
    }

    pub(crate) fn validate(self, table_length: u64) -> Result<(), FormatError> {
        if self.length == 0 {
            return Ok(());
        }
        let end = u64::from(self.offset)
            .checked_add(u64::from(self.length))
            .ok_or(FormatError::InvalidStringRef)?;
        if end <= table_length {
            Ok(())
        } else {
            Err(FormatError::InvalidStringRef)
        }
    }
}
