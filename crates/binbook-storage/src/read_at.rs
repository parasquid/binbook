//! `ReadAt` adapter wrapping a `Filesystem` + filename.
//!
//! Maps `ReadAt::len` → `Filesystem::file_size` and `ReadAt::read_exact_at` →
//! `Filesystem::read_at`. All backend errors are surfaced as `FsReadError::Backend`.

use crate::filesystem::Filesystem;
use binbook_core::ReadAt;

/// A `ReadAt` over a named file in a `Filesystem`.
///
/// Holds a `&mut F`, so it is single-use (one open book at a time per backend
/// handle) — matching the constrained-RAM, single-active-book firmware model.
pub struct FsReadAt<'a, F: Filesystem> {
    pub(crate) fs: &'a mut F,
    pub(crate) name: &'a str,
}

impl<'a, F: Filesystem> FsReadAt<'a, F> {
    pub fn new(fs: &'a mut F, name: &'a str) -> Self {
        Self { fs, name }
    }
}

/// Errors from `FsReadAt` operations.
#[derive(Debug)]
pub enum FsReadError<E> {
    /// The underlying backend returned an error.
    Backend(E),
    /// The named file was not found.
    NotFound,
}

impl<'a, F: Filesystem> ReadAt for FsReadAt<'a, F> {
    type Error = FsReadError<F::Error>;

    fn len(&mut self) -> Result<u64, Self::Error> {
        self.fs.file_size(self.name).map_err(FsReadError::Backend)
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        self.fs
            .read_at(self.name, offset, out)
            .map_err(FsReadError::Backend)
    }
}
