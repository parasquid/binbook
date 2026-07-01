use std::fs;
use std::io::Write;
use std::path::Path;

use crate::atomic_output::write_atomic;
use crate::error::io;
use crate::CliError;

pub fn run_decode(book: &Path, page: u32, output: &Path) -> Result<(), CliError> {
    let bytes = fs::read(book).map_err(|source| io(book, source))?;
    let decoded = binbook_image::decode_book_page(&bytes, page)
        .map_err(|_| CliError::PageOutOfRange(page))?;
    let png = binbook_image::encode_page_png(&decoded).map_err(|_| CliError::Png)?;
    write_atomic(output, |target| {
        target
            .write_all(&png)
            .map_err(|source| io(output, source))?;
        Ok(())
    })
}
