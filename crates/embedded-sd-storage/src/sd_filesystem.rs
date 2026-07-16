//! Generic `SdStorage` — wraps an SD block device and exposes a flat `/BOOKS`
//! FAT directory through backend-agnostic operations.

use core::{cmp::min, fmt::Write as _, marker::PhantomData};

use binbook_core::ReadAt;
use embedded_hal::delay::DelayNs;
use embedded_hal::spi::SpiDevice;
use embedded_sdmmc::filesystem::TimeSource;
use embedded_sdmmc::{Block, BlockDevice, BlockIdx, SdCard};
use heapless::String;
use lamfat::{FileSystem, FsOptions, Read, Seek, SeekFrom, Write};

use crate::block_stream::{BlockDeviceStream, BorrowedBlockDevice, STREAM_BATCH_BLOCKS};
pub use crate::fat_directory::StorageError;
use crate::fat_directory::{entry_name, FatDirectory, BOOKS_DIR, NAME_BUF_LEN, TMP_DIR};
use crate::{
    decode_upload_journal, encode_upload_journal, UploadJournalPhase, UploadJournalRecord,
    UPLOAD_JOURNAL_BYTES,
};

const MAX_FILE_EXTENTS: usize = 128;
const READ_BATCH_BLOCKS: usize = 8;
const ZERO_CHUNK: [u8; 512] = [0; 512];
pub const UPLOAD_JOURNAL_A: &str = "XFERA.DAT";
pub const UPLOAD_JOURNAL_B: &str = "XFERB.DAT";

pub type MountedScratch = [Block; STREAM_BATCH_BLOCKS];
type MountedBlockStream<D> = BlockDeviceStream<D, &'static mut MountedScratch>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountedScanEntry {
    pub name: String<{ binbook_storage::MAX_BINBOOK_NAME_BYTES }>,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountedScanBatch {
    pub entries: heapless::Vec<MountedScanEntry, 8>,
    pub inspected: u8,
    pub complete: bool,
    pub unsupported_name: bool,
}

pub struct MountedScanFile<'a, D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    pub name: String<{ binbook_storage::MAX_BINBOOK_NAME_BYTES }>,
    pub size: u64,
    pub file: MountedFileSession<'a, D>,
}

pub struct MountedFileScanBatch<'a, D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    pub entries: heapless::Vec<MountedScanFile<'a, D>, 8>,
    pub inspected: u8,
    pub complete: bool,
    pub unsupported_name: bool,
}

/// A persistent `/BOOKS` directory iterator over one consuming FAT mount.
///
/// The session advances at most the caller's requested number of raw directory
/// entries per `step`; dropping it is cancellation. The filesystem remains
/// independently borrowable between calls, allowing foreground operations to
/// run before the next background batch.
pub struct MountedDirectoryScan<'a, D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    entries: Option<
        lamfat::DirIter<
            'a,
            MountedBlockStream<D>,
            lamfat::DefaultTimeProvider,
            lamfat::LossyOemCpConverter,
        >,
    >,
    complete: bool,
}

/// One long-lived FAT mount that owns its block device.
pub struct MountedStorage<D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    fs: FileSystem<MountedBlockStream<D>>,
}

pub struct MountedDirectorySession<'a, D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    dir: lamfat::Dir<
        'a,
        MountedBlockStream<D>,
        lamfat::DefaultTimeProvider,
        lamfat::LossyOemCpConverter,
    >,
}

pub struct MountedNamedFile<'a, D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    pub name: String<{ binbook_storage::MAX_BINBOOK_NAME_BYTES }>,
    pub file: MountedFileSession<'a, D>,
}

