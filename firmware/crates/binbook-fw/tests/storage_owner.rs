#![cfg(feature = "sd-storage")]
#![allow(clippy::drop_non_drop)]

use binbook_fw::storage_owner::{
    decode_scan_records, encode_scan_records, BufferLease, BufferPool, BufferSlot, CompletionError,
    FilesystemOwnerBackend, FilesystemOwnerError, MountedStorageOwnerBackend, OwnedMailbox,
    OwnedRequest, OwnerBackend, OwnerError, Request, RequestEnvelope, RequestError, Response,
    ResponseEnvelope, ScanProgress, ScanRecord, ScanToken, StorageClient, StorageMailbox,
    StorageName, StorageOwner, StorageOwnerPhase, StorageOwnerProtocol, StorageOwnerPump,
    StorageRequestQueue, MAX_BOUNDED_IO_BYTES, SCAN_BATCH_BYTES, SCAN_ENTRY_BUDGET,
};
use binbook_storage::{Filesystem, WritableFilesystem};
use std::collections::BTreeMap;
use std::{cell::RefCell, rc::Rc};

fn name(value: &str) -> StorageName {
    StorageName::new(value).unwrap()
}

#[derive(Debug, Clone)]
struct MemoryBlockState {
    blocks: Vec<embedded_sd_storage::Block>,
    reads: usize,
}

#[derive(Debug, Clone)]
struct MemoryBlocks(Rc<RefCell<MemoryBlockState>>);

impl MemoryBlocks {
    fn fat_fixture() -> Self {
        let bytes = include_bytes!(
            "../../../../crates/embedded-sd-storage/tests/fixtures/fat16_with_book.img"
        );
        let blocks = bytes
            .chunks_exact(512)
            .map(|bytes| {
                let mut block = embedded_sd_storage::Block::new();
                block.contents.copy_from_slice(bytes);
                block
            })
            .collect();
        Self(Rc::new(RefCell::new(MemoryBlockState { blocks, reads: 0 })))
    }

    fn reset_reads(&self) {
        self.0.borrow_mut().reads = 0;
    }

    fn reads(&self) -> usize {
        self.0.borrow().reads
    }
}

impl embedded_sd_storage::BlockDevice for MemoryBlocks {
    type Error = core::convert::Infallible;

    fn read(
        &self,
        blocks: &mut [embedded_sd_storage::Block],
        start: embedded_sd_storage::BlockIdx,
    ) -> Result<(), Self::Error> {
        let mut source = self.0.borrow_mut();
        source.reads += blocks.len();
        for (index, block) in blocks.iter_mut().enumerate() {
            *block = source.blocks[start.0 as usize + index].clone();
        }
        Ok(())
    }

    fn write(
        &self,
        blocks: &[embedded_sd_storage::Block],
        start: embedded_sd_storage::BlockIdx,
    ) -> Result<(), Self::Error> {
        let mut target = self.0.borrow_mut();
        for (index, block) in blocks.iter().enumerate() {
            target.blocks[start.0 as usize + index] = block.clone();
        }
        Ok(())
    }

    fn num_blocks(&self) -> Result<embedded_sd_storage::BlockCount, Self::Error> {
        Ok(embedded_sd_storage::BlockCount(
            self.0.borrow().blocks.len() as u32,
        ))
    }
}

struct TestTime;

impl embedded_sd_storage::TimeSource for TestTime {
    fn get_timestamp(&self) -> embedded_sd_storage::Timestamp {
        embedded_sd_storage::Timestamp::from_calendar(2026, 7, 14, 0, 0, 0).unwrap()
    }
}

#[test]
fn mounted_owner_backend_executes_real_scan_read_upload_abort_and_delete() {
    let mut storage =
        embedded_sd_storage::SdStorage::from_block_device(MemoryBlocks::fat_fixture(), TestTime);
    storage.create_or_truncate("existing.binbook").unwrap();
    storage
        .write_at("existing.binbook", 0, b"existing")
        .unwrap();
    let mounted = storage.into_owned_mount().unwrap();
    let mut owner = StorageOwner::new(MountedStorageOwnerBackend::new(&mounted));
    let mut buffers = Buffers([0; 4096]);

    let token = match owner.execute(Request::ScanStart, &mut buffers).unwrap() {
        Response::ScanStarted(token) => token,
        response => panic!("unexpected scan response {response:?}"),
    };
    let progress = owner
        .execute(Request::scan_step(token), &mut buffers)
        .unwrap();
    let Response::ScanProgress(progress) = progress else {
        panic!("unexpected scan progress response")
    };
    let records = decode_scan_records(progress.entries, &buffers.0).unwrap();
    assert!(records
        .iter()
        .any(|record| record.name.as_str() == "existing.binbook"));
    assert!(records.iter().all(|record| record.mount_token.is_none()));
    assert!(matches!(
        owner.execute(Request::ScanCancel { token }, &mut buffers),
        Ok(Response::Ack)
    ));
    let token = match owner.execute(Request::ScanStart, &mut buffers).unwrap() {
        Response::ScanStarted(token) => token,
        response => panic!("unexpected scan response {response:?}"),
    };
    assert!(matches!(
        owner.execute(Request::ScanFinish { token }, &mut buffers),
        Ok(Response::Ack)
    ));

    let upload_id = match owner
        .execute(
            Request::UploadBegin {
                name: name("state.bin"),
                expected_size: 5,
                power_save: true,
            },
            &mut buffers,
        )
        .unwrap()
    {
        Response::UploadStarted(id) => id,
        response => panic!("unexpected upload response {response:?}"),
    };
    buffers.0[..5].copy_from_slice(b"state");
    owner
        .execute(
            Request::UploadWrite {
                upload_id,
                offset: 0,
                buffer_offset: 0,
                len: 5,
                buffer: BufferSlot::State,
                power_save: true,
            },
            &mut buffers,
        )
        .unwrap();
    assert!(matches!(
        owner.execute(Request::upload_commit(upload_id, true), &mut buffers),
        Ok(Response::Ack)
    ));
    buffers.0[..5].fill(0);
    owner
        .execute(
            Request::read(name("state.bin"), 0, 5, BufferSlot::State).unwrap(),
            &mut buffers,
        )
        .unwrap();
    assert_eq!(&buffers.0[..5], b"state");
    assert_eq!(
        owner
            .execute(
                Request::FileSize {
                    name: name("state.bin")
                },
                &mut buffers
            )
            .unwrap(),
        Response::FileSize(5)
    );

    let abort_id = match owner
        .execute(
            Request::UploadBegin {
                name: name("abort.bin"),
                expected_size: 3,
                power_save: false,
            },
            &mut buffers,
        )
        .unwrap()
    {
        Response::UploadStarted(id) => id,
        response => panic!("unexpected upload response {response:?}"),
    };
    assert!(matches!(
        owner.execute(
            Request::UploadAbort {
                upload_id: abort_id
            },
            &mut buffers
        ),
        Ok(Response::Ack)
    ));
    assert!(owner
        .execute(
            Request::FileSize {
                name: name("abort.bin")
            },
            &mut buffers
        )
        .is_err());

    assert!(matches!(
        owner.execute(
            Request::Delete {
                name: name("state.bin")
            },
            &mut buffers
        ),
        Ok(Response::Ack)
    ));
}

