//! Backend-agnostic filesystem trait + BinBook entry type.

/// A backend-agnostic, readable filesystem over a flat directory of named files.
///
/// Implementations: SD FAT (in `embedded-sd-storage`, adapted in `binbook-fw`),
/// internal-flash LittleFS (roadmap). Methods borrow `&mut self` because reads
/// may touch shared hardware (a shared SPI bus).
pub trait Filesystem {
    type Error;

    /// Visit every entry in the listed directory. The callback is called once
    /// per entry with its filename (UTF-8, no path separators) and byte length.
    /// Non-UTF-8 names are skipped by the implementation.
    fn for_each_entry(&mut self, visit: &mut dyn FnMut(&str, u64)) -> Result<(), Self::Error>;

    /// Open `name` for reading and read `out.len()` bytes at byte `offset`.
    /// Returns `Err(NotFound)` if the file is absent.
    fn read_at(&mut self, name: &str, offset: u64, out: &mut [u8]) -> Result<(), Self::Error>;

    /// Total byte length of `name`, or an error if absent.
    fn file_size(&mut self, name: &str) -> Result<u64, Self::Error>;

    /// Visit valid `.binbook` entries with file size and page count.
    ///
    /// The default implementation is backend agnostic but may call
    /// [`read_at`](Self::read_at) and [`file_size`](Self::file_size) multiple
    /// times per file. Backends with expensive mount/open operations should
    /// override this method with a single-open implementation.
    fn for_each_binbook_entry(
        &mut self,
        visit: &mut dyn FnMut(&str, u64, u32),
    ) -> Result<(), Self::Error>
    where
        Self: Sized,
    {
        let mut entries = heapless::Vec::<(heapless::String<64>, u64), 64>::new();
        self.for_each_entry(&mut |name, size| {
            if name.ends_with(".binbook") {
                if let Ok(name_buf) = heapless::String::<64>::try_from(name) {
                    let _ = entries.push((name_buf, size));
                }
            }
        })?;

        let mut scratch = [0u8; 1024];
        for (name, size) in entries {
            let source = crate::read_at::FsReadAt::new(self, name.as_str());
            if let Ok(book) = binbook_core::Book::open(source, &mut scratch) {
                visit(name.as_str(), size, book.page_count());
            }
        }
        Ok(())
    }
}

/// Backend-agnostic writable filesystem operations over the BinBook directory.
pub trait WritableFilesystem: Filesystem {
    /// Create or truncate `name`.
    fn create_or_truncate(&mut self, name: &str) -> Result<(), Self::Error>;

    /// Write `data` at byte `offset`.
    fn write_at(&mut self, name: &str, offset: u64, data: &[u8]) -> Result<(), Self::Error>;

    /// Flush pending file and directory metadata.
    fn flush(&mut self, name: &str) -> Result<(), Self::Error>;

    /// Delete `name`.
    fn delete_file(&mut self, name: &str) -> Result<(), Self::Error>;

    /// Begin a chunked upload to `name`.
    ///
    /// The default implementation preserves the simple stateless behavior.
    /// Storage backends with expensive mount/open operations should override
    /// the upload methods and keep the underlying file/session open until
    /// commit or abort.
    fn begin_upload(&mut self, name: &str, _expected_size: u64) -> Result<(), Self::Error> {
        self.create_or_truncate(name)
    }

    /// Write one chunk for an upload started by [`begin_upload`](Self::begin_upload).
    fn write_upload_chunk(
        &mut self,
        name: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        self.write_at(name, offset, data)
    }

    /// Commit an upload started by [`begin_upload`](Self::begin_upload).
    fn commit_upload(&mut self, name: &str) -> Result<(), Self::Error> {
        self.flush(name)
    }

    /// Abort an upload started by [`begin_upload`](Self::begin_upload).
    fn abort_upload(&mut self, name: &str) -> Result<(), Self::Error> {
        self.delete_file(name)
    }
}

impl<T: Filesystem> Filesystem for &mut T {
    type Error = T::Error;

    fn for_each_entry(&mut self, visit: &mut dyn FnMut(&str, u64)) -> Result<(), Self::Error> {
        (**self).for_each_entry(visit)
    }

    fn read_at(&mut self, name: &str, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        (**self).read_at(name, offset, out)
    }

    fn file_size(&mut self, name: &str) -> Result<u64, Self::Error> {
        (**self).file_size(name)
    }

    fn for_each_binbook_entry(
        &mut self,
        visit: &mut dyn FnMut(&str, u64, u32),
    ) -> Result<(), Self::Error> {
        (**self).for_each_binbook_entry(visit)
    }
}

impl<T: WritableFilesystem> WritableFilesystem for &mut T {
    fn create_or_truncate(&mut self, name: &str) -> Result<(), Self::Error> {
        (**self).create_or_truncate(name)
    }

    fn write_at(&mut self, name: &str, offset: u64, data: &[u8]) -> Result<(), Self::Error> {
        (**self).write_at(name, offset, data)
    }

    fn flush(&mut self, name: &str) -> Result<(), Self::Error> {
        (**self).flush(name)
    }

    fn delete_file(&mut self, name: &str) -> Result<(), Self::Error> {
        (**self).delete_file(name)
    }

    fn begin_upload(&mut self, name: &str, expected_size: u64) -> Result<(), Self::Error> {
        (**self).begin_upload(name, expected_size)
    }

    fn write_upload_chunk(
        &mut self,
        name: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        (**self).write_upload_chunk(name, offset, data)
    }

