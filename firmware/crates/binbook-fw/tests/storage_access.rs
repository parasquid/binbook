#![cfg(feature = "sd-storage")]

use std::collections::BTreeMap;
use std::{cell::RefCell, rc::Rc};

use binbook_core::ReadAt;
use binbook_fw::library::{Library, ScanErrorKind};
use binbook_fw::reader_state::{ProgressRecord, ReaderState, MAX_STATE_BYTES};
use binbook_fw::storage_access::{
    LibraryIndexStore, SdBookSource, SharedSdFilesystem, StateSlots, StateStoreError,
    LIBRARY_INDEX_FILE, STATE_SLOT_A, STATE_SLOT_B,
};
use binbook_storage::{Filesystem, WritableFilesystem};

#[derive(Default)]
struct MemoryFs {
    files: BTreeMap<String, Vec<u8>>,
}

#[derive(Default)]
struct FaultControl {
    fail_begin: bool,
    fail_write_call: Option<usize>,
    fail_commit: bool,
    fail_verification: bool,
    write_calls: usize,
}

#[derive(Default)]
struct FaultFs {
    files: BTreeMap<String, Vec<u8>>,
    staging: Option<(String, Vec<u8>)>,
    control: Rc<RefCell<FaultControl>>,
}

impl FaultFs {
    fn new(control: Rc<RefCell<FaultControl>>) -> Self {
        Self {
            control,
            ..Self::default()
        }
    }
}

impl Filesystem for FaultFs {
    type Error = ();

    fn for_each_entry(&mut self, visit: &mut dyn FnMut(&str, u64)) -> Result<(), Self::Error> {
        for (name, bytes) in &self.files {
            visit(name, bytes.len() as u64);
        }
        Ok(())
    }

    fn read_at(&mut self, name: &str, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        if self.control.borrow_mut().fail_verification {
            return Err(());
        }
        let bytes = self.files.get(name).ok_or(())?;
        let start = offset as usize;
        out.copy_from_slice(bytes.get(start..start + out.len()).ok_or(())?);
        Ok(())
    }

    fn file_size(&mut self, name: &str) -> Result<u64, Self::Error> {
        self.files
            .get(name)
            .map(|bytes| bytes.len() as u64)
            .ok_or(())
    }
}

impl WritableFilesystem for FaultFs {
    fn create_or_truncate(&mut self, name: &str) -> Result<(), Self::Error> {
        self.begin_upload(name, 0)
    }

    fn write_at(&mut self, name: &str, offset: u64, data: &[u8]) -> Result<(), Self::Error> {
        self.write_upload_chunk(name, offset, data)
    }

    fn flush(&mut self, name: &str) -> Result<(), Self::Error> {
        self.commit_upload(name)
    }

    fn delete_file(&mut self, name: &str) -> Result<(), Self::Error> {
        self.files.remove(name).map(|_| ()).ok_or(())
    }

    fn begin_upload(&mut self, name: &str, _expected_size: u64) -> Result<(), Self::Error> {
        let mut control = self.control.borrow_mut();
        control.write_calls = 0;
        if control.fail_begin {
            return Err(());
        }
        drop(control);
        self.staging = Some((name.into(), Vec::new()));
        Ok(())
    }

    fn write_upload_chunk(
        &mut self,
        name: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        let mut control = self.control.borrow_mut();
        let call = control.write_calls;
        control.write_calls += 1;
        if control.fail_write_call == Some(call) {
            return Err(());
        }
        drop(control);
        let (staged_name, bytes) = self.staging.as_mut().ok_or(())?;
        if staged_name != name || bytes.len() != offset as usize {
            return Err(());
        }
        bytes.extend_from_slice(data);
        Ok(())
    }

    fn commit_upload(&mut self, name: &str) -> Result<(), Self::Error> {
        if self.control.borrow().fail_commit {
            return Err(());
        }
        let (staged_name, bytes) = self.staging.take().ok_or(())?;
        if staged_name != name {
            return Err(());
        }
        self.files.insert(staged_name, bytes);
        Ok(())
    }

