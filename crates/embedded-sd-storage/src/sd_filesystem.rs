//! Generic `SdStorage` — wraps an SD block device and exposes a flat `/BOOKS`
//! FAT directory through backend-agnostic operations.

use core::{cmp::min, marker::PhantomData};

use embedded_hal::delay::DelayNs;
use embedded_hal::spi::SpiDevice;
use embedded_sdmmc::filesystem::TimeSource;
use embedded_sdmmc::{Block, BlockDevice, BlockIdx, SdCard};
use heapless::String;
use lamfat::{FileSystem, FsOptions, Read, Seek, SeekFrom, Write};

use crate::block_stream::{BlockDeviceStream, BorrowedBlockDevice};
pub use crate::fat_directory::StorageError;
use crate::fat_directory::{FatDirectory, BOOKS_DIR, NAME_BUF_LEN, TMP_DIR};

const MAX_FILE_EXTENTS: usize = 128;
const ZERO_CHUNK: [u8; 512] = [0; 512];

/// Generic SD + FAT handle.
///
/// `D` is a `BlockDevice` — either an `SdCard<SPI, DELAY>` (hardware) or a
/// mock for host testing. `TIME` is retained for constructor compatibility with
/// the previous `embedded-sdmmc` FAT manager path.
pub struct SdStorage<D, TIME>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
    TIME: TimeSource,
{
    block_device: D,
    active_upload: Option<ActiveUpload>,
    active_read: Option<ActiveRead>,
    _time: PhantomData<TIME>,
}

struct ActiveUpload {
    dir: String<NAME_BUF_LEN>,
    name: String<NAME_BUF_LEN>,
    expected_size: u64,
    base_offset: u64,
    extents: heapless::Vec<lamfat::Extent, MAX_FILE_EXTENTS>,
}

struct ActiveRead {
    dir: String<NAME_BUF_LEN>,
    name: String<NAME_BUF_LEN>,
    file_size: u64,
    base_offset: u64,
    extents: heapless::Vec<lamfat::Extent, MAX_FILE_EXTENTS>,
}

// --- Hardware path: construct from SPI device + delay ---

impl<SPI, DELAY, TIME> SdStorage<SdCard<SPI, DELAY>, TIME>
where
    SPI: SpiDevice<u8>,
    DELAY: DelayNs,
    <SdCard<SPI, DELAY> as BlockDevice>::Error: core::fmt::Debug,
    TIME: TimeSource,
{
    /// Construct from an SPI device + delay (real hardware path).
    pub fn new(spi: SPI, delay: DELAY, _time: TIME) -> Self {
        let block_device = SdCard::new(spi, delay);
        Self {
            block_device,
            active_upload: None,
            active_read: None,
            _time: PhantomData,
        }
    }
}

// --- Host-test path: construct directly over a BlockDevice ---

