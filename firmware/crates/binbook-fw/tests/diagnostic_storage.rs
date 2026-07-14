#![cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]

use std::collections::BTreeMap;

use binbook_fw::diag_storage::{FilesystemStorage, StorageHandle};
use binbook_storage::{Filesystem, WritableFilesystem};

const BOOK_A: &[u8] =
    include_bytes!("../../../../crates/binbook-storage/tests/fixtures/book_a.binbook");

#[derive(Default)]
struct MemoryFs {
    files: BTreeMap<String, Vec<u8>>,
    calls: Vec<&'static str>,
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
        self.files.get(name).map(|b| b.len() as u64).ok_or(())
    }
}

impl WritableFilesystem for MemoryFs {
    fn create_or_truncate(&mut self, name: &str) -> Result<(), Self::Error> {
        self.calls.push("create_or_truncate");
        self.files.insert(name.to_string(), Vec::new());
        Ok(())
    }

    fn write_at(&mut self, name: &str, offset: u64, data: &[u8]) -> Result<(), Self::Error> {
        self.calls.push("write_at");
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
        self.calls.push("flush");
        Ok(())
    }

    fn delete_file(&mut self, name: &str) -> Result<(), Self::Error> {
        self.calls.push("delete_file");
        self.files.remove(name).map(|_| ()).ok_or(())
    }

    fn begin_upload(&mut self, name: &str, expected_size: u64) -> Result<(), Self::Error> {
        self.calls.push("begin_upload");
        self.files
            .insert(name.to_string(), Vec::with_capacity(expected_size as usize));
        Ok(())
    }

