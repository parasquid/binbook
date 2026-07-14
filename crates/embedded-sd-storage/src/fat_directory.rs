use embedded_sdmmc::{Block, BlockDevice, BlockIdx};
use heapless::String;
use lamfat::{
    File, FileSystem, FsOptions, Read, ReadWriteSeek, Seek, SeekFrom, TimeProvider, Write,
};

use crate::block_stream::{BlockDeviceStream, BorrowedBlockDevice, StreamError};

pub const BOOKS_DIR: &str = "BOOKS";
pub const TMP_DIR: &str = "TMP";
pub(crate) const NAME_BUF_LEN: usize = 256;

/// Errors from SD FAT directory operations.
#[derive(Debug)]
pub enum StorageError<D: BlockDevice>
where
    D::Error: core::fmt::Debug,
{
    Device(D::Error),
    Fat(lamfat::Error<StreamError<D::Error>>),
    NotFound,
    NameTooLong,
    UploadInProgress,
    NoUploadInProgress,
    UploadNameMismatch,
    UploadTooFragmented,
    FileTooFragmented,
    BadUploadOffset,
    UnexpectedEof,
}

pub struct FatDirectory;

impl FatDirectory {
    pub fn for_each_binbook_entry<D>(
        device: &mut D,
        visit: &mut dyn FnMut(&str, u64, u32),
    ) -> Result<(), StorageError<D>>
    where
        D: BlockDevice,
        D::Error: core::fmt::Debug,
    {
        let base_offset = filesystem_base_offset(device)?;
        let stream = BlockDeviceStream::new(BorrowedBlockDevice::new(device), base_offset);
        let fs = FileSystem::new(stream, FsOptions::new()).map_err(StorageError::Fat)?;
        let mut names =
            heapless::Vec::<(String<{ binbook_storage::MAX_BINBOOK_NAME_BYTES }>, u64), 64>::new();
        let mut missing_books_dir = false;
        {
            let root = fs.root_dir();
            if let Some(dir) = match root.open_dir(BOOKS_DIR) {
                Ok(dir) => Some(dir),
                Err(lamfat::Error::NotFound) => {
                    missing_books_dir = true;
                    None
                }
                Err(error) => return Err(StorageError::Fat(error)),
            } {
                for entry in dir.iter() {
                    let entry = entry.map_err(StorageError::Fat)?;
                    if !entry.is_file() {
                        continue;
                    }
                    let name = entry_name::<D, _, _, _>(&entry)?;
                    if !name.ends_with(".binbook") {
                        continue;
                    }
                    if let Ok(name) =
                        String::<{ binbook_storage::MAX_BINBOOK_NAME_BYTES }>::try_from(
                            name.as_str(),
                        )
                    {
                        let _ = names.push((name, entry.len()));
                    }
                }
            }
        }
        if !missing_books_dir {
            let root = fs.root_dir();
            let dir = root.open_dir(BOOKS_DIR).map_err(map_not_found::<D>)?;
            let mut scratch = [0u8; 1024];
            for (name, size) in names {
                let Ok(file) = dir.open_file(name.as_str()) else {
                    continue;
                };
                if let Ok(page_count) = read_binbook_page_count(file, size, &mut scratch) {
                    visit(name.as_str(), size, page_count);
                }
            }
        }
        fs.unmount().map_err(StorageError::Fat)
    }

    pub fn for_each_entry<D>(
        device: &mut D,
        visit: &mut dyn FnMut(&str, u64),
    ) -> Result<(), StorageError<D>>
    where
        D: BlockDevice,
        D::Error: core::fmt::Debug,
    {
        let base_offset = filesystem_base_offset(device)?;
        let stream = BlockDeviceStream::new(BorrowedBlockDevice::new(device), base_offset);
        let fs = FileSystem::new(stream, FsOptions::new()).map_err(StorageError::Fat)?;
        let mut missing_books_dir = false;
        {
            let root = fs.root_dir();
            if let Some(dir) = match root.open_dir(BOOKS_DIR) {
                Ok(dir) => Some(dir),
                Err(lamfat::Error::NotFound) => {
                    missing_books_dir = true;
                    None
                }
                Err(error) => return Err(StorageError::Fat(error)),
            } {
                for entry in dir.iter() {
                    let entry = entry.map_err(StorageError::Fat)?;
                    if entry.is_file() {
                        let name = entry_name::<D, _, _, _>(&entry)?;
                        visit(name.as_str(), entry.len());
                    }
                }
            }
        }
        if missing_books_dir {
            return fs.unmount().map_err(StorageError::Fat);
        }
        fs.unmount().map_err(StorageError::Fat)
    }