impl<'a, D> MountedDirectorySession<'a, D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    pub fn open_file(&self, name: &str) -> Result<MountedFileSession<'a, D>, StorageError<D>> {
        let file = self
            .dir
            .open_file(name)
            .map_err(map_mounted_not_found::<D>)?;
        Ok(MountedFileSession { file })
    }

    pub fn create_file_session(
        &self,
        name: &str,
    ) -> Result<MountedWriteSession<'a, D>, StorageError<D>> {
        let mut file = self.dir.create_file(name).map_err(StorageError::Fat)?;
        file.seek(SeekFrom::Start(0)).map_err(StorageError::Fat)?;
        file.truncate().map_err(StorageError::Fat)?;
        Ok(MountedWriteSession { file })
    }

    pub fn open_matching_files<const N: usize>(
        &self,
        names: &[&str],
    ) -> Result<heapless::Vec<MountedNamedFile<'a, D>, N>, StorageError<D>> {
        let mut files = heapless::Vec::new();
        for entry in self.dir.iter() {
            let entry = entry.map_err(StorageError::Fat)?;
            if !entry.is_file() {
                continue;
            }
            let name = entry_name::<D, _, _, _>(&entry)?;
            if !names.iter().any(|candidate| *candidate == name.as_str()) {
                continue;
            }
            let Ok(name) = String::try_from(name.as_str()) else {
                continue;
            };
            files
                .push(MountedNamedFile {
                    name,
                    file: MountedFileSession {
                        file: entry.to_file(),
                    },
                })
                .map_err(|_| StorageError::BadUploadOffset)?;
        }
        Ok(files)
    }
}

pub struct MountedFileSession<'a, D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    file: lamfat::File<
        'a,
        MountedBlockStream<D>,
        lamfat::DefaultTimeProvider,
        lamfat::LossyOemCpConverter,
    >,
}

pub struct MountedWriteSession<'a, D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    file: lamfat::File<
        'a,
        MountedBlockStream<D>,
        lamfat::DefaultTimeProvider,
        lamfat::LossyOemCpConverter,
    >,
}

impl<'a, D> MountedWriteSession<'a, D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    pub fn size(&mut self) -> Result<u64, StorageError<D>> {
        self.file.seek(SeekFrom::End(0)).map_err(StorageError::Fat)
    }

    pub fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<(), StorageError<D>> {
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(StorageError::Fat)?;
        self.file.write_all(data).map_err(StorageError::Fat)
    }

    pub fn flush(&mut self) -> Result<(), StorageError<D>> {
        self.file.flush().map_err(StorageError::Fat)
    }

    /// Reuse a flushed write session for immediate verified readback without
    /// reopening its directory entry.
    pub fn into_read_session(self) -> MountedFileSession<'a, D> {
        MountedFileSession { file: self.file }
    }
}

impl<D> MountedFileSession<'_, D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    pub fn size(&mut self) -> Result<u64, StorageError<D>> {
        self.file.seek(SeekFrom::End(0)).map_err(StorageError::Fat)
    }

    pub fn read_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), StorageError<D>> {
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(StorageError::Fat)?;
        self.file.read_exact(out).map_err(StorageError::Fat)
    }
}

