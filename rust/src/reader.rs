use crate::Error;

pub trait Reader {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), Error>;
}

impl Reader for &[u8] {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
        let start = offset as usize;
        let end = start + buf.len();
        if end > self.len() {
            return Err(Error::InvalidHeader);
        }
        buf.copy_from_slice(&self[start..end]);
        Ok(())
    }
}
