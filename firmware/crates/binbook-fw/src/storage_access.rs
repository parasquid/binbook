use core::cell::RefCell;

use binbook_core::ReadAt;
use binbook_storage::{Filesystem, WritableFilesystem, MAX_BINBOOK_NAME_BYTES};
use embassy_sync::blocking_mutex::raw::NoopRawMutex as SharedRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use heapless::String;

use crate::library::{
    Library, LibraryIndexDecoder, LibraryIndexError, INDEX_HEADER_SIZE, INDEX_RECORD_SIZE,
};
use crate::reader_state::{ReaderState, ReaderStateError, MAX_STATE_BYTES};

pub const STATE_SLOT_A: &str = "BINBOOK.A.STATE";
pub const STATE_SLOT_B: &str = "BINBOOK.B.STATE";
pub const LEGACY_STATE_SLOT_A: &str = "LEGACY.BINBOOK.A.STATE";
pub const LEGACY_STATE_SLOT_B: &str = "LEGACY.BINBOOK.B.STATE";
pub const LIBRARY_INDEX_FILE: &str = "BINBOOK.LIBRARY.INDEX";
pub const LIBRARY_INDEX_SLOT_A: &str = "BINBOOK.A.INDEX";
pub const LIBRARY_INDEX_SLOT_B: &str = "BINBOOK.B.INDEX";
pub const METADATA_DIR: &str = "BINBOOK";
const WRITE_CHUNK_BYTES: usize = 512;

#[derive(Debug, PartialEq, Eq)]
pub enum LibraryIndexStoreError<E> {
    Backend(E),
    Codec(LibraryIndexError),
    Length,
}

pub struct LibraryIndexStore<'a, F> {
    fs: SdHandle<'a, F>,
}

impl<'a, F: WritableFilesystem> LibraryIndexStore<'a, F> {
    pub fn new(fs: SdHandle<'a, F>) -> Self {
        Self { fs }
    }

    pub fn load_into(
        &mut self,
        library: &mut Library,
    ) -> Result<(), LibraryIndexStoreError<F::Error>> {
        let file_len = self
            .fs
            .file_size(LIBRARY_INDEX_FILE)
            .map_err(LibraryIndexStoreError::Backend)?;
        let file_len = usize::try_from(file_len).map_err(|_| LibraryIndexStoreError::Length)?;
        if !(INDEX_HEADER_SIZE..=Library::max_index_bytes()).contains(&file_len) {
            return Err(LibraryIndexStoreError::Length);
        }
        let mut header = [0; INDEX_HEADER_SIZE];
        self.fs
            .read_at(LIBRARY_INDEX_FILE, 0, &mut header)
            .map_err(LibraryIndexStoreError::Backend)?;
        let mut decoder = LibraryIndexDecoder::new(&header, file_len, library)
            .map_err(LibraryIndexStoreError::Codec)?;
        let record_count = (file_len - INDEX_HEADER_SIZE) / INDEX_RECORD_SIZE;
        let mut record = [0; INDEX_RECORD_SIZE];
        for index in 0..record_count {
            let offset = INDEX_HEADER_SIZE + index * INDEX_RECORD_SIZE;
            self.fs
                .read_at(LIBRARY_INDEX_FILE, offset as u64, &mut record)
                .map_err(LibraryIndexStoreError::Backend)?;
            decoder
                .push_record(&record)
                .map_err(LibraryIndexStoreError::Codec)?;
        }
        decoder.finish().map_err(LibraryIndexStoreError::Codec)
    }

    pub fn save(&mut self, library: &Library) -> Result<(), LibraryIndexStoreError<F::Error>> {
        let file_len = library.index_encoded_len();
        let mut header = [0; INDEX_HEADER_SIZE];
        library
            .encode_index_header(&mut header)
            .map_err(LibraryIndexStoreError::Codec)?;
        self.fs
            .begin_upload(LIBRARY_INDEX_FILE, file_len as u64)
            .map_err(LibraryIndexStoreError::Backend)?;
        if let Err(error) = self.fs.write_upload_chunk(LIBRARY_INDEX_FILE, 0, &header) {
            let _ = self.fs.abort_upload(LIBRARY_INDEX_FILE);
            return Err(LibraryIndexStoreError::Backend(error));
        }
        let mut record = [0; INDEX_RECORD_SIZE];
        for index in 0..library.entries().len() {
            library
                .encode_index_record(index, &mut record)
                .map_err(LibraryIndexStoreError::Codec)?;
            let offset = INDEX_HEADER_SIZE + index * INDEX_RECORD_SIZE;
            if let Err(error) =
                self.fs
                    .write_upload_chunk(LIBRARY_INDEX_FILE, offset as u64, &record)
            {
                let _ = self.fs.abort_upload(LIBRARY_INDEX_FILE);
                return Err(LibraryIndexStoreError::Backend(error));
            }
        }
        self.fs
            .commit_upload(LIBRARY_INDEX_FILE)
            .map_err(LibraryIndexStoreError::Backend)
    }
}

