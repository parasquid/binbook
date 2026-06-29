pub trait ReadAt {
    type Error;

    fn len(&mut self) -> Result<u64, Self::Error>;

    fn is_empty(&mut self) -> Result<bool, Self::Error> {
        self.len().map(|length| length == 0)
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceReadError {
    LengthOverflow,
    OutOfBounds {
        offset: u64,
        length: usize,
        source_length: usize,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct SliceSource<'a> {
    bytes: &'a [u8],
}

impl<'a> SliceSource<'a> {
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }
}

impl ReadAt for SliceSource<'_> {
    type Error = SliceReadError;

    fn len(&mut self) -> Result<u64, Self::Error> {
        u64::try_from(self.bytes.len()).map_err(|_| SliceReadError::LengthOverflow)
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        let start = usize::try_from(offset).map_err(|_| SliceReadError::OutOfBounds {
            offset,
            length: out.len(),
            source_length: self.bytes.len(),
        })?;
        let end = start
            .checked_add(out.len())
            .ok_or(SliceReadError::OutOfBounds {
                offset,
                length: out.len(),
                source_length: self.bytes.len(),
            })?;
        let source = self
            .bytes
            .get(start..end)
            .ok_or(SliceReadError::OutOfBounds {
                offset,
                length: out.len(),
                source_length: self.bytes.len(),
            })?;
        out.copy_from_slice(source);
        Ok(())
    }
}