#[test]
fn mounted_owner_prefers_dedicated_library_index_and_falls_back_to_legacy_books() {
    use binbook_fw::storage_access::LIBRARY_INDEX_FILE;

    let mut storage =
        embedded_sd_storage::SdStorage::from_block_device(MemoryBlocks::fat_fixture(), TestTime);
    storage.create_or_truncate(LIBRARY_INDEX_FILE).unwrap();
    storage.write_at(LIBRARY_INDEX_FILE, 0, b"legacy").unwrap();
    let mounted = storage.into_owned_mount().unwrap();

    {
        let mut backend = MountedStorageOwnerBackend::new(&mounted);
        let mut bytes = [0; 6];
        assert_eq!(backend.file_size(LIBRARY_INDEX_FILE).unwrap(), 6);
        backend.read_at(LIBRARY_INDEX_FILE, 0, &mut bytes).unwrap();
        assert_eq!(&bytes, b"legacy");
    }

    mounted
        .create_or_truncate_in_dir("BINBOOK", LIBRARY_INDEX_FILE)
        .unwrap();
    mounted
        .write_at_in_dir("BINBOOK", LIBRARY_INDEX_FILE, 0, b"fast")
        .unwrap();
    let mut backend = MountedStorageOwnerBackend::new(&mounted);
    let mut bytes = [0; 4];
    assert_eq!(backend.file_size(LIBRARY_INDEX_FILE).unwrap(), 4);
    backend.read_at(LIBRARY_INDEX_FILE, 0, &mut bytes).unwrap();
    assert_eq!(&bytes, b"fast");
}

#[test]
fn mounted_owner_persists_a_decodable_library_index_in_metadata_directory() {
    use binbook_fw::{
        library::{IncrementalLibraryScan, Library},
        storage_access::LIBRARY_INDEX_FILE,
    };

    let storage =
        embedded_sd_storage::SdStorage::from_block_device(MemoryBlocks::fat_fixture(), TestTime);
    let mounted = storage.into_owned_mount().unwrap();
    let mut library = Library::empty();
    let mut scan = IncrementalLibraryScan::begin(&mut library);
    scan.push("one.binbook", 123);
    scan.finish();
    let mut encoded = vec![0; library.index_encoded_len()];
    library.encode_index(&mut encoded).unwrap();

    {
        let mut backend = MountedStorageOwnerBackend::new(&mounted);
        backend
            .upload_begin(LIBRARY_INDEX_FILE, encoded.len() as u64)
            .unwrap();
        backend
            .upload_write(LIBRARY_INDEX_FILE, 0, &encoded[..16])
            .unwrap();
        backend
            .upload_write(LIBRARY_INDEX_FILE, 16, &encoded[16..])
            .unwrap();
        backend.upload_commit(LIBRARY_INDEX_FILE).unwrap();
    }

    let mut bytes = vec![0; encoded.len()];
    let mut backend = MountedStorageOwnerBackend::new(&mounted);
    assert_eq!(
        backend.file_size(LIBRARY_INDEX_FILE).unwrap(),
        encoded.len() as u64
    );
    backend.read_at(LIBRARY_INDEX_FILE, 0, &mut bytes).unwrap();
    assert_eq!(bytes, encoded);
    assert_eq!(Library::decode_index(&bytes).unwrap(), library);
}

#[test]
fn mounted_owner_can_scan_after_reading_metadata_and_deleting_a_book() {
    use binbook_fw::storage_access::LIBRARY_INDEX_FILE;

    let mut storage =
        embedded_sd_storage::SdStorage::from_block_device(MemoryBlocks::fat_fixture(), TestTime);
    storage.create_or_truncate("deleted.binbook").unwrap();
    storage.write_at("deleted.binbook", 0, b"book").unwrap();
    let mounted = storage.into_owned_mount().unwrap();
    mounted
        .create_or_truncate_in_dir("BINBOOK", LIBRARY_INDEX_FILE)
        .unwrap();
    mounted
        .write_at_in_dir("BINBOOK", LIBRARY_INDEX_FILE, 0, b"index")
        .unwrap();
    let mut backend = MountedStorageOwnerBackend::new(&mounted);
    let mut metadata = [0; 5];
    let mut scan_bytes = [0; SCAN_BATCH_BYTES];

    backend
        .read_at(LIBRARY_INDEX_FILE, 0, &mut metadata)
        .unwrap();
    backend.delete("deleted.binbook").unwrap();
    backend.scan_start().unwrap();
    let progress = backend
        .scan_step(SCAN_ENTRY_BUDGET, &mut scan_bytes)
        .unwrap();

    assert_eq!(metadata, *b"index");
    let records = decode_scan_records(progress.entries, &scan_bytes).unwrap();
    assert!(records
        .iter()
        .all(|record| record.name.as_str() != "deleted.binbook"));
}