    #[allow(dead_code)]
    pub fn read_at<D>(
        device: &mut D,
        name: &str,
        offset: u64,
        out: &mut [u8],
    ) -> Result<(), StorageError<D>>
    where
        D: BlockDevice,
        D::Error: core::fmt::Debug,
    {
        let base_offset = filesystem_base_offset(device)?;
        let stream = BlockDeviceStream::new(BorrowedBlockDevice::new(device), base_offset);
        let fs = FileSystem::new(stream, FsOptions::new()).map_err(StorageError::Fat)?;
        {
            let root = fs.root_dir();
            let dir = root.open_dir(BOOKS_DIR).map_err(map_not_found::<D>)?;
            let mut file = dir.open_file(name).map_err(map_not_found::<D>)?;
            file.seek(SeekFrom::Start(offset))
                .map_err(StorageError::Fat)?;

            let mut filled = 0;
            while filled < out.len() {
                let read = file.read(&mut out[filled..]).map_err(StorageError::Fat)?;
                if read == 0 {
                    break;
                }
                filled += read;
            }
            if filled != out.len() {
                return Err(StorageError::UnexpectedEof);
            }
        }
        fs.unmount().map_err(StorageError::Fat)
    }

    pub fn file_size<D>(device: &mut D, name: &str) -> Result<u64, StorageError<D>>
    where
        D: BlockDevice,
        D::Error: core::fmt::Debug,
    {
        Self::file_size_in_dir(device, BOOKS_DIR, name)
    }

    pub fn file_size_in_dir<D>(
        device: &mut D,
        dir_name: &str,
        name: &str,
    ) -> Result<u64, StorageError<D>>
    where
        D: BlockDevice,
        D::Error: core::fmt::Debug,
    {
        let base_offset = filesystem_base_offset(device)?;
        let stream = BlockDeviceStream::new(BorrowedBlockDevice::new(device), base_offset);
        let fs = FileSystem::new(stream, FsOptions::new()).map_err(StorageError::Fat)?;
        let mut found = None;
        {
            let root = fs.root_dir();
            let dir = root.open_dir(dir_name).map_err(map_not_found::<D>)?;
            for entry in dir.iter() {
                let entry = entry.map_err(StorageError::Fat)?;
                if entry.is_file() {
                    let entry_name = entry_name::<D, _, _, _>(&entry)?;
                    if entry_name.as_str() == name {
                        found = Some(entry.len());
                        break;
                    }
                }
            }
        }
        fs.unmount().map_err(StorageError::Fat)?;
        found.ok_or(StorageError::NotFound)
    }

    pub fn create_or_truncate<D>(device: &mut D, name: &str) -> Result<(), StorageError<D>>
    where
        D: BlockDevice,
        D::Error: core::fmt::Debug,
    {
        Self::create_or_truncate_in_dir(device, BOOKS_DIR, name)
    }