impl<D> MountedStorage<D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    pub fn start_scan(&self) -> Result<MountedDirectoryScan<'_, D>, StorageError<D>> {
        MountedDirectoryScan::start(&self.fs)
    }

    pub fn open_directory(
        &self,
        dir_name: &str,
    ) -> Result<MountedDirectorySession<'_, D>, StorageError<D>> {
        let dir = self
            .fs
            .root_dir()
            .open_dir(dir_name)
            .map_err(map_mounted_not_found::<D>)?;
        Ok(MountedDirectorySession { dir })
    }

    pub fn open_or_create_directory(
        &self,
        dir_name: &str,
    ) -> Result<MountedDirectorySession<'_, D>, StorageError<D>> {
        let root = self.fs.root_dir();
        let dir = match root.open_dir(dir_name) {
            Ok(dir) => dir,
            Err(lamfat::Error::NotFound) => root.create_dir(dir_name).map_err(StorageError::Fat)?,
            Err(error) => return Err(StorageError::Fat(error)),
        };
        Ok(MountedDirectorySession { dir })
    }

    pub fn open_file(&self, name: &str) -> Result<MountedFileSession<'_, D>, StorageError<D>> {
        self.open_file_in_dir(BOOKS_DIR, name)
    }

    pub fn open_file_in_dir(
        &self,
        dir_name: &str,
        name: &str,
    ) -> Result<MountedFileSession<'_, D>, StorageError<D>> {
        let root = self.fs.root_dir();
        let dir = root
            .open_dir(dir_name)
            .map_err(map_mounted_not_found::<D>)?;
        let file = dir.open_file(name).map_err(map_mounted_not_found::<D>)?;
        Ok(MountedFileSession { file })
    }

    pub fn create_file_session_in_dir(
        &self,
        dir_name: &str,
        name: &str,
    ) -> Result<MountedWriteSession<'_, D>, StorageError<D>> {
        let root = self.fs.root_dir();
        let dir = match root.open_dir(dir_name) {
            Ok(dir) => dir,
            Err(lamfat::Error::NotFound) => root.create_dir(dir_name).map_err(StorageError::Fat)?,
            Err(error) => return Err(StorageError::Fat(error)),
        };
        let mut file = dir.create_file(name).map_err(StorageError::Fat)?;
        file.seek(SeekFrom::Start(0)).map_err(StorageError::Fat)?;
        file.truncate().map_err(StorageError::Fat)?;
        Ok(MountedWriteSession { file })
    }

    pub fn create_file_session(
        &self,
        name: &str,
    ) -> Result<MountedWriteSession<'_, D>, StorageError<D>> {
        self.create_file_session_in_dir(BOOKS_DIR, name)
    }

    pub fn begin_book_upload(
        &self,
        record: &UploadJournalRecord,
    ) -> Result<MountedWriteSession<'_, D>, StorageError<D>> {
        if record.phase != UploadJournalPhase::Receiving {
            return Err(StorageError::UploadJournal);
        }
        let stage = upload_stage_name(record.generation)?;
        self.create_file_session_in_dir(TMP_DIR, stage.as_str())
    }

    pub fn publish_book_upload(&self, record: &UploadJournalRecord) -> Result<(), StorageError<D>> {
        if record.phase != UploadJournalPhase::Receiving {
            return Err(StorageError::UploadJournal);
        }
        let stage = upload_stage_name(record.generation)?;
        let backup = upload_backup_name(record.generation)?;
        let verified = record_with_phase(record, UploadJournalPhase::Verified)?;
        let root = self.fs.root_dir();
        let books = match root.open_dir(BOOKS_DIR) {
            Ok(dir) => dir,
            Err(lamfat::Error::NotFound) => {
                root.create_dir(BOOKS_DIR).map_err(StorageError::Fat)?
            }
            Err(error) => return Err(StorageError::Fat(error)),
        };
        let tmp = root.open_dir(TMP_DIR).map_err(StorageError::Fat)?;
        self.write_upload_journal_in_dir(&tmp, &verified)?;
        tmp.rename(stage.as_str(), &tmp, record.target_name.as_str())
            .map_err(StorageError::Fat)?;
        let old_target_moved = books.open_file(record.target_name.as_str()).is_ok();
        if old_target_moved {
            tmp.remove(backup.as_str()).ok();
            books
                .rename(record.target_name.as_str(), &tmp, backup.as_str())
                .map_err(StorageError::Fat)?;
            self.write_upload_journal_in_dir(
                &tmp,
                &record_with_phase(record, UploadJournalPhase::OldMoved)?,
            )?;
        }
        if let Err(error) = tmp.rename(
            record.target_name.as_str(),
            &books,
            record.target_name.as_str(),
        ) {
            if books.open_file(record.target_name.as_str()).is_err() {
                let _ = tmp.rename(backup.as_str(), &books, record.target_name.as_str());
            }
            return Err(StorageError::Fat(error));
        }
        let tmp = root.open_dir(TMP_DIR).map_err(StorageError::Fat)?;
        self.write_upload_journal_in_dir(
            &tmp,
            &record_with_phase(record, UploadJournalPhase::Published)?,
        )?;
        tmp.remove(backup.as_str()).ok();
        self.clear_upload_journal_in_dir(&tmp, record)
    }

    pub fn abort_book_upload(&self, record: &UploadJournalRecord) -> Result<(), StorageError<D>> {
        let root = self.fs.root_dir();
        if let Ok(tmp) = root.open_dir(TMP_DIR) {
            let stage = upload_stage_name(record.generation)?;
            tmp.remove(stage.as_str()).ok();
        }
        Ok(())
    }

    pub fn discard_book_upload_stage(&self, generation: u32) -> Result<(), StorageError<D>> {
        let root = self.fs.root_dir();
        let tmp = match root.open_dir(TMP_DIR) {
            Ok(dir) => dir,
            Err(lamfat::Error::NotFound) => return Ok(()),
            Err(error) => return Err(StorageError::Fat(error)),
        };
        let stage = upload_stage_name(generation)?;
        match tmp.remove(stage.as_str()) {
            Ok(()) | Err(lamfat::Error::NotFound) => Ok(()),
            Err(error) => Err(StorageError::Fat(error)),
        }
    }

    pub fn recover_book_upload(&self) -> Result<(), StorageError<D>> {
        let Some(record) = self.read_upload_journal()? else {
            return Ok(());
        };
        let root = self.fs.root_dir();
        let books = match root.open_dir(BOOKS_DIR) {
            Ok(dir) => dir,
            Err(lamfat::Error::NotFound) => {
                root.create_dir(BOOKS_DIR).map_err(StorageError::Fat)?
            }
            Err(error) => return Err(StorageError::Fat(error)),
        };
        let tmp = match root.open_dir(TMP_DIR) {
            Ok(dir) => dir,
            Err(lamfat::Error::NotFound) => root.create_dir(TMP_DIR).map_err(StorageError::Fat)?,
            Err(error) => return Err(StorageError::Fat(error)),
        };
        let stage = upload_stage_name(record.generation)?;
        let backup = upload_backup_name(record.generation)?;
        match record.phase {
            UploadJournalPhase::Clear => {}
            UploadJournalPhase::Receiving | UploadJournalPhase::Verified => {
                tmp.remove(stage.as_str()).ok();
                tmp.remove(record.target_name.as_str()).ok();
            }
            UploadJournalPhase::OldMoved => {
                if books.open_file(record.target_name.as_str()).is_err() {
                    let _ = tmp.rename(backup.as_str(), &books, record.target_name.as_str());
                } else {
                    tmp.remove(backup.as_str()).ok();
                }
                tmp.remove(stage.as_str()).ok();
                tmp.remove(record.target_name.as_str()).ok();
            }
            UploadJournalPhase::Published => {
                tmp.remove(stage.as_str()).ok();
                tmp.remove(record.target_name.as_str()).ok();
                tmp.remove(backup.as_str()).ok();
            }
        }
        self.clear_upload_journal_in_dir(&tmp, &record)
    }

    pub fn next_upload_generation(&self) -> Result<u32, StorageError<D>> {
        Ok(self
            .read_upload_journal()?
            .map_or(1, |record| record.generation.wrapping_add(1).max(1)))
    }

    fn clear_upload_journal_in_dir(
        &self,
        tmp: &lamfat::Dir<
            '_,
            MountedBlockStream<D>,
            lamfat::DefaultTimeProvider,
            lamfat::LossyOemCpConverter,
        >,
        record: &UploadJournalRecord,
    ) -> Result<(), StorageError<D>> {
        let clear = UploadJournalRecord::new(
            UploadJournalPhase::Clear,
            record.generation.wrapping_add(1).max(1),
            0,
            0,
            record.target_name.as_str(),
        )
        .map_err(|_| StorageError::UploadJournal)?;
        self.write_upload_journal_in_dir(tmp, &clear)
    }

    fn read_upload_journal(&self) -> Result<Option<UploadJournalRecord>, StorageError<D>> {
        let root = self.fs.root_dir();
        let tmp = match root.open_dir(TMP_DIR) {
            Ok(dir) => dir,
            Err(lamfat::Error::NotFound) => return Ok(None),
            Err(error) => return Err(StorageError::Fat(error)),
        };
        let mut selected: Option<UploadJournalRecord> = None;
        for name in [UPLOAD_JOURNAL_A, UPLOAD_JOURNAL_B] {
            let Ok(mut file) = tmp.open_file(name) else {
                continue;
            };
            let mut bytes = [0; UPLOAD_JOURNAL_BYTES];
            if file.read_exact(&mut bytes).is_err() {
                continue;
            }
            let Ok(candidate) = decode_upload_journal(&bytes) else {
                continue;
            };
            if selected
                .as_ref()
                .is_none_or(|current| journal_is_newer(&candidate, current))
            {
                selected = Some(candidate);
            }
        }
        Ok(selected)
    }

    fn write_upload_journal_in_dir(
        &self,
        tmp: &lamfat::Dir<
            '_,
            MountedBlockStream<D>,
            lamfat::DefaultTimeProvider,
            lamfat::LossyOemCpConverter,
        >,
        record: &UploadJournalRecord,
    ) -> Result<(), StorageError<D>> {
        let slot = if (record.generation + u32::from(record.phase as u8)) & 1 == 0 {
            UPLOAD_JOURNAL_A
        } else {
            UPLOAD_JOURNAL_B
        };
        let mut bytes = [0; UPLOAD_JOURNAL_BYTES];
        encode_upload_journal(record, &mut bytes).map_err(|_| StorageError::UploadJournal)?;
        let mut file = tmp.create_file(slot).map_err(StorageError::Fat)?;
        let current_len = file.seek(SeekFrom::End(0)).map_err(StorageError::Fat)?;
        if current_len != UPLOAD_JOURNAL_BYTES as u64 {
            file.truncate().map_err(StorageError::Fat)?;
        }
        file.seek(SeekFrom::Start(0)).map_err(StorageError::Fat)?;
        file.write_all(&bytes).map_err(StorageError::Fat)?;
        file.flush().map_err(StorageError::Fat)
    }

    pub fn file_size(&self, name: &str) -> Result<u64, StorageError<D>> {
        self.file_size_in_dir(BOOKS_DIR, name)
    }

    pub fn file_size_in_dir(&self, dir_name: &str, name: &str) -> Result<u64, StorageError<D>> {
        let root = self.fs.root_dir();
        let dir = root
            .open_dir(dir_name)
            .map_err(map_mounted_not_found::<D>)?;
        let mut file = dir.open_file(name).map_err(map_mounted_not_found::<D>)?;
        file.seek(SeekFrom::End(0)).map_err(StorageError::Fat)
    }

    pub fn read_at(&self, name: &str, offset: u64, out: &mut [u8]) -> Result<(), StorageError<D>> {
        let root = self.fs.root_dir();
        let dir = root
            .open_dir(BOOKS_DIR)
            .map_err(map_mounted_not_found::<D>)?;
        let mut file = dir.open_file(name).map_err(map_mounted_not_found::<D>)?;
        file.seek(SeekFrom::Start(offset))
            .map_err(StorageError::Fat)?;
        file.read_exact(out).map_err(StorageError::Fat)
    }

    pub fn create_or_truncate(&self, name: &str) -> Result<(), StorageError<D>> {
        self.create_or_truncate_in_dir(BOOKS_DIR, name)
    }

    pub fn create_or_truncate_in_dir(
        &self,
        dir_name: &str,
        name: &str,
    ) -> Result<(), StorageError<D>> {
        let root = self.fs.root_dir();
        let dir = match root.open_dir(dir_name) {
            Ok(dir) => dir,
            Err(lamfat::Error::NotFound) => root.create_dir(dir_name).map_err(StorageError::Fat)?,
            Err(error) => return Err(StorageError::Fat(error)),
        };
        let mut file = dir.create_file(name).map_err(StorageError::Fat)?;
        file.seek(SeekFrom::Start(0)).map_err(StorageError::Fat)?;
        file.truncate().map_err(StorageError::Fat)?;
        file.flush().map_err(StorageError::Fat)
    }

    pub fn write_at(&self, name: &str, offset: u64, data: &[u8]) -> Result<(), StorageError<D>> {
        self.write_at_in_dir(BOOKS_DIR, name, offset, data)
    }

    pub fn write_at_in_dir(
        &self,
        dir_name: &str,
        name: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), StorageError<D>> {
        let root = self.fs.root_dir();
        let dir = root
            .open_dir(dir_name)
            .map_err(map_mounted_not_found::<D>)?;
        let mut file = dir.create_file(name).map_err(StorageError::Fat)?;
        file.seek(SeekFrom::Start(offset))
            .map_err(StorageError::Fat)?;
        file.write_all(data).map_err(StorageError::Fat)?;
        file.flush().map_err(StorageError::Fat)
    }

    pub fn flush(&self, name: &str) -> Result<(), StorageError<D>> {
        self.flush_in_dir(BOOKS_DIR, name)
    }

    pub fn flush_in_dir(&self, dir_name: &str, name: &str) -> Result<(), StorageError<D>> {
        let root = self.fs.root_dir();
        let dir = root
            .open_dir(dir_name)
            .map_err(map_mounted_not_found::<D>)?;
        let mut file = dir.open_file(name).map_err(map_mounted_not_found::<D>)?;
        file.flush().map_err(StorageError::Fat)
    }

    pub fn delete_file(&self, name: &str) -> Result<(), StorageError<D>> {
        self.delete_file_in_dir(BOOKS_DIR, name)
    }

    pub fn delete_file_in_dir(&self, dir_name: &str, name: &str) -> Result<(), StorageError<D>> {
        let root = self.fs.root_dir();
        let dir = root
            .open_dir(dir_name)
            .map_err(map_mounted_not_found::<D>)?;
        dir.remove(name).map_err(map_mounted_not_found::<D>)
    }
}