#[test]
fn metadata_upload_keeps_the_selected_book_read_session_warm() {
    use binbook_fw::storage_access::STATE_SLOT_A;

    let blocks = MemoryBlocks::fat_fixture();
    let mut storage = embedded_sd_storage::SdStorage::from_block_device(blocks.clone(), TestTime);
    storage.create_or_truncate("selected.binbook").unwrap();
    storage
        .write_at("selected.binbook", 0, &[0x5a; 1024])
        .unwrap();
    let mounted = storage.into_owned_mount().unwrap();
    let mut backend = MountedStorageOwnerBackend::new(&mounted);
    let mut byte = [0; 1];

    backend.read_at("selected.binbook", 0, &mut byte).unwrap();
    backend.upload_begin(STATE_SLOT_A, 1).unwrap();
    backend.upload_write(STATE_SLOT_A, 0, &[7]).unwrap();
    backend.upload_commit(STATE_SLOT_A).unwrap();

    blocks.reset_reads();
    backend.read_at("selected.binbook", 513, &mut byte).unwrap();
    assert_eq!(byte, [0x5a]);
    assert!(
        blocks.reads() <= 2,
        "metadata upload reopened the selected book: {} block reads",
        blocks.reads()
    );
}

#[test]
fn committed_metadata_upload_is_read_back_through_the_same_file_session() {
    use binbook_fw::storage_access::STATE_SLOT_A;

    let blocks = MemoryBlocks::fat_fixture();
    let storage = embedded_sd_storage::SdStorage::from_block_device(blocks.clone(), TestTime);
    let mounted = storage.into_owned_mount().unwrap();
    let mut backend = MountedStorageOwnerBackend::new(&mounted);
    let expected = [0x3c; 292];

    backend
        .upload_begin(STATE_SLOT_A, expected.len() as u64)
        .unwrap();
    backend.upload_write(STATE_SLOT_A, 0, &expected).unwrap();
    backend.upload_commit(STATE_SLOT_A).unwrap();

    blocks.reset_reads();
    let mut actual = [0; 292];
    backend.read_at(STATE_SLOT_A, 0, &mut actual).unwrap();
    assert_eq!(actual, expected);
    assert!(
        blocks.reads() <= 2,
        "verified readback reopened the metadata directory: {} block reads",
        blocks.reads()
    );
}

#[test]
fn completed_scan_token_can_reactivate_a_file_after_switching_away() {
    let blocks = MemoryBlocks::fat_fixture();
    let mut storage = embedded_sd_storage::SdStorage::from_block_device(blocks.clone(), TestTime);
    storage.create_or_truncate("selected.binbook").unwrap();
    storage
        .write_at("selected.binbook", 0, &[0x6b; 1024])
        .unwrap();
    storage.create_or_truncate("other.binbook").unwrap();
    storage.write_at("other.binbook", 0, &[0x4a; 512]).unwrap();
    let mounted = storage.into_owned_mount().unwrap();
    let mut owner = StorageOwner::new(MountedStorageOwnerBackend::new(&mounted));
    let mut buffers = Buffers([0; 4096]);
    let scan = match owner.execute(Request::ScanStart, &mut buffers).unwrap() {
        Response::ScanStarted(token) => token,
        response => panic!("unexpected response {response:?}"),
    };
    owner
        .execute(
            Request::ScanPrefetch {
                token: scan,
                name: name("selected.binbook"),
            },
            &mut buffers,
        )
        .unwrap();
    let selected_token = loop {
        let Response::ScanProgress(progress) = owner
            .execute(Request::scan_step(scan), &mut buffers)
            .unwrap()
        else {
            panic!("scan progress required")
        };
        let records = decode_scan_records(progress.entries, &buffers.0).unwrap();
        if let Some(token) = records
            .iter()
            .find(|record| record.name.as_str() == "selected.binbook")
            .and_then(|record| record.mount_token)
        {
            if progress.complete {
                break token;
            }
            while !match owner
                .execute(Request::scan_step(scan), &mut buffers)
                .unwrap()
            {
                Response::ScanProgress(next) => next.complete,
                _ => false,
            } {}
            break token;
        }
        assert!(!progress.complete, "selected file missing from scan");
    };
    owner
        .execute(Request::ScanFinish { token: scan }, &mut buffers)
        .unwrap();

    blocks.reset_reads();
    assert!(matches!(
        owner.execute(
            Request::ActivateFile {
                name: name("selected.binbook"),
                token: selected_token,
            },
            &mut buffers,
        ),
        Ok(Response::FileSize(1024))
    ));
    owner
        .execute(
            Request::read(name("selected.binbook"), 513, 1, BufferSlot::Reader).unwrap(),
            &mut buffers,
        )
        .unwrap();
    assert_eq!(buffers.0[0], 0x6b);
    assert!(
        blocks.reads() <= 2,
        "token activation rescanned the directory: {} block reads",
        blocks.reads()
    );
    assert!(matches!(
        owner.execute(
            Request::FileSize {
                name: name("other.binbook"),
            },
            &mut buffers,
        ),
        Ok(Response::FileSize(512))
    ));
    blocks.reset_reads();
    assert!(matches!(
        owner.execute(
            Request::ActivateFile {
                name: name("selected.binbook"),
                token: selected_token,
            },
            &mut buffers,
        ),
        Ok(Response::FileSize(1024))
    ));
    assert!(
        blocks.reads() <= 2,
        "token reactivation rescanned the directory: {} block reads",
        blocks.reads()
    );
}

#[test]
fn mounted_owner_file_token_cache_has_a_fixed_bounded_ram_cost() {
    let bytes = core::mem::size_of::<MountedStorageOwnerBackend<'static, MemoryBlocks>>();
    assert!(
        bytes <= 5 * 1024,
        "mounted owner backend uses {bytes} bytes"
    );
}