    fn abort_upload(&mut self, _name: &str) -> Result<(), Self::Error> {
        self.staging = None;
        Ok(())
    }
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
        let start = offset as usize;
        out.copy_from_slice(bytes.get(start..start + out.len()).ok_or(())?);
        Ok(())
    }

    fn file_size(&mut self, name: &str) -> Result<u64, Self::Error> {
        self.files
            .get(name)
            .map(|bytes| bytes.len() as u64)
            .ok_or(())
    }
}

impl WritableFilesystem for MemoryFs {
    fn create_or_truncate(&mut self, name: &str) -> Result<(), Self::Error> {
        self.files.insert(name.into(), Vec::new());
        Ok(())
    }

    fn write_at(&mut self, name: &str, offset: u64, data: &[u8]) -> Result<(), Self::Error> {
        let bytes = self.files.get_mut(name).ok_or(())?;
        let start = offset as usize;
        if bytes.len() < start {
            return Err(());
        }
        bytes.resize(bytes.len().max(start + data.len()), 0);
        bytes[start..start + data.len()].copy_from_slice(data);
        Ok(())
    }

    fn flush(&mut self, _name: &str) -> Result<(), Self::Error> {
        Ok(())
    }

    fn delete_file(&mut self, name: &str) -> Result<(), Self::Error> {
        self.files.remove(name).map(|_| ()).ok_or(())
    }
}

#[test]
fn two_handles_share_files_and_named_source_reads_more_than_four_kib() {
    let payload: Vec<u8> = (0..10_000).map(|index| (index % 251) as u8).collect();
    let shared = SharedSdFilesystem::new(MemoryFs::default());
    let mut writer = shared.handle();
    writer.create_or_truncate("large.binbook").unwrap();
    writer.write_at("large.binbook", 0, &payload).unwrap();

    let mut source = SdBookSource::new(shared.handle(), "large.binbook").unwrap();
    let mut out = [0; 6_000];
    source.read_exact_at(2_345, &mut out).unwrap();
    assert_eq!(&out[..], &payload[2_345..8_345]);
    assert_eq!(source.len().unwrap(), 10_000);
}

#[test]
fn library_index_store_streams_roundtrip_without_a_whole_index_buffer() {
    let mut fs = MemoryFs::default();
    fs.files.insert("z.binbook".into(), vec![0; 99]);
    fs.files.insert("a.binbook".into(), vec![0; 42]);
    let shared = SharedSdFilesystem::new(fs);
    let mut scan_handle = shared.handle();
    let library = Library::scan(&mut scan_handle, |_| ScanErrorKind::CardError);

    let mut store = LibraryIndexStore::new(shared.handle());
    store.save(&library).unwrap();
    let mut loaded = Library::empty();
    store.load_into(&mut loaded).unwrap();

    assert_eq!(loaded, library);
    let fs = shared.into_inner();
    assert_eq!(
        fs.files[LIBRARY_INDEX_FILE].len(),
        library.index_encoded_len()
    );
}

#[test]
fn corrupt_library_index_is_rejected_instead_of_replacing_cache() {
    let mut fs = MemoryFs::default();
    fs.files.insert("a.binbook".into(), vec![0; 42]);
    let shared = SharedSdFilesystem::new(fs);
    let mut scan_handle = shared.handle();
    let library = Library::scan(&mut scan_handle, |_| ScanErrorKind::CardError);
    let mut store = LibraryIndexStore::new(shared.handle());
    store.save(&library).unwrap();

    let mut corruptor = shared.handle();
    corruptor
        .write_at(LIBRARY_INDEX_FILE, 12, &[0, 0, 0, 0])
        .unwrap();
    let mut loaded = Library::empty();
    assert!(store.load_into(&mut loaded).is_err());
}