impl<D, TIME> SdStorage<D, TIME>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
    TIME: TimeSource,
{
    /// Construct from a raw `BlockDevice` (host-test path, no real SPI/delay).
    pub fn from_block_device(block_device: D, _time: TIME) -> Self {
        Self {
            block_device,
            active_upload: None,
            active_read: None,
            _time: PhantomData,
        }
    }

    /// Enumerate files in `/BOOKS`, calling `visit` for each.
    pub fn for_each_entry(
        &mut self,
        visit: &mut dyn FnMut(&str, u64),
    ) -> Result<(), StorageError<D>> {
        FatDirectory::for_each_entry(&mut self.block_device, visit)
    }

    /// Enumerate valid `.binbook` files in `/BOOKS`, including page counts.
    pub fn for_each_binbook_entry(
        &mut self,
        visit: &mut dyn FnMut(&str, u64, u32),
    ) -> Result<(), StorageError<D>> {
        FatDirectory::for_each_binbook_entry(&mut self.block_device, visit)
    }

    /// Read `out.len()` bytes at `offset` from file `name`.
    pub fn read_at(
        &mut self,
        name: &str,
        offset: u64,
        out: &mut [u8],
    ) -> Result<(), StorageError<D>> {
        self.read_at_in_dir(BOOKS_DIR, name, offset, out)
    }

    pub fn read_at_in_dir(
        &mut self,
        dir_name: &str,
        name: &str,
        offset: u64,
        out: &mut [u8],
    ) -> Result<(), StorageError<D>> {
        self.ensure_read_cache(dir_name, name)?;
        let read = self.active_read.as_ref().ok_or(StorageError::NotFound)?;
        let available = read.file_size.saturating_sub(offset);
        let read_len = min(out.len(), available as usize);
        if read_len > 0 {
            read_extents(
                &mut self.block_device,
                read.base_offset,
                &read.extents,
                offset,
                &mut out[..read_len],
            )?;
        }
        for byte in out.iter_mut().skip(read_len) {
            *byte = 0;
        }
        Ok(())
    }

    /// Return the byte length of `name`, or `StorageError::NotFound`.
    pub fn file_size(&mut self, name: &str) -> Result<u64, StorageError<D>> {
        self.file_size_in_dir(BOOKS_DIR, name)
    }

    pub fn file_size_in_dir(&mut self, dir_name: &str, name: &str) -> Result<u64, StorageError<D>> {
        if let Some(read) = self.active_read.as_ref() {
            if read.dir.as_str() == dir_name && read.name.as_str() == name {
                return Ok(read.file_size);
            }
        }
        if dir_name == BOOKS_DIR {
            FatDirectory::file_size(&mut self.block_device, name)
        } else {
            FatDirectory::file_size_in_dir(&mut self.block_device, dir_name, name)
        }
    }

    /// Create `/BOOKS` if needed, then create or truncate `name`.
    pub fn create_or_truncate(&mut self, name: &str) -> Result<(), StorageError<D>> {
        self.create_or_truncate_in_dir(BOOKS_DIR, name)
    }

    pub fn create_or_truncate_in_dir(
        &mut self,
        dir_name: &str,
        name: &str,
    ) -> Result<(), StorageError<D>> {
        self.active_read = None;
        if dir_name == BOOKS_DIR {
            FatDirectory::create_or_truncate(&mut self.block_device, name)
        } else {
            FatDirectory::create_or_truncate_in_dir(&mut self.block_device, dir_name, name)
        }
    }

    /// Write bytes at `offset` in `/BOOKS/name`.
    pub fn write_at(
        &mut self,
        name: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), StorageError<D>> {
        self.write_at_in_dir(BOOKS_DIR, name, offset, data)
    }

    pub fn write_at_in_dir(
        &mut self,
        dir_name: &str,
        name: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), StorageError<D>> {
        self.active_read = None;
        if dir_name == BOOKS_DIR {
            FatDirectory::write_at(&mut self.block_device, name, offset, data)
        } else {
            FatDirectory::write_at_in_dir(&mut self.block_device, dir_name, name, offset, data)
        }
    }

    /// Flush `/BOOKS/name`.
    pub fn flush(&mut self, name: &str) -> Result<(), StorageError<D>> {
        self.flush_in_dir(BOOKS_DIR, name)
    }

    pub fn flush_in_dir(&mut self, dir_name: &str, name: &str) -> Result<(), StorageError<D>> {
        if dir_name == BOOKS_DIR {
            FatDirectory::flush(&mut self.block_device, name)
        } else {
            FatDirectory::flush_in_dir(&mut self.block_device, dir_name, name)
        }
    }

    /// Delete `/BOOKS/name`.
    pub fn delete_file(&mut self, name: &str) -> Result<(), StorageError<D>> {
        self.delete_file_in_dir(BOOKS_DIR, name)
    }

    pub fn delete_file_in_dir(
        &mut self,
        dir_name: &str,
        name: &str,
    ) -> Result<(), StorageError<D>> {
        self.active_read = None;
        if dir_name == BOOKS_DIR {
            FatDirectory::delete_file(&mut self.block_device, name)
        } else {
            FatDirectory::delete_file_in_dir(&mut self.block_device, dir_name, name)
        }
    }

    /// Begin a chunked upload and keep one FAT filesystem mounted for the
    /// duration of the upload.
    pub fn begin_upload(&mut self, name: &str, expected_size: u64) -> Result<(), StorageError<D>> {
        self.begin_upload_in_dir(BOOKS_DIR, name, expected_size)
    }

    pub fn begin_upload_in_dir(
        &mut self,
        dir_name: &str,
        name: &str,
        expected_size: u64,
    ) -> Result<(), StorageError<D>> {
        if self.active_upload.is_some() {
            return Err(StorageError::UploadInProgress);
        }
        self.active_read = None;
        if expected_size > u64::from(u32::MAX) {
            return Err(StorageError::BadUploadOffset);
        }
        let base_offset = crate::fat_directory::filesystem_base_offset(&mut self.block_device)?;
        let stream = BlockDeviceStream::new(
            BorrowedBlockDevice::new(&mut self.block_device),
            base_offset,
        );
        let fs = FileSystem::new(stream, FsOptions::new()).map_err(StorageError::Fat)?;
        let mut extents = heapless::Vec::new();
        {
            let root = fs.root_dir();
            let dir = match root.open_dir(dir_name) {
                Ok(dir) => dir,
                Err(lamfat::Error::NotFound) => {
                    root.create_dir(dir_name).map_err(StorageError::Fat)?
                }
                Err(error) => return Err(StorageError::Fat(error)),
            };
            let mut file = dir.create_file(name).map_err(StorageError::Fat)?;
            file.seek(SeekFrom::Start(0)).map_err(StorageError::Fat)?;
            file.truncate().map_err(StorageError::Fat)?;
            let mut remaining = expected_size;
            while remaining > 0 {
                let len = min(remaining as usize, ZERO_CHUNK.len());
                file.write_all(&ZERO_CHUNK[..len])
                    .map_err(StorageError::Fat)?;
                remaining -= len as u64;
            }
            file.flush().map_err(StorageError::Fat)?;
            for extent in file.extents() {
                extents
                    .push(extent.map_err(StorageError::Fat)?)
                    .map_err(|_| StorageError::UploadTooFragmented)?;
            }
        }
        fs.unmount().map_err(StorageError::Fat)?;
        self.active_upload = Some(ActiveUpload {
            dir: String::try_from(dir_name).map_err(|_| StorageError::NameTooLong)?,
            name: String::try_from(name).map_err(|_| StorageError::NameTooLong)?,
            expected_size,
            base_offset,
            extents,
        });
        Ok(())
    }

    /// Write a chunk to the active upload.
    pub fn write_upload_chunk(
        &mut self,
        name: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), StorageError<D>> {
        self.write_upload_chunk_in_dir(BOOKS_DIR, name, offset, data)
    }

    pub fn write_upload_chunk_in_dir(
        &mut self,
        dir_name: &str,
        name: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), StorageError<D>> {
        let upload = self
            .active_upload
            .as_mut()
            .ok_or(StorageError::NoUploadInProgress)?;
        if upload.dir.as_str() != dir_name || upload.name.as_str() != name {
            return Err(StorageError::UploadNameMismatch);
        }
        let end = offset
            .checked_add(data.len() as u64)
            .ok_or(StorageError::BadUploadOffset)?;
        if end > upload.expected_size {
            return Err(StorageError::BadUploadOffset);
        }
        write_extents(
            &mut self.block_device,
            upload.base_offset,
            &upload.extents,
            offset,
            data,
        )
    }

    /// Commit the active upload.
    pub fn commit_upload(&mut self, name: &str) -> Result<(), StorageError<D>> {
        self.commit_upload_in_dir(BOOKS_DIR, name)
    }

    pub fn commit_upload_in_dir(
        &mut self,
        dir_name: &str,
        name: &str,
    ) -> Result<(), StorageError<D>> {
        let upload = self
            .active_upload
            .take()
            .ok_or(StorageError::NoUploadInProgress)?;
        if upload.dir.as_str() != dir_name || upload.name.as_str() != name {
            self.active_upload = Some(upload);
            return Err(StorageError::UploadNameMismatch);
        }
        self.active_read = Some(ActiveRead {
            dir: upload.dir,
            name: upload.name,
            file_size: upload.expected_size,
            base_offset: upload.base_offset,
            extents: upload.extents,
        });
        Ok(())
    }

    /// Abort the active upload and remove the partially written file.
    pub fn abort_upload(&mut self, name: &str) -> Result<(), StorageError<D>> {
        self.abort_upload_in_dir(BOOKS_DIR, name)
    }

    pub fn abort_upload_in_dir(
        &mut self,
        dir_name: &str,
        name: &str,
    ) -> Result<(), StorageError<D>> {
        let upload = self
            .active_upload
            .take()
            .ok_or(StorageError::NoUploadInProgress)?;
        if upload.dir.as_str() != dir_name || upload.name.as_str() != name {
            self.active_upload = Some(upload);
            return Err(StorageError::UploadNameMismatch);
        }
        self.active_read = None;
        FatDirectory::delete_file_in_dir(&mut self.block_device, dir_name, name)
    }

    pub fn tmp_read_at(
        &mut self,
        name: &str,
        offset: u64,
        out: &mut [u8],
    ) -> Result<(), StorageError<D>> {
        self.read_at_in_dir(TMP_DIR, name, offset, out)
    }

    pub fn tmp_file_size(&mut self, name: &str) -> Result<u64, StorageError<D>> {
        self.file_size_in_dir(TMP_DIR, name)
    }

    pub fn tmp_create_or_truncate(&mut self, name: &str) -> Result<(), StorageError<D>> {
        self.create_or_truncate_in_dir(TMP_DIR, name)
    }

    pub fn tmp_write_at(
        &mut self,
        name: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), StorageError<D>> {
        self.write_at_in_dir(TMP_DIR, name, offset, data)
    }

    pub fn tmp_flush(&mut self, name: &str) -> Result<(), StorageError<D>> {
        self.flush_in_dir(TMP_DIR, name)
    }

    pub fn tmp_begin_upload(
        &mut self,
        name: &str,
        expected_size: u64,
    ) -> Result<(), StorageError<D>> {
        self.begin_upload_in_dir(TMP_DIR, name, expected_size)
    }

    pub fn tmp_write_upload_chunk(
        &mut self,
        name: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), StorageError<D>> {
        self.write_upload_chunk_in_dir(TMP_DIR, name, offset, data)
    }

    pub fn tmp_commit_upload(&mut self, name: &str) -> Result<(), StorageError<D>> {
        self.commit_upload_in_dir(TMP_DIR, name)
    }

    pub fn tmp_delete_file(&mut self, name: &str) -> Result<(), StorageError<D>> {
        self.delete_file_in_dir(TMP_DIR, name)
    }

    pub fn copy_tmp_to_books(
        &mut self,
        source_name: &str,
        destination_name: &str,
        scratch: &mut [u8],
    ) -> Result<u64, StorageError<D>> {
        if scratch.is_empty() {
            return Err(StorageError::BadUploadOffset);
        }
        self.active_read = None;
        self.active_upload = None;
        let base_offset = crate::fat_directory::filesystem_base_offset(&mut self.block_device)?;
        let stream = BlockDeviceStream::new(
            BorrowedBlockDevice::new(&mut self.block_device),
            base_offset,
        );
        let fs = FileSystem::new(stream, FsOptions::new()).map_err(StorageError::Fat)?;
        let mut copied = 0u64;
        {
            let root = fs.root_dir();
            let source_dir = root.open_dir(TMP_DIR).map_err(StorageError::Fat)?;
            let destination_dir = match root.open_dir(BOOKS_DIR) {
                Ok(dir) => dir,
                Err(lamfat::Error::NotFound) => {
                    root.create_dir(BOOKS_DIR).map_err(StorageError::Fat)?
                }
                Err(error) => return Err(StorageError::Fat(error)),
            };
            let mut source = source_dir
                .open_file(source_name)
                .map_err(StorageError::Fat)?;
            let mut destination = destination_dir
                .create_file(destination_name)
                .map_err(StorageError::Fat)?;
            destination.truncate().map_err(StorageError::Fat)?;
            loop {
                let read = source.read(scratch).map_err(StorageError::Fat)?;
                if read == 0 {
                    break;
                }
                destination
                    .write_all(&scratch[..read])
                    .map_err(StorageError::Fat)?;
                copied += read as u64;
            }
            destination.flush().map_err(StorageError::Fat)?;
        }
        fs.unmount().map_err(StorageError::Fat)?;
        Ok(copied)
    }

    fn ensure_read_cache(&mut self, dir_name: &str, name: &str) -> Result<(), StorageError<D>> {
        if let Some(read) = self.active_read.as_ref() {
            if read.dir.as_str() == dir_name && read.name.as_str() == name {
                return Ok(());
            }
        }

        let base_offset = crate::fat_directory::filesystem_base_offset(&mut self.block_device)?;
        let stream = BlockDeviceStream::new(
            BorrowedBlockDevice::new(&mut self.block_device),
            base_offset,
        );
        let fs = FileSystem::new(stream, FsOptions::new()).map_err(StorageError::Fat)?;
        let mut extents = heapless::Vec::new();
        let mut file_size = 0u64;
        {
            let root = fs.root_dir();
            let dir = root.open_dir(dir_name).map_err(map_not_found::<D>)?;
            let mut file = dir.open_file(name).map_err(map_not_found::<D>)?;
            for extent in file.extents() {
                let extent = extent.map_err(StorageError::Fat)?;
                file_size += u64::from(extent.size);
                extents
                    .push(extent)
                    .map_err(|_| StorageError::FileTooFragmented)?;
            }
        }
        fs.unmount().map_err(StorageError::Fat)?;
        self.active_read = Some(ActiveRead {
            dir: String::try_from(dir_name).map_err(|_| StorageError::NameTooLong)?,
            name: String::try_from(name).map_err(|_| StorageError::NameTooLong)?,
            file_size,
            base_offset,
            extents,
        });
        Ok(())
    }
}