pub struct NoFilesystem;

impl Filesystem for NoFilesystem {
    type Error = core::convert::Infallible;

    fn for_each_entry(&mut self, _visit: &mut dyn FnMut(&str, u64)) -> Result<(), Self::Error> {
        Ok(())
    }

    fn read_at(&mut self, _name: &str, _offset: u64, _out: &mut [u8]) -> Result<(), Self::Error> {
        unreachable!()
    }

    fn file_size(&mut self, _name: &str) -> Result<u64, Self::Error> {
        unreachable!()
    }
}

pub struct SharedSdFilesystem<F> {
    inner: Mutex<SharedRawMutex, RefCell<F>>,
}

impl<F> SharedSdFilesystem<F> {
    /// Share synchronous access between tasks on the single-thread Embassy
    /// executor.
    ///
    /// Every closure passed to `inner.lock` must finish without awaiting, and
    /// filesystem access must never occur from an interrupt handler. Under
    /// those invariants task code cannot interleave while a borrow is active,
    /// so masking interrupts for an entire FAT operation is unnecessary and
    /// would make USB, timers, and input unresponsive during slow SD I/O.
    pub const fn new(inner: F) -> Self {
        Self {
            inner: Mutex::new(RefCell::new(inner)),
        }
    }

    pub fn handle(&self) -> SdHandle<'_, F> {
        SdHandle { shared: self }
    }

    pub fn into_inner(self) -> F {
        self.inner.into_inner().into_inner()
    }
}

pub struct SdHandle<'a, F> {
    shared: &'a SharedSdFilesystem<F>,
}

impl<F> Clone for SdHandle<'_, F> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<F> Copy for SdHandle<'_, F> {}

#[cfg(feature = "sd-storage")]
impl<F: embedded_sd_storage::MountedFileAccess> SdHandle<'_, F> {
    pub fn with_mounted_file<R>(
        &mut self,
        name: &str,
        use_file: impl FnOnce(
            &mut dyn ReadAt<Error = <F as embedded_sd_storage::MountedFileAccess>::Error>,
        ) -> R,
    ) -> Result<R, <F as embedded_sd_storage::MountedFileAccess>::Error> {
        self.shared
            .inner
            .lock(|cell| cell.borrow_mut().with_mounted_file(name, use_file))
    }
}

impl<F: Filesystem> Filesystem for SdHandle<'_, F> {
    type Error = F::Error;

    fn for_each_entry(&mut self, visit: &mut dyn FnMut(&str, u64)) -> Result<(), Self::Error> {
        self.shared
            .inner
            .lock(|cell| cell.borrow_mut().for_each_entry(visit))
    }

    fn read_at(&mut self, name: &str, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        self.shared
            .inner
            .lock(|cell| cell.borrow_mut().read_at(name, offset, out))
    }

    fn file_size(&mut self, name: &str) -> Result<u64, Self::Error> {
        self.shared
            .inner
            .lock(|cell| cell.borrow_mut().file_size(name))
    }

    fn for_each_binbook_entry(
        &mut self,
        visit: &mut dyn FnMut(&str, u64, u32),
    ) -> Result<(), Self::Error> {
        self.shared
            .inner
            .lock(|cell| cell.borrow_mut().for_each_binbook_entry(visit))
    }
}