fn map_mounted_not_found<D>(
    error: lamfat::Error<crate::block_stream::StreamError<D::Error>>,
) -> StorageError<D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    if matches!(error, lamfat::Error::NotFound) {
        StorageError::NotFound
    } else {
        StorageError::Fat(error)
    }
}

fn upload_stage_name<D>(generation: u32) -> Result<String<12>, StorageError<D>>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    upload_transaction_name::<D>("BBU", generation)
}

fn upload_backup_name<D>(generation: u32) -> Result<String<12>, StorageError<D>>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    upload_transaction_name::<D>("BBB", generation)
}

fn upload_transaction_name<D>(prefix: &str, generation: u32) -> Result<String<12>, StorageError<D>>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    let mut name = String::new();
    write!(&mut name, "{prefix}{:05X}.TMP", generation & 0x000f_ffff)
        .map_err(|_| StorageError::UploadJournal)?;
    Ok(name)
}

fn record_with_phase<D>(
    record: &UploadJournalRecord,
    phase: UploadJournalPhase,
) -> Result<UploadJournalRecord, StorageError<D>>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    UploadJournalRecord::new(
        phase,
        record.generation,
        record.expected_size,
        record.expected_crc32,
        record.target_name.as_str(),
    )
    .map_err(|_| StorageError::UploadJournal)
}