fn map_not_found<D>(
    error: lamfat::Error<crate::block_stream::StreamError<D::Error>>,
) -> StorageError<D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    match error {
        lamfat::Error::NotFound => StorageError::NotFound,
        other => StorageError::Fat(other),
    }
}

fn write_extents<D>(
    device: &mut D,
    base_offset: u64,
    extents: &[lamfat::Extent],
    mut offset: u64,
    mut data: &[u8],
) -> Result<(), StorageError<D>>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    let mut file_cursor = 0u64;
    for extent in extents {
        let extent_len = u64::from(extent.size);
        let extent_end = file_cursor + extent_len;
        if offset >= extent_end {
            file_cursor = extent_end;
            continue;
        }
        let local = offset.saturating_sub(file_cursor);
        let writable = min(data.len(), (extent_len - local) as usize);
        write_absolute(
            device,
            base_offset + extent.offset + local,
            &data[..writable],
        )?;
        offset += writable as u64;
        data = &data[writable..];
        if data.is_empty() {
            return Ok(());
        }
        file_cursor = extent_end;
    }
    Err(StorageError::BadUploadOffset)
}

fn write_absolute<D>(
    device: &mut D,
    mut absolute: u64,
    mut data: &[u8],
) -> Result<(), StorageError<D>>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    let mut scratch = Block::new();
    while !data.is_empty() {
        let block_idx = BlockIdx((absolute / 512) as u32);
        let block_off = (absolute % 512) as usize;
        device
            .read(core::slice::from_mut(&mut scratch), block_idx)
            .map_err(StorageError::Device)?;
        let len = min(data.len(), 512 - block_off);
        scratch.contents[block_off..block_off + len].copy_from_slice(&data[..len]);
        device
            .write(core::slice::from_ref(&scratch), block_idx)
            .map_err(StorageError::Device)?;
        absolute += len as u64;
        data = &data[len..];
    }
    Ok(())
}