#[test]
fn state_slots_alternate_and_corrupt_newest_falls_back_to_previous() {
    let shared = SharedSdFilesystem::new(MemoryFs::default());
    let mut slots = StateSlots::new(shared.handle());
    let mut buffer = [0; MAX_STATE_BYTES];
    let initial = slots.load(&mut buffer);
    assert!(initial.warning);

    let mut state = initial.state;
    state
        .set_progress(ProgressRecord {
            filename_hash: 7,
            rendition_hash: [0x44; 32],
            page: 3,
        })
        .unwrap();
    slots.save(&mut state, &mut buffer).unwrap();
    assert_eq!(state.generation, 1);
    assert_eq!(state.lookup_page(7, &[0x44; 32]), Some(3));

    state
        .set_progress(ProgressRecord {
            filename_hash: 7,
            rendition_hash: [0x44; 32],
            page: 9,
        })
        .unwrap();
    slots.save(&mut state, &mut buffer).unwrap();
    assert_eq!(state.generation, 2);

    drop(slots);
    let mut corruptor = shared.handle();
    corruptor.write_at(STATE_SLOT_B, 20, &[0xff]).unwrap();
    let mut reloaded = StateSlots::new(shared.handle());
    let loaded = reloaded.load(&mut buffer);
    assert!(!loaded.warning);
    assert_eq!(loaded.state.generation, 1);
    assert_eq!(loaded.state.lookup_page(7, &[0x44; 32]), Some(3));

    let fs = shared.into_inner();
    assert!(fs.files.contains_key(STATE_SLOT_A));
    assert!(fs.files.contains_key(STATE_SLOT_B));
}

fn maximum_state() -> ReaderState {
    let mut state = ReaderState::empty();
    for index in 0..200_u64 {
        let mut rendition = [0x55; 32];
        rendition[..8].copy_from_slice(&index.to_le_bytes());
        state
            .set_progress(ProgressRecord {
                filename_hash: index + 1,
                rendition_hash: rendition,
                page: index as u32,
            })
            .unwrap();
    }
    state
}

#[test]
fn every_dual_slot_save_boundary_preserves_committed_ram_generation_on_failure() {
    let chunk_count = MAX_STATE_BYTES.div_ceil(512);
    for failure in 0..chunk_count + 3 {
        let control = Rc::new(RefCell::new(FaultControl::default()));
        let shared = SharedSdFilesystem::new(FaultFs::new(control.clone()));
        let mut slots = StateSlots::new(shared.handle());
        let mut buffer = [0; MAX_STATE_BYTES];
        let mut state = maximum_state();
        slots.load(&mut buffer);
        slots.save(&mut state, &mut buffer).unwrap();
        assert_eq!(state.generation, 1);

        {
            let mut fault = control.borrow_mut();
            match failure {
                0 => fault.fail_begin = true,
                value if value <= chunk_count => fault.fail_write_call = Some(value - 1),
                value if value == chunk_count + 1 => fault.fail_commit = true,
                _ => fault.fail_verification = true,
            }
        }
        let error = slots.save(&mut state, &mut buffer).unwrap_err();
        let expected_stage = match failure {
            0 => 2,
            value if value <= chunk_count => 3,
            value if value == chunk_count + 1 => 4,
            _ => 5,
        };
        assert_eq!(error.stage_code(), expected_stage);
        assert_eq!(state.generation, 1, "failure boundary {failure}");

        control.borrow_mut().fail_verification = false;
        let mut prior = [0; MAX_STATE_BYTES];
        let mut reader = shared.handle();
        let prior_len = reader.file_size(STATE_SLOT_A).unwrap() as usize;
        reader
            .read_at(STATE_SLOT_A, 0, &mut prior[..prior_len])
            .unwrap();
        assert_eq!(
            ReaderState::decode(&prior[..prior_len]).unwrap().generation,
            1
        );
    }
}

#[test]
fn state_store_error_stage_codes_are_stable_for_diagnostics() {
    assert_eq!(
        StateStoreError::<()>::Codec(binbook_fw::reader_state::ReaderStateError::BufferTooSmall)
            .stage_code(),
        1
    );
    assert_eq!(StateStoreError::Begin(()).stage_code(), 2);
    assert_eq!(StateStoreError::Write(()).stage_code(), 3);
    assert_eq!(StateStoreError::Commit(()).stage_code(), 4);
    assert_eq!(StateStoreError::<()>::Readback.stage_code(), 5);
    assert_eq!(StateStoreError::<()>::Mismatch.stage_code(), 6);
}