impl<F: WritableFilesystem> WritableFilesystem for SdHandle<'_, F> {
    fn create_or_truncate(&mut self, name: &str) -> Result<(), Self::Error> {
        self.shared
            .inner
            .lock(|cell| cell.borrow_mut().create_or_truncate(name))
    }

    fn write_at(&mut self, name: &str, offset: u64, data: &[u8]) -> Result<(), Self::Error> {
        self.shared
            .inner
            .lock(|cell| cell.borrow_mut().write_at(name, offset, data))
    }

    fn flush(&mut self, name: &str) -> Result<(), Self::Error> {
        self.shared.inner.lock(|cell| cell.borrow_mut().flush(name))
    }

    fn delete_file(&mut self, name: &str) -> Result<(), Self::Error> {
        self.shared
            .inner
            .lock(|cell| cell.borrow_mut().delete_file(name))
    }

    fn begin_upload(&mut self, name: &str, expected_size: u64) -> Result<(), Self::Error> {
        self.shared
            .inner
            .lock(|cell| cell.borrow_mut().begin_upload(name, expected_size))
    }

    fn write_upload_chunk(
        &mut self,
        name: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        self.shared
            .inner
            .lock(|cell| cell.borrow_mut().write_upload_chunk(name, offset, data))
    }

    fn commit_upload(&mut self, name: &str) -> Result<(), Self::Error> {
        self.shared
            .inner
            .lock(|cell| cell.borrow_mut().commit_upload(name))
    }

    fn abort_upload(&mut self, name: &str) -> Result<(), Self::Error> {
        self.shared
            .inner
            .lock(|cell| cell.borrow_mut().abort_upload(name))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SdBookSourceError<E> {
    Backend(E),
}

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeBookSourceError<E> {
    Sd(SdBookSourceError<E>),
    #[cfg(feature = "diagnostic-console")]
    Embedded(binbook_core::SliceReadError),
}

pub struct SdBookSource<'a, F> {
    fs: SdHandle<'a, F>,
    name: String<MAX_BINBOOK_NAME_BYTES>,
}

impl<'a, F> SdBookSource<'a, F> {
    #[allow(clippy::result_unit_err)]
    pub fn new(fs: SdHandle<'a, F>, name: &str) -> Result<Self, ()> {
        Ok(Self {
            fs,
            name: String::try_from(name).map_err(|_| ())?,
        })
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }
}

impl<F: Filesystem> ReadAt for SdBookSource<'_, F> {
    type Error = SdBookSourceError<F::Error>;

    fn len(&mut self) -> Result<u64, Self::Error> {
        self.fs
            .file_size(self.name.as_str())
            .map_err(SdBookSourceError::Backend)
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        self.fs
            .read_at(self.name.as_str(), offset, out)
            .map_err(SdBookSourceError::Backend)
    }
}