fn journal_is_newer(candidate: &UploadJournalRecord, current: &UploadJournalRecord) -> bool {
    let generation_delta = candidate.generation.wrapping_sub(current.generation);
    (generation_delta != 0 && generation_delta < (1 << 31))
        || (candidate.generation == current.generation && candidate.phase > current.phase)
}

impl<'a, D> MountedDirectoryScan<'a, D>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    pub fn start(fs: &'a FileSystem<MountedBlockStream<D>>) -> Result<Self, StorageError<D>> {
        let root = fs.root_dir();
        let entries = match root.open_dir(BOOKS_DIR) {
            Ok(dir) => Some(dir.iter()),
            Err(lamfat::Error::NotFound) => None,
            Err(error) => return Err(StorageError::Fat(error)),
        };
        Ok(Self {
            complete: entries.is_none(),
            entries,
        })
    }

    pub fn step(&mut self, budget: u8) -> Result<MountedScanBatch, StorageError<D>> {
        if budget == 0 || budget > 8 {
            return Err(StorageError::BadUploadOffset);
        }
        let mut batch = MountedScanBatch {
            entries: heapless::Vec::new(),
            inspected: 0,
            complete: self.complete,
            unsupported_name: false,
        };
        let Some(entries) = self.entries.as_mut() else {
            return Ok(batch);
        };
        while batch.inspected < budget {
            let Some(entry) = entries.next() else {
                self.complete = true;
                batch.complete = true;
                break;
            };
            batch.inspected += 1;
            let entry = entry.map_err(StorageError::Fat)?;
            if !entry.is_file() {
                continue;
            }
            let name = entry_name::<D, _, _, _>(&entry)?;
            if !name.ends_with(".binbook") {
                continue;
            }
            let Ok(name) = String::try_from(name.as_str()) else {
                batch.unsupported_name = true;
                continue;
            };
            batch
                .entries
                .push(MountedScanEntry {
                    name,
                    size: entry.len(),
                })
                .map_err(|_| StorageError::BadUploadOffset)?;
        }
        Ok(batch)
    }

    pub fn step_with_files(
        &mut self,
        budget: u8,
    ) -> Result<MountedFileScanBatch<'a, D>, StorageError<D>> {
        if budget == 0 || budget > 8 {
            return Err(StorageError::BadUploadOffset);
        }
        let mut batch = MountedFileScanBatch {
            entries: heapless::Vec::new(),
            inspected: 0,
            complete: self.complete,
            unsupported_name: false,
        };
        let Some(entries) = self.entries.as_mut() else {
            return Ok(batch);
        };
        while batch.inspected < budget {
            let Some(entry) = entries.next() else {
                self.complete = true;
                batch.complete = true;
                break;
            };
            batch.inspected += 1;
            let entry = entry.map_err(StorageError::Fat)?;
            if !entry.is_file() {
                continue;
            }
            let name = entry_name::<D, _, _, _>(&entry)?;
            if !name.ends_with(".binbook") {
                continue;
            }
            let Ok(name) = String::try_from(name.as_str()) else {
                batch.unsupported_name = true;
                continue;
            };
            let size = entry.len();
            batch
                .entries
                .push(MountedScanFile {
                    name,
                    size,
                    file: MountedFileSession {
                        file: entry.to_file(),
                    },
                })
                .map_err(|_| StorageError::BadUploadOffset)?;
        }
        Ok(batch)
    }
}

