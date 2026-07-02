use binbook_diagnostic_protocol::StorageBackend;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageError {
    Unavailable,
}

/// Trait for diagnostic storage operations.
///
/// Each method writes encoded response data to the provided buffer and returns
/// the number of bytes written on success. The caller (dispatch handler) uses
/// this to construct `DispatchResult::Response`.
pub trait StorageHandle {
    /// List entries at a path.
    fn store_list(
        &mut self,
        backend: StorageBackend,
        path: &str,
        resp_buf: &mut [u8],
    ) -> Result<usize, StorageError>;

    /// Read a file's content.
    fn store_read(
        &mut self,
        backend: StorageBackend,
        path: &str,
        resp_buf: &mut [u8],
    ) -> Result<usize, StorageError>;

    /// Delete a file.
    fn store_delete(&mut self, backend: StorageBackend, path: &str) -> Result<(), StorageError>;

    /// Begin an upload session. Returns an upload_id on success.
    fn store_upload_begin(
        &mut self,
        backend: StorageBackend,
        path: &str,
        file_size: u32,
        expected_crc32: u32,
    ) -> Result<u32, StorageError>;

    /// Write data to an ongoing upload session.
    fn store_upload_write(
        &mut self,
        upload_id: u32,
        offset: u32,
        data: &[u8],
    ) -> Result<u32, StorageError>;

    /// Commit an upload session.
    fn store_upload_commit(&mut self, upload_id: u32) -> Result<(), StorageError>;

    /// Abort an upload session.
    fn store_upload_abort(&mut self, upload_id: u32) -> Result<(), StorageError>;
}

/// A storage backend that rejects all operations.
///
/// Useful as a default or placeholder when no real storage is available,
/// e.g. during unit tests or before SD card initialisation.
pub struct UnavailableStorage;

impl StorageHandle for UnavailableStorage {
    fn store_list(
        &mut self,
        _backend: StorageBackend,
        _path: &str,
        _resp_buf: &mut [u8],
    ) -> Result<usize, StorageError> {
        Err(StorageError::Unavailable)
    }

    fn store_read(
        &mut self,
        _backend: StorageBackend,
        _path: &str,
        _resp_buf: &mut [u8],
    ) -> Result<usize, StorageError> {
        Err(StorageError::Unavailable)
    }

    fn store_delete(&mut self, _backend: StorageBackend, _path: &str) -> Result<(), StorageError> {
        Err(StorageError::Unavailable)
    }

    fn store_upload_begin(
        &mut self,
        _backend: StorageBackend,
        _path: &str,
        _file_size: u32,
        _expected_crc32: u32,
    ) -> Result<u32, StorageError> {
        Err(StorageError::Unavailable)
    }

    fn store_upload_write(
        &mut self,
        _upload_id: u32,
        _offset: u32,
        _data: &[u8],
    ) -> Result<u32, StorageError> {
        Err(StorageError::Unavailable)
    }

    fn store_upload_commit(&mut self, _upload_id: u32) -> Result<(), StorageError> {
        Err(StorageError::Unavailable)
    }

    fn store_upload_abort(&mut self, _upload_id: u32) -> Result<(), StorageError> {
        Err(StorageError::Unavailable)
    }
}