pub enum RuntimeBookSource<'a, F> {
    Sd(SdBookSource<'a, F>),
    #[cfg(feature = "diagnostic-console")]
    Embedded(binbook_core::SliceSource<'static>),
}

impl<F: Filesystem> ReadAt for RuntimeBookSource<'_, F> {
    type Error = RuntimeBookSourceError<F::Error>;

    fn len(&mut self) -> Result<u64, Self::Error> {
        match self {
            Self::Sd(source) => source.len().map_err(RuntimeBookSourceError::Sd),
            #[cfg(feature = "diagnostic-console")]
            Self::Embedded(source) => source.len().map_err(RuntimeBookSourceError::Embedded),
        }
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        match self {
            Self::Sd(source) => source
                .read_exact_at(offset, out)
                .map_err(RuntimeBookSourceError::Sd),
            #[cfg(feature = "diagnostic-console")]
            Self::Embedded(source) => source
                .read_exact_at(offset, out)
                .map_err(RuntimeBookSourceError::Embedded),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StateStoreError<E> {
    Codec(ReaderStateError),
    Begin(E),
    Write(E),
    Commit(E),
    Readback,
    Mismatch,
}

impl<E> StateStoreError<E> {
    pub const fn stage_code(&self) -> u8 {
        match self {
            Self::Codec(_) => 1,
            Self::Begin(_) => 2,
            Self::Write(_) => 3,
            Self::Commit(_) => 4,
            Self::Readback => 5,
            Self::Mismatch => 6,
        }
    }
}

pub struct StateLoad {
    pub state: ReaderState,
    pub warning: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparedStateSave {
    pub target_slot: usize,
    pub target_name: &'static str,
    pub next_generation: u32,
    pub used: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StateFinishError {
    Decode(ReaderStateError),
    Mismatch,
}

pub struct StateSlots<'a, F> {
    fs: SdHandle<'a, F>,
    loaded_slot: Option<usize>,
    loaded_generation: u32,
}

impl<'a, F: WritableFilesystem> StateSlots<'a, F> {
    pub fn new(fs: SdHandle<'a, F>) -> Self {
        Self {
            fs,
            loaded_slot: None,
            loaded_generation: 0,
        }
    }

    pub fn load(&mut self, buffer: &mut [u8]) -> StateLoad {
        let a_generation = read_slot(&mut self.fs, STATE_SLOT_A, buffer)
            .ok()
            .map(|state| state.generation);
        let b_generation = read_slot(&mut self.fs, STATE_SLOT_B, buffer)
            .ok()
            .map(|state| state.generation);
        self.loaded_slot = select_generation(a_generation, b_generation);
        let warning = self.loaded_slot.is_none();
        let state = match self.loaded_slot {
            Some(0) => read_slot(&mut self.fs, STATE_SLOT_A, buffer)
                .expect("slot A was valid during selection"),
            Some(1) => read_slot(&mut self.fs, STATE_SLOT_B, buffer)
                .expect("slot B was valid during selection"),
            _ => ReaderState::empty(),
        };
        self.loaded_generation = state.generation;
        StateLoad { state, warning }
    }

    pub fn save(
        &mut self,
        state: &mut ReaderState,
        buffer: &mut [u8],
    ) -> Result<(), StateStoreError<F::Error>> {
        let prepared = self
            .prepare_save(state, buffer)
            .map_err(StateStoreError::Codec)?;

        self.fs
            .begin_upload(prepared.target_name, prepared.used as u64)
            .map_err(StateStoreError::Begin)?;
        for (index, chunk) in buffer[..prepared.used]
            .chunks(WRITE_CHUNK_BYTES)
            .enumerate()
        {
            if let Err(error) = self.fs.write_upload_chunk(
                prepared.target_name,
                (index * WRITE_CHUNK_BYTES) as u64,
                chunk,
            ) {
                let _ = self.fs.abort_upload(prepared.target_name);
                return Err(StateStoreError::Write(error));
            }
        }
        self.fs
            .commit_upload(prepared.target_name)
            .map_err(StateStoreError::Commit)?;

        self.fs
            .read_at(prepared.target_name, 0, &mut buffer[..prepared.used])
            .map_err(|_| StateStoreError::Readback)?;
        self.finish_save(state, buffer, prepared)
            .map_err(|error| match error {
                StateFinishError::Decode(error) => StateStoreError::Codec(error),
                StateFinishError::Mismatch => StateStoreError::Mismatch,
            })
    }

    pub fn prepare_save(
        &self,
        state: &ReaderState,
        buffer: &mut [u8],
    ) -> Result<PreparedStateSave, ReaderStateError> {
        let target_slot = match self.loaded_slot {
            Some(0) => 1,
            Some(1) | None => 0,
            _ => unreachable!(),
        };
        let target_name = if target_slot == 0 {
            STATE_SLOT_A
        } else {
            STATE_SLOT_B
        };
        let next_generation = self.loaded_generation.wrapping_add(1);
        let used = state.encode_with_generation(next_generation, buffer)?;
        Ok(PreparedStateSave {
            target_slot,
            target_name,
            next_generation,
            used,
        })
    }

    pub fn finish_save(
        &mut self,
        state: &mut ReaderState,
        buffer: &[u8],
        prepared: PreparedStateSave,
    ) -> Result<(), StateFinishError> {
        let verified =
            ReaderState::decode(&buffer[..prepared.used]).map_err(StateFinishError::Decode)?;
        if verified.generation != prepared.next_generation
            || !state.matches_except_generation(&verified)
        {
            return Err(StateFinishError::Mismatch);
        }
        state.generation = prepared.next_generation;
        self.loaded_generation = prepared.next_generation;
        self.loaded_slot = Some(prepared.target_slot);
        Ok(())
    }
}

fn select_generation(a: Option<u32>, b: Option<u32>) -> Option<usize> {
    match (a, b) {
        (None, None) => None,
        (Some(_), None) => Some(0),
        (None, Some(_)) => Some(1),
        (Some(a), Some(b)) => {
            if b.wrapping_sub(a) < 0x8000_0000 {
                Some(1)
            } else {
                Some(0)
            }
        }
    }
}

fn read_slot<F: Filesystem>(
    fs: &mut SdHandle<'_, F>,
    name: &str,
    buffer: &mut [u8],
) -> Result<ReaderState, ()> {
    let size = fs.file_size(name).map_err(|_| ())?;
    let size = usize::try_from(size).map_err(|_| ())?;
    if !(crate::reader_state::STATE_HEADER_SIZE..=MAX_STATE_BYTES).contains(&size) {
        return Err(());
    }
    fs.read_at(name, 0, &mut buffer[..size]).map_err(|_| ())?;
    ReaderState::decode(&buffer[..size]).map_err(|_| ())
}