#[test]
fn mounted_owner_retains_only_viewport_capacity_sessions() {
    use binbook_fw::library::LIBRARY_VIEWPORT_CAPACITY;

    let mut storage =
        embedded_sd_storage::SdStorage::from_block_device(MemoryBlocks::fat_fixture(), TestTime);
    for index in 0..(LIBRARY_VIEWPORT_CAPACITY + 5) {
        let filename = format!("bounded-{index:02}.binbook");
        storage.create_or_truncate(&filename).unwrap();
        storage.write_at(&filename, 0, &[index as u8]).unwrap();
    }
    let mounted = storage.into_owned_mount().unwrap();
    let mut owner = StorageOwner::new(MountedStorageOwnerBackend::new(&mounted));
    let mut buffers = Buffers([0; 4096]);
    let scan = match owner.execute(Request::ScanStart, &mut buffers).unwrap() {
        Response::ScanStarted(token) => token,
        response => panic!("unexpected response {response:?}"),
    };
    let wanted: Vec<_> = (5..(LIBRARY_VIEWPORT_CAPACITY + 5))
        .map(|index| format!("bounded-{index:02}.binbook"))
        .collect();
    for filename in &wanted {
        owner
            .execute(
                Request::ScanPrefetch {
                    token: scan,
                    name: name(filename),
                },
                &mut buffers,
            )
            .unwrap();
    }
    let mut records = Vec::new();
    loop {
        let Response::ScanProgress(progress) = owner
            .execute(Request::scan_step(scan), &mut buffers)
            .unwrap()
        else {
            panic!("scan progress required")
        };
        records.extend(decode_scan_records(progress.entries, &buffers.0).unwrap());
        if progress.complete {
            break;
        }
    }
    owner
        .execute(Request::ScanFinish { token: scan }, &mut buffers)
        .unwrap();

    assert!(records.len() >= LIBRARY_VIEWPORT_CAPACITY + 5);
    assert_eq!(
        records
            .iter()
            .filter(|record| record.mount_token.is_some())
            .count(),
        LIBRARY_VIEWPORT_CAPACITY
    );
    assert!(records.iter().all(|record| {
        record.mount_token.is_some() == wanted.iter().any(|name| name == record.name.as_str())
    }));
}

#[test]
fn library_index_save_client_roundtrips_through_mounted_owner() {
    use binbook_fw::{
        library::{IncrementalLibraryScan, Library},
        library_index_save::{LibraryIndexSaveClient, LibraryIndexSaveUpdate},
        storage_access::LIBRARY_INDEX_FILE,
    };

    let storage =
        embedded_sd_storage::SdStorage::from_block_device(MemoryBlocks::fat_fixture(), TestTime);
    let mounted = storage.into_owned_mount().unwrap();
    let mut library = Library::empty();
    let mut scan = IncrementalLibraryScan::begin(&mut library);
    for index in 0..29 {
        scan.push(&format!("book-{index:02}.binbook"), index);
    }
    scan.finish();
    let mut owner = StorageOwner::new(MountedStorageOwnerBackend::new(&mounted));
    let mut buffers = Buffers([0; 4096]);
    let mut save_index = |library: &Library, sequence| {
        let (mut save, mut request) = LibraryIndexSaveClient::begin(library, sequence);
        loop {
            let result = owner.execute(request.request, &mut buffers);
            let response = ResponseEnvelope {
                client: StorageClient::Library,
                sequence: request.sequence,
                response: result.map_err(|_| binbook_fw::storage_owner::CompletionError::Backend),
            };
            match save.accept(response, &mut buffers.0).unwrap() {
                LibraryIndexSaveUpdate::Request(next) => request = next,
                LibraryIndexSaveUpdate::Complete => break,
            }
        }
    };
    save_index(&library, 100);

    let mut pruned = Library::empty();
    let mut scan = IncrementalLibraryScan::begin(&mut pruned);
    for index in 0..28 {
        scan.push(&format!("book-{index:02}.binbook"), index);
    }
    scan.finish();
    save_index(&pruned, 200);
    drop(save_index);
    drop(owner);

    let mut backend = MountedStorageOwnerBackend::new(&mounted);
    let len = backend.file_size(LIBRARY_INDEX_FILE).unwrap() as usize;
    let mut encoded = vec![0; len];
    backend
        .read_at(LIBRARY_INDEX_FILE, 0, &mut encoded)
        .unwrap();
    assert_eq!(Library::decode_index(&encoded).unwrap(), pruned);
}

#[test]
fn owner_state_slots_save_verify_and_reload_through_mounted_owner() {
    use binbook_fw::{
        reader_state::{ProgressRecord, ReaderState, MAX_STATE_BYTES},
        state_owner::{OwnerStateSlots, StateLoadUpdate, StateSaveUpdate},
    };

    let storage =
        embedded_sd_storage::SdStorage::from_block_device(MemoryBlocks::fat_fixture(), TestTime);
    let mounted = storage.into_owned_mount().unwrap();
    let mut owner = StorageOwner::new(MountedStorageOwnerBackend::new(&mounted));
    let mut state = ReaderState::empty();
    state
        .set_progress(ProgressRecord {
            filename_hash: 42,
            rendition_hash: [0x33; 32],
            page: 7,
        })
        .unwrap();
    let mut slots = OwnerStateSlots::new();
    let mut buffers = StateBuffers([0; MAX_STATE_BYTES]);
    let (mut save, mut request) = slots.begin_save(&mut state, &mut buffers.0, 500).unwrap();
    loop {
        let sequence = request.sequence;
        let response = owner.execute(request.request, &mut buffers);
        let envelope = ResponseEnvelope {
            client: StorageClient::State,
            sequence,
            response: response.map_err(|_| binbook_fw::storage_owner::CompletionError::Backend),
        };
        match save.accept(envelope, &mut buffers.0).unwrap() {
            StateSaveUpdate::Request(next) => request = next,
            StateSaveUpdate::Complete => break,
        }
    }
    drop(save);
    assert_eq!(state.generation, 1);

    let expected = state.clone();
    let mut loaded = ReaderState::empty();
    let mut reloaded_slots = OwnerStateSlots::new();
    let (mut load, mut request) = reloaded_slots.begin_load(&mut loaded, 700);
    loop {
        let sequence = request.sequence;
        let response = owner.execute(request.request, &mut buffers);
        let envelope = ResponseEnvelope {
            client: StorageClient::State,
            sequence,
            response: response.map_err(|_| binbook_fw::storage_owner::CompletionError::Backend),
        };
        match load.accept(envelope, &mut buffers.0).unwrap() {
            StateLoadUpdate::Request(next) => request = next,
            StateLoadUpdate::Complete { warning } => {
                assert!(!warning);
                break;
            }
        }
    }
    drop(load);
    assert_eq!(loaded, expected);
}

