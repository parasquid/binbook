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
    fn for_each_entry(
        &mut self,
        visit: &mut dyn FnMut(&str, u64),
    ) -> Result<(), Self::Error>;

    /// Open `name` for reading and read `out.len()` bytes at byte `offset`.
    /// Returns `Err(NotFound)` if the file is absent.
    fn read_at(
        &mut self,
        name: &str,
        offset: u64,
        out: &mut [u8],
    ) -> Result<(), Self::Error>;

    /// Total byte length of `name`, or an error if absent.
    fn file_size(&mut self, name: &str) -> Result<u64, Self::Error>;
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
    use std::string::String;
    use std::vec::Vec;

    #[derive(Default)]
    pub struct MemoryFs {
        pub files: BTreeMap<String, Vec<u8>>,
    }

    impl Filesystem for MemoryFs {
        type Error = ();

        fn for_each_entry(
            &mut self,
            visit: &mut dyn FnMut(&str, u64),
        ) -> Result<(), Self::Error> {
            for (name, bytes) in &self.files {
                visit(name, bytes.len() as u64);
            }
            Ok(())
        }

        fn read_at(
            &mut self,
            name: &str,
            offset: u64,
            out: &mut [u8],
        ) -> Result<(), Self::Error> {
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
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use super::test_helpers::MemoryFs;
    use std::string::ToString;
    use std::vec::Vec;
    use std::vec;

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
        fs.files.insert("data".to_string(), b"Hello, World!".to_vec());

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
}
