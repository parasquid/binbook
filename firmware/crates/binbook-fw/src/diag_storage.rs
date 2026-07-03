use binbook_diagnostic_protocol::{encode_store_entry, StorageBackend};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageError {
    Unavailable,
    BadRequest,
    Unsupported,
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
        resp_buf: &mut [u8],
    ) -> Result<usize, StorageError>;

    /// Read a file's content.
    fn store_read(
        &mut self,
        backend: StorageBackend,
        name: &str,
        offset: u32,
        max_bytes: u32,
        resp_buf: &mut [u8],
    ) -> Result<usize, StorageError>;

    /// Delete a file.
    fn store_delete(&mut self, backend: StorageBackend, name: &str) -> Result<(), StorageError>;

    /// Begin an upload session. Returns an upload_id on success.
    fn store_upload_begin(
        &mut self,
        backend: StorageBackend,
        name: &str,
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
    fn store_upload_commit(&mut self, upload_id: u32) -> Result<(bool, u32), StorageError>;

    /// Abort an upload session.
    fn store_upload_abort(&mut self, upload_id: u32) -> Result<(), StorageError>;
}

#[cfg(feature = "sd-storage")]
pub struct FilesystemStorage<F> {
    fs: F,
    upload: Option<UploadState>,
    next_upload_id: u32,
}

#[cfg(feature = "sd-storage")]
struct UploadState {
    id: u32,
    name: heapless::String<64>,
    expected_size: u32,
    expected_crc32: u32,
    written: u32,
    crc_state: u32,
}

#[cfg(feature = "sd-storage")]
impl<F> FilesystemStorage<F> {
    pub fn new(fs: F) -> Self {
        Self {
            fs,
            upload: None,
            next_upload_id: 1,
        }
    }

    pub fn into_inner(self) -> F {
        self.fs
    }
}