    fn commit_upload(&mut self, name: &str) -> Result<(), Self::Error> {
        (**self).commit_upload(name)
    }

    fn abort_upload(&mut self, name: &str) -> Result<(), Self::Error> {
        (**self).abort_upload(name)
    }
}

/// Errors from storage operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError<E> {
    /// The underlying backend returned an error.
    Backend(E),
    /// The named file was not found.
    NotFound,
    /// The file was found but is not a valid BinBook.
    NotBinbook,
    /// An I/O error from binbook-core.
    Io(binbook_core::Error<E>),
}

/// In-memory test filesystem accessible from integration tests.
///
/// Contains a `BTreeMap<String, Vec<u8>>` mapping filenames to their full
/// content. All trait methods delegate to the map. Only compiled in `#[cfg(test)]`.
#[cfg(test)]
pub mod test_helpers {
    extern crate std;

    use super::Filesystem;
    use std::collections::BTreeMap;
    use std::string::{String, ToString};
    use std::vec::Vec;

    #[derive(Default)]
    pub struct MemoryFs {
        pub files: BTreeMap<String, Vec<u8>>,
    }

    impl Filesystem for MemoryFs {
        type Error = ();

        fn for_each_entry(&mut self, visit: &mut dyn FnMut(&str, u64)) -> Result<(), Self::Error> {
            for (name, bytes) in &self.files {
                visit(name, bytes.len() as u64);
            }
            Ok(())
        }

        fn read_at(&mut self, name: &str, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
            let bytes = self.files.get(name).ok_or(())?;
            let start = usize::try_from(offset).map_err(|_| ())?;
            let end = start.checked_add(out.len()).ok_or(())?;
            out.copy_from_slice(bytes.get(start..end).ok_or(())?);
            Ok(())
        }

        fn file_size(&mut self, name: &str) -> Result<u64, Self::Error> {
            Ok(self.files.get(name).map(|b| b.len() as u64).unwrap_or(0))
        }
    }

    impl super::WritableFilesystem for MemoryFs {
        fn create_or_truncate(&mut self, name: &str) -> Result<(), Self::Error> {
            self.files.insert(name.to_string(), Vec::new());
            Ok(())
        }

        fn write_at(&mut self, name: &str, offset: u64, data: &[u8]) -> Result<(), Self::Error> {
            let bytes = self.files.get_mut(name).ok_or(())?;
            let start = usize::try_from(offset).map_err(|_| ())?;
            if bytes.len() < start {
                return Err(());
            }
            let end = start.checked_add(data.len()).ok_or(())?;
            if bytes.len() < end {
                bytes.resize(end, 0);
            }
            bytes[start..end].copy_from_slice(data);
            Ok(())
        }

        fn flush(&mut self, _name: &str) -> Result<(), Self::Error> {
            Ok(())
        }

        fn delete_file(&mut self, name: &str) -> Result<(), Self::Error> {
            self.files.remove(name).map(|_| ()).ok_or(())
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::test_helpers::MemoryFs;
    use super::*;
    use std::string::ToString;
    use std::vec;
    use std::vec::Vec;

    /// Verify the StorageError variants exist and are constructible.
    #[test]
    fn storage_error_variants_exist() {
        let _: StorageError<u32> = StorageError::NotFound;
        let _: StorageError<u32> = StorageError::NotBinbook;
    }

    #[test]
    fn memory_fs_for_each_entry() {
        let mut fs = MemoryFs::default();
        fs.files.insert("a.bin".to_string(), vec![1, 2, 3]);
        fs.files.insert("b.bin".to_string(), vec![4, 5]);

        let mut entries: Vec<(std::string::String, u64)> = Vec::new();
        fs.for_each_entry(&mut |name, size| {
            entries.push((name.to_string(), size));
        })
        .unwrap();

        assert_eq!(entries.len(), 2);
        assert!(entries.contains(&("a.bin".to_string(), 3)));
        assert!(entries.contains(&("b.bin".to_string(), 2)));
    }

    #[test]
    fn memory_fs_read_at() {
        let mut fs = MemoryFs::default();
        fs.files
            .insert("data".to_string(), b"Hello, World!".to_vec());

        let mut buf = [0u8; 5];
        fs.read_at("data", 0, &mut buf).unwrap();
        assert_eq!(&buf, b"Hello");

        fs.read_at("data", 7, &mut buf).unwrap();
        assert_eq!(&buf, b"World");
    }

    #[test]
    fn memory_fs_file_size() {
        let mut fs = MemoryFs::default();
        fs.files.insert("data".to_string(), b"1234567890".to_vec());

        assert_eq!(fs.file_size("data").unwrap(), 10);
        assert_eq!(fs.file_size("missing").unwrap(), 0);
    }

    #[test]
    fn memory_fs_writes_reads_flushes_and_deletes() {
        let mut fs = MemoryFs::default();

        fs.create_or_truncate("book.binbook").unwrap();
        fs.write_at("book.binbook", 0, b"hello").unwrap();
        fs.write_at("book.binbook", 5, b" world").unwrap();
        fs.flush("book.binbook").unwrap();

        let mut out = [0u8; 11];
        fs.read_at("book.binbook", 0, &mut out).unwrap();
        assert_eq!(&out, b"hello world");
        assert_eq!(fs.file_size("book.binbook").unwrap(), 11);

        fs.delete_file("book.binbook").unwrap();
        assert!(fs.read_at("book.binbook", 0, &mut out).is_err());
    }
}