pub trait MountedFileAccess {
    type Error;

    fn with_mounted_file<R>(
        &mut self,
        name: &str,
        use_file: impl FnOnce(&mut dyn ReadAt<Error = Self::Error>) -> R,
    ) -> Result<R, Self::Error>;
}

struct MountedFileSource<'fs, IO, D>
where
    IO: lamfat::ReadWriteSeek,
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    fs: &'fs FileSystem<IO>,
    name: &'fs str,
    size: u64,
    _device: PhantomData<D>,
}

impl<IO, D> ReadAt for MountedFileSource<'_, IO, D>
where
    IO: lamfat::ReadWriteSeek<Error = crate::block_stream::StreamError<D::Error>>,
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    type Error = StorageError<D>;

    fn len(&mut self) -> Result<u64, Self::Error> {
        Ok(self.size)
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        let root = self.fs.root_dir();
        let dir = root.open_dir(BOOKS_DIR).map_err(map_not_found::<D>)?;
        let mut file = dir.open_file(self.name).map_err(map_not_found::<D>)?;
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
        if filled == out.len() {
            Ok(())
        } else {
            Err(StorageError::UnexpectedEof)
        }
    }
}

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
    read_blocks: [Block; READ_BATCH_BLOCKS],
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
            read_blocks: core::array::from_fn(|_| Block::new()),
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
    /// Consume this handle and mount FAT over an owned block device.
    ///
    /// This is intended for a single long-lived storage-owner task. The
    /// returned filesystem owns the device, so directory iterators may remain
    /// live between bounded task steps without a self-borrow or global mutex.
    /// The caller owns the persistent eight-block scratch allocation, keeping
    /// it out of temporary FAT streams and async task moves.
    pub fn into_mounted_filesystem(
        self,
        scratch: &'static mut MountedScratch,
    ) -> Result<FileSystem<MountedBlockStream<D>>, StorageError<D>> {
        if self.active_upload.is_some() {
            return Err(StorageError::UploadInProgress);
        }
        let mut device = self.block_device;
        let base_offset = crate::fat_directory::filesystem_base_offset(&mut device)?;
        let stream = BlockDeviceStream::with_scratch(device, base_offset, scratch);
        FileSystem::new(stream, FsOptions::new()).map_err(StorageError::Fat)
    }

    pub fn into_owned_mount(
        self,
        scratch: &'static mut MountedScratch,
    ) -> Result<MountedStorage<D>, StorageError<D>> {
        self.into_mounted_filesystem(scratch)
            .map(|fs| MountedStorage { fs })
    }

    /// Construct from a raw `BlockDevice` (host-test path, no real SPI/delay).
    pub fn from_block_device(block_device: D, _time: TIME) -> Self {
        Self {
            block_device,
            active_upload: None,
            active_read: None,
            read_blocks: core::array::from_fn(|_| Block::new()),
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
        if u64::try_from(out.len()).unwrap_or(u64::MAX) > available {
            return Err(StorageError::UnexpectedEof);
        }
        let read_len = out.len();
        if read_len > 0 {
            read_extents(
                &mut self.block_device,
                &mut self.read_blocks,
                read.base_offset,
                &read.extents,
                offset,
                &mut out[..read_len],
            )?;
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

impl<D, TIME> MountedFileAccess for SdStorage<D, TIME>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
    TIME: TimeSource,
{
    type Error = StorageError<D>;

    fn with_mounted_file<R>(
        &mut self,
        name: &str,
        use_file: impl FnOnce(&mut dyn ReadAt<Error = Self::Error>) -> R,
    ) -> Result<R, Self::Error> {
        self.active_read = None;
        let base_offset = crate::fat_directory::filesystem_base_offset(&mut self.block_device)?;
        let stream = BlockDeviceStream::new(
            BorrowedBlockDevice::new(&mut self.block_device),
            base_offset,
        );
        let fs = FileSystem::new(stream, FsOptions::new()).map_err(StorageError::Fat)?;
        let size = {
            let root = fs.root_dir();
            let dir = root.open_dir(BOOKS_DIR).map_err(map_not_found::<D>)?;
            let mut file = dir.open_file(name).map_err(map_not_found::<D>)?;
            file.seek(SeekFrom::End(0)).map_err(StorageError::Fat)?
        };
        let result = {
            let mut source = MountedFileSource::<_, D> {
                fs: &fs,
                name,
                size,
                _device: PhantomData,
            };
            use_file(&mut source)
        };
        fs.unmount().map_err(StorageError::Fat)?;
        Ok(result)
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
    scratch: &mut [Block; READ_BATCH_BLOCKS],
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
            scratch,
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
    scratch: &mut [Block; READ_BATCH_BLOCKS],
    mut absolute: u64,
    mut out: &mut [u8],
) -> Result<(), StorageError<D>>
where
    D: BlockDevice,
    D::Error: core::fmt::Debug,
{
    while !out.is_empty() {
        let block_idx = BlockIdx((absolute / 512) as u32);
        let block_off = (absolute % 512) as usize;
        let full_blocks = if block_off == 0 {
            min(out.len() / 512, scratch.len())
        } else {
            0
        };
        let len = if full_blocks > 0 {
            device
                .read(&mut scratch[..full_blocks], block_idx)
                .map_err(StorageError::Device)?;
            let len = full_blocks * 512;
            for (source, destination) in scratch[..full_blocks]
                .iter()
                .zip(out[..len].chunks_exact_mut(512))
            {
                destination.copy_from_slice(&source.contents);
            }
            len
        } else {
            device
                .read(core::slice::from_mut(&mut scratch[0]), block_idx)
                .map_err(StorageError::Device)?;
            let len = min(out.len(), 512 - block_off);
            out[..len].copy_from_slice(&scratch[0].contents[block_off..block_off + len]);
            len
        };
        absolute += len as u64;
        out = &mut out[len..];
    }
    Ok(())
}
