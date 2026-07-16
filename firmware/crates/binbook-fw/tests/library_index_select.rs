#![cfg(feature = "sd-storage")]

use std::collections::BTreeMap;

use binbook_fw::{
    library::{
        IncrementalLibraryScan, Library, LibraryIndexSlot, INDEX_RECORD_SIZE, INDEX_V2_HEADER_SIZE,
    },
    library_index_select::{LibraryIndexSelectClient, LibraryIndexSelectUpdate},
    storage_access::{LIBRARY_INDEX_FILE, LIBRARY_INDEX_SLOT_A, LIBRARY_INDEX_SLOT_B},
    storage_owner::{CompletionError, Request, Response, ResponseEnvelope, StorageClient},
};

fn indexes() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let mut library = Library::empty();
    let mut scan = IncrementalLibraryScan::begin(&mut library);
    scan.push("alpha.binbook", 1);
    scan.push("beta.binbook", 2);
    scan.finish();
    let mut a = vec![0; library.index_v2_encoded_len()];
    library.encode_index_v2(8, &mut a).unwrap();
    let mut b = vec![0; library.index_v2_encoded_len()];
    library.encode_index_v2(9, &mut b).unwrap();
    let mut legacy = vec![0; library.index_encoded_len()];
    library.encode_index(&mut legacy).unwrap();
    (a, b, legacy)
}

fn select(
    files: BTreeMap<&'static str, Vec<u8>>,
) -> (Option<binbook_fw::library::LibraryIndexCursor>, Vec<String>) {
    let (mut client, mut request) = LibraryIndexSelectClient::begin(1);
    let mut buffer = [0; 4096];
    let mut requested = Vec::new();
    loop {
        let sequence = request.sequence;
        let envelope = match request.request {
            Request::FileSize { ref name } => {
                requested.push(name.as_str().to_owned());
                match files.get(name.as_str()) {
                    Some(bytes) => ResponseEnvelope {
                        client: StorageClient::Library,
                        sequence,
                        response: Ok(Response::FileSize(bytes.len() as u64)),
                    },
                    None => ResponseEnvelope {
                        client: StorageClient::Library,
                        sequence,
                        response: Err(CompletionError::Backend),
                    },
                }
            }
            Request::Read {
                ref name,
                offset,
                len,
                ..
            } => {
                requested.push(name.as_str().to_owned());
                let bytes = &files[name.as_str()];
                let offset = offset as usize;
                let len = usize::from(len);
                buffer[..len].copy_from_slice(&bytes[offset..offset + len]);
                ResponseEnvelope {
                    client: StorageClient::Library,
                    sequence,
                    response: Ok(Response::Read(len as u16)),
                }
            }
            ref other => panic!("unexpected request {other:?}"),
        };
        match client.accept(envelope, Some(&buffer)).unwrap() {
            LibraryIndexSelectUpdate::Request(next) => request = next,
            LibraryIndexSelectUpdate::Complete(cursor) => return (cursor, requested),
        }
    }
}

#[test]
fn selector_chooses_newest_fully_valid_v2_and_never_opens_legacy() {
    let (a, b, legacy) = indexes();
    let (cursor, requested) = select(BTreeMap::from([
        (LIBRARY_INDEX_SLOT_A, a),
        (LIBRARY_INDEX_SLOT_B, b),
        (LIBRARY_INDEX_FILE, legacy),
    ]));
    let cursor = cursor.unwrap();
    assert_eq!(cursor.slot(), LibraryIndexSlot::B);
    assert_eq!(cursor.generation(), 9);
    assert!(!requested.iter().any(|name| name == LIBRARY_INDEX_FILE));
}

#[test]
fn corrupt_newer_slot_falls_back_to_older_and_both_corrupt_use_legacy() {
    let (a, mut b, legacy) = indexes();
    b[INDEX_V2_HEADER_SIZE + INDEX_RECORD_SIZE] ^= 1;
    let (cursor, _) = select(BTreeMap::from([
        (LIBRARY_INDEX_SLOT_A, a.clone()),
        (LIBRARY_INDEX_SLOT_B, b.clone()),
        (LIBRARY_INDEX_FILE, legacy.clone()),
    ]));
    assert_eq!(cursor.unwrap().slot(), LibraryIndexSlot::A);

    let mut corrupt_a = a;
    corrupt_a[16] ^= 1;
    let (cursor, requested) = select(BTreeMap::from([
        (LIBRARY_INDEX_SLOT_A, corrupt_a),
        (LIBRARY_INDEX_SLOT_B, b),
        (LIBRARY_INDEX_FILE, legacy),
    ]));
    assert_eq!(cursor.unwrap().slot(), LibraryIndexSlot::Legacy);
    assert!(requested.iter().any(|name| name == LIBRARY_INDEX_FILE));
}

#[test]
fn missing_or_all_corrupt_slots_produce_no_cursor() {
    let (mut a, _, _) = indexes();
    a.truncate(a.len() - 1);
    let (cursor, _) = select(BTreeMap::from([(LIBRARY_INDEX_SLOT_A, a)]));
    assert_eq!(cursor, None);
}