    fn write_upload_chunk(
        &mut self,
        name: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        self.calls.push("write_upload_chunk");
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

    fn commit_upload(&mut self, _name: &str) -> Result<(), Self::Error> {
        self.calls.push("commit_upload");
        Ok(())
    }

    fn abort_upload(&mut self, name: &str) -> Result<(), Self::Error> {
        self.calls.push("abort_upload");
        self.files.remove(name).map(|_| ()).ok_or(())
    }
}

fn crc32_iso_hdlc(bytes: &[u8]) -> u32 {
    let mut state = 0xFFFF_FFFF;
    for byte in bytes {
        state ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(state & 1);
            state = (state >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    state ^ 0xFFFF_FFFF
}

#[test]
fn filesystem_storage_upload_list_read_delete_roundtrip() {
    let mut storage = FilesystemStorage::new(MemoryFs::default());
    let crc = crc32_iso_hdlc(BOOK_A);

    let upload_id = storage
        .store_upload_begin(
            binbook_diagnostic_protocol::StorageBackend::Sd,
            "book_a.binbook",
            BOOK_A.len() as u32,
            crc,
        )
        .unwrap();
    let split = 17;
    assert_eq!(
        storage
            .store_upload_write(upload_id, 0, &BOOK_A[..split])
            .unwrap(),
        split as u32
    );
    assert_eq!(
        storage
            .store_upload_write(upload_id, split as u32, &BOOK_A[split..])
            .unwrap(),
        (BOOK_A.len() - split) as u32
    );
    assert_eq!(storage.store_upload_commit(upload_id).unwrap(), (true, crc));

    let mut list = [0u8; 256];
    let list_len = storage
        .store_list(binbook_diagnostic_protocol::StorageBackend::Sd, &mut list)
        .unwrap();
    assert_eq!(
        binbook_diagnostic_protocol::decode_store_list_count(&list[..list_len]).unwrap(),
        1
    );
    let entry = binbook_diagnostic_protocol::decode_store_list_entry(&list[2..list_len]).unwrap();
    assert_eq!(entry.name, "book_a.binbook");
    assert_eq!(entry.file_size, BOOK_A.len() as u64);
    assert_eq!(entry.page_count, 1);

    let mut read = [0u8; 32];
    let read_len = storage
        .store_read(
            binbook_diagnostic_protocol::StorageBackend::Sd,
            "book_a.binbook",
            3,
            11,
            &mut read,
        )
        .unwrap();
    let decoded =
        binbook_diagnostic_protocol::decode_store_read_response(&read[..read_len]).unwrap();
    assert_eq!(decoded.data, &BOOK_A[3..14]);
    assert!(!decoded.eof);

    storage
        .store_delete(
            binbook_diagnostic_protocol::StorageBackend::Sd,
            "book_a.binbook",
        )
        .unwrap();
    let list_len = storage
        .store_list(binbook_diagnostic_protocol::StorageBackend::Sd, &mut list)
        .unwrap();
    assert_eq!(
        binbook_diagnostic_protocol::decode_store_list_count(&list[..list_len]).unwrap(),
        0
    );
}

#[test]
fn filesystem_storage_list_returns_the_entries_that_fit_in_one_frame() {
    let mut fs = MemoryFs::default();
    fs.files.insert("a.binbook".into(), BOOK_A.to_vec());
    fs.files.insert("b.binbook".into(), BOOK_A.to_vec());
    let mut storage = FilesystemStorage::new(fs);
    let mut list = [0u8; 28];

    let list_len = storage
        .store_list(binbook_diagnostic_protocol::StorageBackend::Sd, &mut list)
        .expect("a full response frame must remain a valid partial listing");

    assert_eq!(
        binbook_diagnostic_protocol::decode_store_list_count(&list[..list_len]).unwrap(),
        1
    );
    let entry = binbook_diagnostic_protocol::decode_store_list_entry(&list[2..list_len]).unwrap();
    assert_eq!(entry.name, "a.binbook");
}

#[test]
fn filesystem_storage_reports_crc_mismatch_without_listing_corrupt_upload() {
    let mut storage = FilesystemStorage::new(MemoryFs::default());
    let upload_id = storage
        .store_upload_begin(
            binbook_diagnostic_protocol::StorageBackend::Sd,
            "bad.binbook",
            4,
            0xDEAD_BEEF,
        )
        .unwrap();
    storage.store_upload_write(upload_id, 0, b"oops").unwrap();
    let (matches, computed) = storage.store_upload_commit(upload_id).unwrap();
    assert!(!matches);
    assert_ne!(computed, 0xDEAD_BEEF);

    let mut list = [0u8; 64];
    let list_len = storage
        .store_list(binbook_diagnostic_protocol::StorageBackend::Sd, &mut list)
        .unwrap();
    assert_eq!(
        binbook_diagnostic_protocol::decode_store_list_count(&list[..list_len]).unwrap(),
        0
    );
}

#[test]
fn filesystem_storage_rejects_non_sequential_upload_offsets() {
    let mut storage = FilesystemStorage::new(MemoryFs::default());
    let upload_id = storage
        .store_upload_begin(
            binbook_diagnostic_protocol::StorageBackend::Sd,
            "book_a.binbook",
            BOOK_A.len() as u32,
            crc32_iso_hdlc(BOOK_A),
        )
        .unwrap();

    assert_eq!(
        storage.store_upload_write(upload_id, 1, &BOOK_A[..4]),
        Err(binbook_fw::diag_storage::StorageError::BadRequest)
    );
    storage
        .store_upload_abort(upload_id)
        .expect("abort after rejected write");
}

#[test]
fn filesystem_storage_upload_uses_streaming_hooks() {
    let mut storage = FilesystemStorage::new(MemoryFs::default());
    let payload = &BOOK_A[..64];
    let crc = crc32_iso_hdlc(payload);

    let upload_id = storage
        .store_upload_begin(
            binbook_diagnostic_protocol::StorageBackend::Sd,
            "book_a.binbook",
            payload.len() as u32,
            crc,
        )
        .unwrap();
    storage
        .store_upload_write(upload_id, 0, &payload[..32])
        .unwrap();
    storage
        .store_upload_write(upload_id, 32, &payload[32..])
        .unwrap();
    assert_eq!(storage.store_upload_commit(upload_id).unwrap(), (true, crc));

    assert_eq!(
        storage.into_inner().calls,
        [
            "begin_upload",
            "write_upload_chunk",
            "write_upload_chunk",
            "commit_upload"
        ]
    );
}