    pub fn create_or_truncate_in_dir<D>(
        device: &mut D,
        dir_name: &str,
        name: &str,
    ) -> Result<(), StorageError<D>>
    where
        D: BlockDevice,
        D::Error: core::fmt::Debug,
    {
        let base_offset = filesystem_base_offset(device)?;
        let stream = BlockDeviceStream::new(BorrowedBlockDevice::new(device), base_offset);
        let fs = FileSystem::new(stream, FsOptions::new()).map_err(StorageError::Fat)?;
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
            file.flush().map_err(StorageError::Fat)?;
        }
        fs.unmount().map_err(StorageError::Fat)
    }

    pub fn write_at<D>(
        device: &mut D,
        name: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), StorageError<D>>
    where
        D: BlockDevice,
        D::Error: core::fmt::Debug,
    {
        Self::write_at_in_dir(device, BOOKS_DIR, name, offset, data)
    }

    pub fn write_at_in_dir<D>(
        device: &mut D,
        dir_name: &str,
        name: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), StorageError<D>>
    where
        D: BlockDevice,
        D::Error: core::fmt::Debug,
    {
        let base_offset = filesystem_base_offset(device)?;
        let stream = BlockDeviceStream::new(BorrowedBlockDevice::new(device), base_offset);
        let fs = FileSystem::new(stream, FsOptions::new()).map_err(StorageError::Fat)?;
        {
            let root = fs.root_dir();
            let dir = root.open_dir(dir_name).map_err(map_not_found::<D>)?;
            let mut file = dir.create_file(name).map_err(StorageError::Fat)?;
            file.seek(SeekFrom::Start(offset))
                .map_err(StorageError::Fat)?;
            file.write_all(data).map_err(StorageError::Fat)?;
            file.flush().map_err(StorageError::Fat)?;
        }
        fs.unmount().map_err(StorageError::Fat)
    }

    pub fn flush<D>(device: &mut D, name: &str) -> Result<(), StorageError<D>>
    where
        D: BlockDevice,
        D::Error: core::fmt::Debug,
    {
        Self::flush_in_dir(device, BOOKS_DIR, name)
    }

    pub fn flush_in_dir<D>(
        device: &mut D,
        dir_name: &str,
        name: &str,
    ) -> Result<(), StorageError<D>>
    where
        D: BlockDevice,
        D::Error: core::fmt::Debug,
    {
        let base_offset = filesystem_base_offset(device)?;
        let stream = BlockDeviceStream::new(BorrowedBlockDevice::new(device), base_offset);
        let fs = FileSystem::new(stream, FsOptions::new()).map_err(StorageError::Fat)?;
        {
            let root = fs.root_dir();
            let dir = root.open_dir(dir_name).map_err(map_not_found::<D>)?;
            let mut file = dir.open_file(name).map_err(map_not_found::<D>)?;
            file.flush().map_err(StorageError::Fat)?;
        }
        fs.unmount().map_err(StorageError::Fat)
    }

    pub fn delete_file<D>(device: &mut D, name: &str) -> Result<(), StorageError<D>>
    where
        D: BlockDevice,
        D::Error: core::fmt::Debug,
    {
        Self::delete_file_in_dir(device, BOOKS_DIR, name)
    }

    pub fn delete_file_in_dir<D>(
        device: &mut D,
        dir_name: &str,
        name: &str,
    ) -> Result<(), StorageError<D>>
    where
        D: BlockDevice,
        D::Error: core::fmt::Debug,
    {
        let base_offset = filesystem_base_offset(device)?;
        let stream = BlockDeviceStream::new(BorrowedBlockDevice::new(device), base_offset);
        let fs = FileSystem::new(stream, FsOptions::new()).map_err(StorageError::Fat)?;
        {
            let root = fs.root_dir();
            let dir = root.open_dir(dir_name).map_err(map_not_found::<D>)?;
            dir.remove(name).map_err(map_not_found::<D>)?;
        }
        fs.unmount().map_err(StorageError::Fat)
    }
}

fn map_not_found<D>(error: lamfat::Error<StreamError<D::Error>>) -> StorageError<D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    match error {
        lamfat::Error::NotFound => StorageError::NotFound,
        other => StorageError::Fat(other),
    }
}

pub(crate) fn filesystem_base_offset<D>(device: &mut D) -> Result<u64, StorageError<D>>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    let mut block = Block::new();
    device
        .read(core::slice::from_mut(&mut block), BlockIdx(0))
        .map_err(StorageError::Device)?;
    let bytes = &block.contents;
    if bytes[510] == 0x55 && bytes[511] == 0xAA {
        let partition_type = bytes[0x1BE + 4];
        let lba = u32::from_le_bytes([
            bytes[0x1BE + 8],
            bytes[0x1BE + 9],
            bytes[0x1BE + 10],
            bytes[0x1BE + 11],
        ]);
        let sectors = u32::from_le_bytes([
            bytes[0x1BE + 12],
            bytes[0x1BE + 13],
            bytes[0x1BE + 14],
            bytes[0x1BE + 15],
        ]);
        if partition_type != 0 && lba != 0 && sectors != 0 {
            return Ok(u64::from(lba) * 512);
        }
    }
    Ok(0)
}

pub(crate) fn entry_name<D, IO, TP, OCC>(
    entry: &lamfat::DirEntry<'_, IO, TP, OCC>,
) -> Result<String<NAME_BUF_LEN>, StorageError<D>>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
    IO: lamfat::ReadWriteSeek,
    TP: lamfat::TimeProvider,
    OCC: lamfat::OemCpConverter,
{
    let mut name = String::<NAME_BUF_LEN>::new();
    if let Some(lfn) = entry.long_file_name_as_ucs2_units() {
        for ch in char::decode_utf16(lfn.iter().copied()) {
            name.push(ch.unwrap_or(char::REPLACEMENT_CHARACTER))
                .map_err(|_| StorageError::NameTooLong)?;
        }
        return Ok(name);
    }

    let raw = entry.short_file_name_as_bytes();
    let base = trim_spaces(&raw[..8]);
    let ext = trim_spaces(&raw[8..]);
    push_ascii(&mut name, base)?;
    if !ext.is_empty() {
        name.push('.').map_err(|_| StorageError::NameTooLong)?;
        push_ascii(&mut name, ext)?;
    }
    Ok(name)
}