#[test]
fn scan_batch_wire_uses_caller_buffer_and_preserves_exact_names_and_sizes() {
    use binbook_fw::library::MountFileToken;

    let records = [
        ScanRecord {
            name: name("A.binbook"),
            size: 17,
            mount_token: Some(MountFileToken::new(41).unwrap()),
        },
        ScanRecord {
            name: name("long reader title.binbook"),
            size: u32::MAX as u64 + 9,
            mount_token: Some(MountFileToken::new(65535).unwrap()),
        },
    ];
    let mut bytes = [0xAA; SCAN_BATCH_BYTES];
    encode_scan_records(&records, &mut bytes).unwrap();
    assert_eq!(decode_scan_records(2, &bytes).unwrap(), records);
    assert!(bytes[2 * binbook_fw::storage_owner::SCAN_RECORD_BYTES..]
        .iter()
        .all(|byte| *byte == 0));
}

#[test]
fn owner_pump_traces_and_routes_the_exact_owned_request() {
    let mut bytes = [0u8; 8];
    let owner = StorageOwner::new(FakeBackend {
        bytes: *b"0123456789abcdef",
        ..Default::default()
    });
    let mut pump = StorageOwnerPump::<_, 2>::new(owner);
    let request = OwnedRequest {
        envelope: RequestEnvelope {
            client: StorageClient::State,
            sequence: 41,
            request: Request::read_into(name("state"), 4, 2, 4, BufferSlot::State).unwrap(),
        },
        buffer: Some(BufferLease::new(BufferSlot::State, &mut bytes)),
    };
    let mut phases = Vec::new();

    pump.enqueue(request, |phase| phases.push(phase)).unwrap();
    let response = pump
        .execute_next(|phase| phases.push(phase))
        .expect("queued request must complete");

    assert_eq!(
        phases,
        vec![
            StorageOwnerPhase::Queued {
                client: StorageClient::State,
                sequence: 41,
            },
            StorageOwnerPhase::Executing {
                client: StorageClient::State,
                sequence: 41,
            },
            StorageOwnerPhase::Completed {
                client: StorageClient::State,
                sequence: 41,
            },
        ]
    );
    assert_eq!(response.envelope.client, StorageClient::State);
    assert_eq!(response.envelope.sequence, 41);
    assert_eq!(response.envelope.response, Ok(Response::Read(4)));
    let lease = response.buffer.expect("caller buffer must be returned");
    assert_eq!(lease.slot(), BufferSlot::State);
    assert_eq!(lease.bytes(), b"\0\x004567\0\x00");
}

#[test]
fn owner_pump_keeps_an_upload_with_one_client_until_commit() {
    let mut byte = [7u8];
    let owner = StorageOwner::new(FakeBackend::default());
    let mut pump = StorageOwnerPump::<_, 4>::new(owner);
    let library_begin = owned_request(
        StorageClient::Library,
        1,
        Request::UploadBegin {
            name: name("index"),
            expected_size: 1,
            power_save: false,
        },
        None,
    );
    pump.enqueue(library_begin, |_| {}).unwrap();
    let started = pump.execute_next(|_| {}).unwrap();
    let Ok(Response::UploadStarted(upload_id)) = started.envelope.response else {
        panic!("library upload must start")
    };

    pump.enqueue(
        owned_request(
            StorageClient::State,
            2,
            Request::UploadBegin {
                name: name("state"),
                expected_size: 1,
                power_save: true,
            },
            None,
        ),
        |_| {},
    )
    .unwrap();
    pump.enqueue(
        owned_request(
            StorageClient::Library,
            3,
            Request::UploadWrite {
                upload_id,
                offset: 0,
                buffer_offset: 0,
                len: 1,
                buffer: BufferSlot::LibraryIndex,
                power_save: false,
            },
            Some(BufferLease::new(BufferSlot::LibraryIndex, &mut byte)),
        ),
        |_| {},
    )
    .unwrap();

    assert_eq!(
        pump.execute_next(|_| {}).unwrap().envelope.client,
        StorageClient::Library
    );
    assert!(pump.execute_next(|_| {}).is_none());

    pump.enqueue(
        owned_request(
            StorageClient::Library,
            4,
            Request::upload_commit(upload_id, false),
            None,
        ),
        |_| {},
    )
    .unwrap();
    assert_eq!(
        pump.execute_next(|_| {}).unwrap().envelope.client,
        StorageClient::Library
    );
    assert_eq!(
        pump.execute_next(|_| {}).unwrap().envelope.client,
        StorageClient::State
    );
}

fn owned_request<'a>(
    client: StorageClient,
    sequence: u32,
    request: Request,
    buffer: Option<BufferLease<'a>>,
) -> OwnedRequest<'a> {
    OwnedRequest {
        envelope: RequestEnvelope {
            client,
            sequence,
            request,
        },
        buffer,
    }
}

#[derive(Default)]
struct FakeBackend {
    bytes: [u8; 16],
    upload: Vec<u8>,
    scan_steps: usize,
    fail: bool,
}

