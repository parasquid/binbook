use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};

use crate::storage_readback::ReadbackError;

pub trait BulkReadbackOutput: Write + Seek {
    fn truncate(&mut self, len: u64) -> std::io::Result<()>;
}

impl BulkReadbackOutput for File {
    fn truncate(&mut self, len: u64) -> std::io::Result<()> {
        self.set_len(len)
    }
}

impl BulkReadbackOutput for BufWriter<File> {
    fn truncate(&mut self, len: u64) -> std::io::Result<()> {
        self.flush()?;
        self.get_mut().set_len(len)
    }
}

pub(crate) fn rollback(
    output: &mut impl BulkReadbackOutput,
    offset: u64,
) -> Result<(), ReadbackError> {
    output.flush()?;
    output.truncate(offset)?;
    output.seek(SeekFrom::Start(offset))?;
    Ok(())
}