fn trim_spaces(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|byte| *byte != b' ')
        .map(|idx| idx + 1)
        .unwrap_or(0);
    &bytes[..end]
}

fn push_ascii<D>(name: &mut String<NAME_BUF_LEN>, bytes: &[u8]) -> Result<(), StorageError<D>>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    for byte in bytes {
        name.push(char::from(*byte))
            .map_err(|_| StorageError::NameTooLong)?;
    }
    Ok(())
}

fn read_binbook_page_count<IO, TP, OCC>(
    mut file: File<'_, IO, TP, OCC>,
    file_len: u64,
    scratch: &mut [u8],
) -> Result<u32, lamfat::Error<IO::Error>>
where
    IO: ReadWriteSeek,
    TP: TimeProvider,
{
    if scratch.len() < binbook_core::HEADER_SIZE || file_len < binbook_core::HEADER_SIZE as u64 {
        return Err(lamfat::Error::InvalidInput);
    }
    let header = &mut scratch[..binbook_core::HEADER_SIZE];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(header)?;
    if header.get(..8) != Some(b"BINBOOK\0")
        || read_u16(header, 12)? != binbook_core::HEADER_SIZE as u16
        || read_u16(header, 36)? != binbook_core::SECTION_RECORD_SIZE as u16
        || read_u16(header, 40)? != 128
        || read_u16(header, 42)? != 48
    {
        return Err(lamfat::Error::InvalidInput);
    }
    let declared_len = read_u64(header, 16)?;
    if declared_len != 0 && declared_len > file_len {
        return Err(lamfat::Error::InvalidInput);
    }
    let effective_len = if declared_len == 0 {
        file_len
    } else {
        declared_len
    };
    let section_table_offset = read_u64(header, 24)?;
    let section_table_len = read_u32(header, 32)? as usize;
    let section_count = read_u16(header, 38)? as usize;
    let required_table_len = section_count
        .checked_mul(binbook_core::SECTION_RECORD_SIZE)
        .ok_or(lamfat::Error::InvalidInput)?;
    if section_count == 0
        || section_table_offset < binbook_core::HEADER_SIZE as u64
        || section_table_len < required_table_len
        || required_table_len > scratch.len()
        || section_table_offset
            .checked_add(required_table_len as u64)
            .ok_or(lamfat::Error::InvalidInput)?
            > effective_len
    {
        return Err(lamfat::Error::InvalidInput);
    }

    let table = &mut scratch[..required_table_len];
    file.seek(SeekFrom::Start(section_table_offset))?;
    file.read_exact(table)?;
    for index in 0..section_count {
        let start = index * binbook_core::SECTION_RECORD_SIZE;
        let record = &table[start..start + binbook_core::SECTION_RECORD_SIZE];
        if read_u16(record, 0)? == 40 {
            let section_offset = read_u64(record, 4)?;
            let section_len = read_u64(record, 12)?;
            let entry_size = read_u32(record, 20)?;
            let record_count = read_u32(record, 24)?;
            if entry_size != 128 || record_count == 0 {
                return Err(lamfat::Error::InvalidInput);
            }
            if section_offset
                .checked_add(section_len)
                .ok_or(lamfat::Error::InvalidInput)?
                > effective_len
            {
                return Err(lamfat::Error::InvalidInput);
            }
            return Ok(record_count);
        }
    }
    Err(lamfat::Error::InvalidInput)
}

fn read_u16<E>(bytes: &[u8], offset: usize) -> Result<u16, lamfat::Error<E>> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or(lamfat::Error::InvalidInput)?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32<E>(bytes: &[u8], offset: usize) -> Result<u32, lamfat::Error<E>> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or(lamfat::Error::InvalidInput)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn read_u64<E>(bytes: &[u8], offset: usize) -> Result<u64, lamfat::Error<E>> {
    let value = bytes
        .get(offset..offset + 8)
        .ok_or(lamfat::Error::InvalidInput)?;
    Ok(u64::from_le_bytes([
        value[0], value[1], value[2], value[3], value[4], value[5], value[6], value[7],
    ]))
}