fn read_extents<D>(
    device: &mut D,
    base_offset: u64,
    extents: &[lamfat::Extent],
    mut offset: u64,
    mut out: &mut [u8],
) -> Result<(), StorageError<D>>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    let mut file_cursor = 0u64;
    for extent in extents {
        let extent_len = u64::from(extent.size);
        let extent_end = file_cursor + extent_len;
        if offset >= extent_end {
            file_cursor = extent_end;
            continue;
        }
        let local = offset.saturating_sub(file_cursor);
        let readable = min(out.len(), (extent_len - local) as usize);
        read_absolute(
            device,
            base_offset + extent.offset + local,
            &mut out[..readable],
        )?;
        offset += readable as u64;
        out = &mut out[readable..];
        if out.is_empty() {
            return Ok(());
        }
        file_cursor = extent_end;
    }
    Err(StorageError::BadUploadOffset)
}

fn read_absolute<D>(
    device: &mut D,
    mut absolute: u64,
    mut out: &mut [u8],
) -> Result<(), StorageError<D>>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    let mut scratch = Block::new();
    while !out.is_empty() {
        let block_idx = BlockIdx((absolute / 512) as u32);
        let block_off = (absolute % 512) as usize;
        device
            .read(core::slice::from_mut(&mut scratch), block_idx)
            .map_err(StorageError::Device)?;
        let len = min(out.len(), 512 - block_off);
        out[..len].copy_from_slice(&scratch.contents[block_off..block_off + len]);
        absolute += len as u64;
        out = &mut out[len..];
    }
    Ok(())
}