impl OwnerBackend for FakeBackend {
    type Error = &'static str;
    fn file_size(&mut self, _: &str) -> Result<u64, Self::Error> {
        Ok(16)
    }
    fn read_at(&mut self, _: &str, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        if self.fail {
            return Err("read");
        }
        let start = offset as usize;
        out.copy_from_slice(&self.bytes[start..start + out.len()]);
        Ok(())
    }
    fn upload_begin(&mut self, _: &str, _: u64) -> Result<(), Self::Error> {
        self.upload.clear();
        Ok(())
    }
    fn upload_write(&mut self, _: &str, _: u64, data: &[u8]) -> Result<(), Self::Error> {
        self.upload.extend_from_slice(data);
        Ok(())
    }
    fn upload_commit(&mut self, _: &str) -> Result<(), Self::Error> {
        Ok(())
    }
    fn upload_abort(&mut self, _: &str) -> Result<(), Self::Error> {
        self.upload.clear();
        Ok(())
    }
    fn delete(&mut self, _: &str) -> Result<(), Self::Error> {
        Ok(())
    }
    fn scan_start(&mut self) -> Result<(), Self::Error> {
        self.scan_steps = 0;
        Ok(())
    }
    fn scan_prefetch(&mut self, _: &str) -> Result<(), Self::Error> {
        Ok(())
    }
    fn scan_step(&mut self, budget: u8, out: &mut [u8]) -> Result<ScanProgress, Self::Error> {
        self.scan_steps += 1;
        out[0] = self.scan_steps as u8;
        Ok(ScanProgress {
            entries: budget,
            complete: self.scan_steps == 2,
        })
    }
    fn scan_finish(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn scan_cancel(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

struct Buffers([u8; 4096]);
impl BufferPool for Buffers {
    fn buffer_mut(&mut self, _: BufferSlot) -> Option<&mut [u8]> {
        Some(&mut self.0)
    }
}

struct StateBuffers([u8; binbook_fw::reader_state::MAX_STATE_BYTES]);
impl BufferPool for StateBuffers {
    fn buffer_mut(&mut self, _slot: BufferSlot) -> Option<&mut [u8]> {
        Some(&mut self.0)
    }
}

#[test]
fn owner_executes_bounded_reads_uploads_and_scan_lifecycle() {
    let mut owner = StorageOwner::new(FakeBackend {
        bytes: *b"0123456789abcdef",
        ..Default::default()
    });
    let mut buffers = Buffers([0; 4096]);
    assert_eq!(
        owner.execute(
            Request::FileSize {
                name: name("a.binbook")
            },
            &mut buffers
        ),
        Ok(Response::FileSize(16))
    );
    assert_eq!(
        owner.execute(
            Request::read(name("a.binbook"), 4, 4, BufferSlot::Reader).unwrap(),
            &mut buffers
        ),
        Ok(Response::Read(4))
    );
    assert_eq!(&buffers.0[..4], b"4567");

    let upload = match owner
        .execute(
            Request::UploadBegin {
                name: name("state"),
                expected_size: 4,
                power_save: true,
            },
            &mut buffers,
        )
        .unwrap()
    {
        Response::UploadStarted(id) => id,
        other => panic!("unexpected {other:?}"),
    };
    buffers.0[..4].copy_from_slice(b"data");
    assert_eq!(
        owner.execute(
            Request::UploadWrite {
                upload_id: upload,
                offset: 0,
                buffer_offset: 0,
                len: 4,
                buffer: BufferSlot::State,
                power_save: true
            },
            &mut buffers
        ),
        Ok(Response::Written(4))
    );
    assert_eq!(
        owner.execute(Request::upload_commit(upload, true), &mut buffers),
        Ok(Response::Ack)
    );

    let token = match owner.execute(Request::ScanStart, &mut buffers).unwrap() {
        Response::ScanStarted(token) => token,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(
        owner.execute(Request::scan_step(token), &mut buffers),
        Ok(Response::ScanProgress(ScanProgress {
            entries: 8,
            complete: false
        }))
    );
    assert_eq!(
        owner.execute(Request::scan_step(token), &mut buffers),
        Ok(Response::ScanProgress(ScanProgress {
            entries: 8,
            complete: true
        }))
    );
    assert_eq!(
        owner.execute(Request::ScanFinish { token }, &mut buffers),
        Ok(Response::Ack)
    );
}

#[test]
fn owner_propagates_backend_and_buffer_errors_without_false_success() {
    let mut owner = StorageOwner::new(FakeBackend {
        fail: true,
        ..Default::default()
    });
    let mut buffers = Buffers([0; 4096]);
    assert_eq!(
        owner.execute(
            Request::read(name("a"), 0, 1, BufferSlot::Reader).unwrap(),
            &mut buffers
        ),
        Err(OwnerError::Backend("read"))
    );
}

#[test]
fn queue_services_foreground_and_power_save_between_200_entry_scan_batches() {
    let mut queue = StorageRequestQueue::<32>::new();
    let token = ScanToken(7);
    let mut scanned = 0usize;
    let mut serviced_reads = 0usize;
    let mut serviced_power = 0usize;
    let mut read_queued = false;
    let mut power_queued = false;

    while scanned < 200 {
        queue
            .push(scanned as u32, Request::scan_step(token))
            .unwrap();
        if scanned == 40 && !read_queued {
            queue
                .push(
                    1_000,
                    Request::read(name("a.binbook"), 0, 512, BufferSlot::Reader).unwrap(),
                )
                .unwrap();
            read_queued = true;
        }
        if scanned == 80 && !power_queued {
            queue.push(1_001, Request::upload_commit(9, true)).unwrap();
            power_queued = true;
        }

        let next = queue.pop().unwrap();
        match next.request {
            Request::ScanStep { .. } => scanned += SCAN_ENTRY_BUDGET as usize,
            Request::Read { .. } => serviced_reads += 1,
            Request::UploadCommit { .. } => serviced_power += 1,
            _ => unreachable!(),
        }
    }

    assert_eq!(serviced_reads, 1);
    assert_eq!(serviced_power, 1);
}

#[test]
fn queue_is_fifo_within_priority_and_rejects_capacity_overflow() {
    let mut queue = StorageRequestQueue::<2>::new();
    queue
        .push(
            10,
            Request::FileSize {
                name: name("a.binbook"),
            },
        )
        .unwrap();
    queue
        .push(
            11,
            Request::FileSize {
                name: name("b.binbook"),
            },
        )
        .unwrap();
    assert_eq!(
        queue.push(
            12,
            Request::FileSize {
                name: name("c.binbook")
            }
        ),
        Err(RequestError::Busy)
    );
    assert_eq!(queue.pop().unwrap().sequence, 10);
    assert_eq!(queue.pop().unwrap().sequence, 11);
}

#[test]
fn protocol_bounds_names_reads_and_caller_owned_buffer_lengths() {
    assert!(StorageName::new(&"x".repeat(122)).is_err());
    assert_eq!(MAX_BOUNDED_IO_BYTES, 4096);
    assert!(Request::read(name("a.binbook"), 0, 4096, BufferSlot::Reader).is_ok());
    assert_eq!(
        Request::read(name("a.binbook"), 0, 4097, BufferSlot::Reader),
        Err(RequestError::TooLarge)
    );
    assert!(core::mem::size_of::<Request>() < 160);
}

#[test]
fn foreground_io_is_prioritized_while_scan_steps_are_bounded() {
    let mut protocol = StorageOwnerProtocol::new();
    let token = protocol.start_scan().unwrap();
    assert_eq!(SCAN_ENTRY_BUDGET, 8);
    assert_eq!(protocol.scan_token(), Some(token));
    assert!(Request::scan_step(token).is_background());
    assert!(
        Request::read(name("a.binbook"), 0, 512, BufferSlot::Reader)
            .unwrap()
            .priority()
            > Request::scan_step(token).priority()
    );
}

#[test]
fn scan_busy_cancellation_and_stale_tokens_are_explicit() {
    let mut protocol = StorageOwnerProtocol::new();
    let first = protocol.start_scan().unwrap();
    assert_eq!(protocol.start_scan(), Err(RequestError::Busy));
    assert_eq!(
        protocol.finish_scan(ScanToken(first.get().wrapping_add(1))),
        Err(RequestError::Stale)
    );
    protocol.cancel_scan(first).unwrap();
    assert_eq!(protocol.scan_token(), None);
    let second = protocol.start_scan().unwrap();
    assert_ne!(first, second);
}

#[test]
fn upload_session_rejects_competing_begin_and_wrong_commit_or_abort() {
    let mut protocol = StorageOwnerProtocol::new();
    let upload = protocol.begin_upload().unwrap();
    assert_eq!(protocol.begin_upload(), Err(RequestError::Busy));
    assert_eq!(
        protocol.commit_upload(upload.wrapping_add(1)),
        Err(RequestError::Stale)
    );
    assert_eq!(
        protocol.abort_upload(upload.wrapping_add(1)),
        Err(RequestError::Stale)
    );
    protocol.abort_upload(upload).unwrap();
    assert!(protocol.begin_upload().is_ok());
}

#[derive(Default)]
struct MemoryFs(BTreeMap<String, Vec<u8>>);

impl Filesystem for MemoryFs {
    type Error = &'static str;

    fn for_each_entry(&mut self, visit: &mut dyn FnMut(&str, u64)) -> Result<(), Self::Error> {
        for (name, bytes) in &self.0 {
            visit(name, bytes.len() as u64);
        }
        Ok(())
    }

    fn read_at(&mut self, name: &str, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        let bytes = self.0.get(name).ok_or("missing")?;
        let start = offset as usize;
        out.copy_from_slice(bytes.get(start..start + out.len()).ok_or("range")?);
        Ok(())
    }

    fn file_size(&mut self, name: &str) -> Result<u64, Self::Error> {
        Ok(self.0.get(name).ok_or("missing")?.len() as u64)
    }
}

impl WritableFilesystem for MemoryFs {
    fn create_or_truncate(&mut self, name: &str) -> Result<(), Self::Error> {
        self.0.insert(name.into(), Vec::new());
        Ok(())
    }

    fn write_at(&mut self, name: &str, offset: u64, data: &[u8]) -> Result<(), Self::Error> {
        let bytes = self.0.get_mut(name).ok_or("missing")?;
        let end = offset as usize + data.len();
        if bytes.len() < end {
            bytes.resize(end, 0);
        }
        bytes[offset as usize..end].copy_from_slice(data);
        Ok(())
    }

    fn flush(&mut self, _: &str) -> Result<(), Self::Error> {
        Ok(())
    }

    fn delete_file(&mut self, name: &str) -> Result<(), Self::Error> {
        self.0.remove(name).ok_or("missing")?;
        Ok(())
    }
}

#[test]
fn filesystem_backend_executes_real_read_upload_commit_abort_and_delete_paths() {
    let mut fs = MemoryFs::default();
    fs.0.insert("book".into(), b"01234567".to_vec());
    let mut owner = StorageOwner::new(FilesystemOwnerBackend::new(fs));
    let mut buffers = Buffers([0; 4096]);
    assert_eq!(
        owner.execute(
            Request::read(name("book"), 2, 3, BufferSlot::Reader).unwrap(),
            &mut buffers
        ),
        Ok(Response::Read(3))
    );
    assert_eq!(&buffers.0[..3], b"234");
    assert_eq!(
        owner.execute(
            Request::read_into(name("book"), 5, 12, 3, BufferSlot::Reader).unwrap(),
            &mut buffers
        ),
        Ok(Response::Read(3))
    );
    assert_eq!(&buffers.0[12..15], b"567");

    let id = match owner
        .execute(
            Request::UploadBegin {
                name: name("state"),
                expected_size: 3,
                power_save: true,
            },
            &mut buffers,
        )
        .unwrap()
    {
        Response::UploadStarted(id) => id,
        _ => unreachable!(),
    };
    buffers.0[8..11].copy_from_slice(b"new");
    owner
        .execute(
            Request::UploadWrite {
                upload_id: id,
                offset: 0,
                buffer_offset: 8,
                len: 3,
                buffer: BufferSlot::State,
                power_save: true,
            },
            &mut buffers,
        )
        .unwrap();
    owner
        .execute(Request::upload_commit(id, true), &mut buffers)
        .unwrap();
    assert_eq!(
        owner.execute(
            Request::FileSize {
                name: name("state")
            },
            &mut buffers
        ),
        Ok(Response::FileSize(3))
    );
    assert_eq!(
        owner.execute(
            Request::Delete {
                name: name("state")
            },
            &mut buffers
        ),
        Ok(Response::Ack)
    );

    let abort = match owner
        .execute(
            Request::UploadBegin {
                name: name("temp"),
                expected_size: 1,
                power_save: false,
            },
            &mut buffers,
        )
        .unwrap()
    {
        Response::UploadStarted(id) => id,
        _ => unreachable!(),
    };
    assert_eq!(
        owner.execute(Request::UploadAbort { upload_id: abort }, &mut buffers),
        Ok(Response::Ack)
    );
}

#[test]
fn filesystem_backend_refuses_fake_incremental_scan_support() {
    let mut owner = StorageOwner::new(FilesystemOwnerBackend::new(MemoryFs::default()));
    let mut buffers = Buffers([0; 4096]);
    assert_eq!(
        owner.execute(Request::ScanStart, &mut buffers),
        Err(OwnerError::Backend(
            FilesystemOwnerError::IncrementalScanUnavailable
        ))
    );
}

#[test]
fn mailbox_preserves_client_and_sequence_while_prioritizing_requests() {
    let mut mailbox = StorageMailbox::<4>::new();
    mailbox
        .push(RequestEnvelope {
            client: StorageClient::Library,
            sequence: 10,
            request: Request::scan_step(ScanToken(1)),
        })
        .unwrap();
    mailbox
        .push(RequestEnvelope {
            client: StorageClient::Reader,
            sequence: 11,
            request: Request::read(name("book"), 0, 512, BufferSlot::Reader).unwrap(),
        })
        .unwrap();
    mailbox
        .push(RequestEnvelope {
            client: StorageClient::State,
            sequence: 12,
            request: Request::upload_commit(3, true),
        })
        .unwrap();

    assert_eq!(mailbox.pop().unwrap().sequence, 12);
    assert_eq!(mailbox.pop().unwrap().sequence, 11);
    assert_eq!(mailbox.pop().unwrap().sequence, 10);
}

#[test]
fn completion_routes_to_the_requesting_client_without_losing_correlation() {
    let request = RequestEnvelope {
        client: StorageClient::Diagnostic,
        sequence: 77,
        request: Request::FileSize { name: name("book") },
    };
    let completion = ResponseEnvelope::success(&request, Response::FileSize(123));
    assert_eq!(completion.client, StorageClient::Diagnostic);
    assert_eq!(completion.sequence, 77);
    assert_eq!(completion.response, Ok(Response::FileSize(123)));
    assert!(core::mem::size_of::<RequestEnvelope>() < 176);
}

#[test]
fn owned_request_returns_the_same_caller_buffer_after_success() {
    let mut owner = StorageOwner::new(FakeBackend {
        bytes: *b"0123456789abcdef",
        ..Default::default()
    });
    let bytes = Box::leak(vec![0_u8; 16].into_boxed_slice());
    let original = bytes.as_ptr();
    let completion = owner.execute_owned(OwnedRequest {
        envelope: RequestEnvelope {
            client: StorageClient::Reader,
            sequence: 91,
            request: Request::read(name("book"), 3, 4, BufferSlot::Reader).unwrap(),
        },
        buffer: Some(BufferLease::new(BufferSlot::Reader, bytes)),
    });

    assert_eq!(completion.envelope.sequence, 91);
    assert_eq!(completion.envelope.response, Ok(Response::Read(4)));
    let returned = completion.buffer.unwrap();
    assert_eq!(returned.bytes().as_ptr(), original);
    assert_eq!(&returned.bytes()[..4], b"3456");
}

#[test]
fn owned_request_returns_the_lease_even_when_backend_fails() {
    let mut owner = StorageOwner::new(FakeBackend {
        fail: true,
        ..Default::default()
    });
    let bytes = Box::leak(vec![0_u8; 16].into_boxed_slice());
    let original = bytes.as_ptr();
    let completion = owner.execute_owned(OwnedRequest {
        envelope: RequestEnvelope {
            client: StorageClient::Diagnostic,
            sequence: 92,
            request: Request::read(name("book"), 0, 4, BufferSlot::Diagnostic).unwrap(),
        },
        buffer: Some(BufferLease::new(BufferSlot::Diagnostic, bytes)),
    });

    assert!(completion.envelope.response.is_err());
    assert_eq!(completion.buffer.unwrap().bytes().as_ptr(), original);
}

#[test]
fn unavailable_owner_rejects_requests_without_losing_correlation_or_buffer() {
    let mut bytes = [0xA5; 32];
    let request = OwnedRequest {
        envelope: RequestEnvelope {
            client: StorageClient::Diagnostic,
            sequence: 0x1234_5678,
            request: Request::read(name("book.binbook"), 0, 32, BufferSlot::Diagnostic).unwrap(),
        },
        buffer: Some(BufferLease::new(BufferSlot::Diagnostic, &mut bytes)),
    };

    let response =
        binbook_fw::storage_owner::reject_owned_request(request, CompletionError::Backend);
    assert_eq!(response.envelope.client, StorageClient::Diagnostic);
    assert_eq!(response.envelope.sequence, 0x1234_5678);
    assert_eq!(response.envelope.response, Err(CompletionError::Backend));
    let lease = response.buffer.unwrap();
    assert_eq!(lease.slot(), BufferSlot::Diagnostic);
    assert_eq!(lease.bytes(), &[0xA5; 32]);
}

#[test]
fn owned_mailbox_prioritizes_without_dropping_buffer_ownership() {
    let scan_bytes = Box::leak(vec![0_u8; 8].into_boxed_slice());
    let read_bytes = Box::leak(vec![0_u8; 8].into_boxed_slice());
    let read_ptr = read_bytes.as_ptr();
    let mut mailbox = OwnedMailbox::<2>::new();
    mailbox
        .push(OwnedRequest {
            envelope: RequestEnvelope {
                client: StorageClient::Library,
                sequence: 1,
                request: Request::scan_step(ScanToken(1)),
            },
            buffer: Some(BufferLease::new(BufferSlot::LibraryIndex, scan_bytes)),
        })
        .unwrap();
    mailbox
        .push(OwnedRequest {
            envelope: RequestEnvelope {
                client: StorageClient::Reader,
                sequence: 2,
                request: Request::read(name("book"), 0, 8, BufferSlot::Reader).unwrap(),
            },
            buffer: Some(BufferLease::new(BufferSlot::Reader, read_bytes)),
        })
        .unwrap();

    let first = mailbox.pop().unwrap();
    assert_eq!(first.envelope.sequence, 2);
    assert_eq!(first.buffer.unwrap().bytes().as_ptr(), read_ptr);
    assert_eq!(mailbox.pop().unwrap().envelope.sequence, 1);
}