#[cfg(feature = "sd-storage")]
fn crc32_update(bytes: &[u8], mut state: u32) -> u32 {
    for byte in bytes {
        state ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(state & 1);
            state = (state >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    state
}

#[cfg(feature = "sd-storage")]
fn ensure_sd_backend(backend: StorageBackend) -> Result<(), StorageError> {
    match backend {
        StorageBackend::Sd => Ok(()),
        StorageBackend::Flash => Err(StorageError::Unsupported),
    }
}

#[cfg(feature = "sd-storage")]
impl<F> StorageHandle for FilesystemStorage<F>
where
    F: binbook_storage::WritableFilesystem,
{
    fn store_list(
        &mut self,
        backend: StorageBackend,
        resp_buf: &mut [u8],
    ) -> Result<usize, StorageError> {
        ensure_sd_backend(backend)?;
        if resp_buf.len() < 2 {
            return Err(StorageError::BadRequest);
        }
        let mut pos = 2;
        let mut count = 0u16;
        let mut encode_failed = false;
        self.fs
            .for_each_binbook_entry(&mut |name, size, page_count| {
                if encode_failed {
                    return;
                }
                match encode_store_entry(name.as_bytes(), size, page_count, &mut resp_buf[pos..]) {
                    Ok(wrote) => {
                        pos += wrote;
                        count = count.saturating_add(1);
                    }
                    Err(_) => encode_failed = true,
                }
            })
            .map_err(|_| StorageError::Unavailable)?;
        if encode_failed {
            return Err(StorageError::BadRequest);
        }
        resp_buf[0..2].copy_from_slice(&count.to_le_bytes());
        Ok(pos)
    }

    fn store_read(
        &mut self,
        backend: StorageBackend,
        name: &str,
        offset: u32,
        max_bytes: u32,
        resp_buf: &mut [u8],
    ) -> Result<usize, StorageError> {
        ensure_sd_backend(backend)?;
        if resp_buf.len() < 5 {
            return Err(StorageError::BadRequest);
        }
        let size = self
            .fs
            .file_size(name)
            .map_err(|_| StorageError::Unavailable)?;
        let offset_u64 = u64::from(offset);
        if offset_u64 > size {
            return Err(StorageError::BadRequest);
        }
        let remaining = size - offset_u64;
        let capacity = (resp_buf.len() - 5).min(u32::MAX as usize) as u32;
        let to_read = remaining
            .min(u64::from(max_bytes.min(capacity)))
            .min(usize::MAX as u64) as usize;
        self.fs
            .read_at(name, offset_u64, &mut resp_buf[5..5 + to_read])
            .map_err(|_| StorageError::Unavailable)?;
        resp_buf[0..4].copy_from_slice(&(to_read as u32).to_le_bytes());
        resp_buf[4] = u8::from(offset_u64 + to_read as u64 >= size);
        Ok(5 + to_read)
    }

    fn store_delete(&mut self, backend: StorageBackend, name: &str) -> Result<(), StorageError> {
        ensure_sd_backend(backend)?;
        self.fs
            .delete_file(name)
            .map_err(|_| StorageError::Unavailable)
    }

    fn store_upload_begin(
        &mut self,
        backend: StorageBackend,
        name: &str,
        file_size: u32,
        expected_crc32: u32,
    ) -> Result<u32, StorageError> {
        ensure_sd_backend(backend)?;
        if self.upload.is_some() {
            return Err(StorageError::BadRequest);
        }
        self.fs
            .begin_upload(name, u64::from(file_size))
            .map_err(|_| StorageError::Unavailable)?;
        let id = self.next_upload_id;
        self.next_upload_id = self.next_upload_id.wrapping_add(1).max(1);
        self.upload = Some(UploadState {
            id,
            name: heapless::String::try_from(name).map_err(|_| StorageError::BadRequest)?,
            expected_size: file_size,
            expected_crc32,
            written: 0,
            crc_state: 0xFFFF_FFFF,
        });
        Ok(id)
    }

    fn store_upload_write(
        &mut self,
        upload_id: u32,
        offset: u32,
        data: &[u8],
    ) -> Result<u32, StorageError> {
        let upload = self.upload.as_mut().ok_or(StorageError::BadRequest)?;
        if upload.id != upload_id || offset != upload.written {
            return Err(StorageError::BadRequest);
        }
        self.fs
            .write_upload_chunk(upload.name.as_str(), u64::from(offset), data)
            .map_err(|_| StorageError::Unavailable)?;
        upload.crc_state = crc32_update(data, upload.crc_state);
        upload.written = offset.saturating_add(data.len() as u32);
        Ok(data.len() as u32)
    }

    fn store_upload_commit(&mut self, upload_id: u32) -> Result<(bool, u32), StorageError> {
        let upload = self.upload.take().ok_or(StorageError::BadRequest)?;
        if upload.id != upload_id || upload.written != upload.expected_size {
            self.upload = Some(upload);
            return Err(StorageError::BadRequest);
        }
        self.fs
            .commit_upload(upload.name.as_str())
            .map_err(|_| StorageError::Unavailable)?;
        let crc = upload.crc_state ^ 0xFFFF_FFFF;
        if crc != upload.expected_crc32 {
            return Ok((false, crc));
        }
        Ok((true, crc))
    }

    fn store_upload_abort(&mut self, upload_id: u32) -> Result<(), StorageError> {
        let upload = self.upload.take().ok_or(StorageError::BadRequest)?;
        if upload.id != upload_id {
            self.upload = Some(upload);
            return Err(StorageError::BadRequest);
        }
        self.fs
            .abort_upload(upload.name.as_str())
            .map_err(|_| StorageError::Unavailable)
    }
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
        _resp_buf: &mut [u8],
    ) -> Result<usize, StorageError> {
        Err(StorageError::Unavailable)
    }

    fn store_read(
        &mut self,
        _backend: StorageBackend,
        _name: &str,
        _offset: u32,
        _max_bytes: u32,
        _resp_buf: &mut [u8],
    ) -> Result<usize, StorageError> {
        Err(StorageError::Unavailable)
    }

    fn store_delete(&mut self, _backend: StorageBackend, _name: &str) -> Result<(), StorageError> {
        Err(StorageError::Unavailable)
    }

    fn store_upload_begin(
        &mut self,
        _backend: StorageBackend,
        _name: &str,
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

    fn store_upload_commit(&mut self, _upload_id: u32) -> Result<(bool, u32), StorageError> {
        Err(StorageError::Unavailable)
    }

    fn store_upload_abort(&mut self, _upload_id: u32) -> Result<(), StorageError> {
        Err(StorageError::Unavailable)
    }
}
