use binbook_core::StringRef;

use crate::ModelError;

#[derive(Debug, Default)]
pub(crate) struct StringTable {
    bytes: Vec<u8>,
    entries: Vec<(String, StringRef)>,
}

impl StringTable {
    pub(crate) fn add(&mut self, value: &str) -> Result<StringRef, ModelError> {
        if value.is_empty() {
            return Ok(StringRef::default());
        }
        if let Some((_, reference)) = self.entries.iter().find(|(entry, _)| entry == value) {
            return Ok(*reference);
        }
        let offset = u32::try_from(self.bytes.len()).map_err(|_| ModelError::LengthOverflow)?;
        let length = u32::try_from(value.len()).map_err(|_| ModelError::LengthOverflow)?;
        let reference = StringRef { offset, length };
        self.bytes.extend_from_slice(value.as_bytes());
        self.entries.push((value.into(), reference));
        Ok(reference)
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}
