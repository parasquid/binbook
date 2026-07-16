#![cfg(feature = "sd-storage")]
#![allow(clippy::drop_non_drop)]

use binbook_fw::{
    library::{IncrementalLibraryScan, Library, LibraryStatus, LibraryViewport},
    library_index_load::{
        LibraryIndexLoadClient, LibraryIndexLoadError, LibraryIndexLoadUpdate,
        LibraryIndexPruneClient, LibraryIndexPruneUpdate, LibraryViewportLoadClient,
        LibraryViewportWindowClient,
    },
    reader_state::{ProgressRecord, ReaderState},
    storage_owner::{CompletionError, Request, Response, ResponseEnvelope, StorageClient},
};

fn response(sequence: u32, value: Response) -> ResponseEnvelope {
    ResponseEnvelope {
        client: StorageClient::Library,
        sequence,
        response: Ok(value),
    }
}

fn maximum_index() -> (Library, Vec<u8>) {
    let mut library = Library::empty();
    let mut builder = IncrementalLibraryScan::begin(&mut library);
    for index in 0..200 {
        builder.push(&format!("book-{index:03}.binbook"), index as u64);
    }
    builder.finish();
    let mut bytes = vec![0; library.index_encoded_len()];
    library.encode_index(&mut bytes).unwrap();
    (library, bytes)
}

#[test]
fn index_client_streams_maximum_index_through_bounded_reads() {
    let (expected, encoded) = maximum_index();
    let mut loaded = Library::empty();
    let (mut client, mut request) = LibraryIndexLoadClient::begin(&mut loaded, 100);
    let mut scratch = vec![0u8; 4096];
    loop {
        let update = match request.request {
            Request::FileSize { .. } => client
                .accept(
                    response(request.sequence, Response::FileSize(encoded.len() as u64)),
                    None,
                )
                .unwrap(),
            Request::Read { offset, len, .. } => {
                assert!(usize::from(len) <= 4096);
                scratch.fill(0);
                let start = offset as usize;
                scratch[..usize::from(len)]
                    .copy_from_slice(&encoded[start..start + usize::from(len)]);
                client
                    .accept(
                        response(request.sequence, Response::Read(len)),
                        Some(&scratch),
                    )
                    .unwrap()
            }
            _ => panic!("unexpected index request"),
        };
        match update {
            LibraryIndexLoadUpdate::Request(next) => request = next,
            LibraryIndexLoadUpdate::Complete(status) => {
                assert_eq!(status, LibraryStatus::Ready);
                break;
            }
        }
    }
    drop(client);
    assert_eq!(loaded, expected);
}

#[test]
fn viewport_client_validates_200_records_but_retains_only_15_requested_entries() {
    let (_, encoded) = maximum_index();
    let mut viewport = LibraryViewport::empty();
    let (mut client, mut request) = LibraryViewportLoadClient::begin(&mut viewport, 93, 500);
    let mut scratch = vec![0u8; 4096];
    loop {
        let update = match request.request {
            Request::FileSize { .. } => client
                .accept(
                    response(request.sequence, Response::FileSize(encoded.len() as u64)),
                    None,
                )
                .unwrap(),
            Request::Read { offset, len, .. } => {
                assert!(usize::from(len) <= 4096);
                let start = offset as usize;
                scratch[..usize::from(len)]
                    .copy_from_slice(&encoded[start..start + usize::from(len)]);
                client
                    .accept(
                        response(request.sequence, Response::Read(len)),
                        Some(&scratch),
                    )
                    .unwrap()
            }
            _ => panic!("unexpected viewport request"),
        };
        match update {
            LibraryIndexLoadUpdate::Request(next) => request = next,
            LibraryIndexLoadUpdate::Complete(status) => {
                assert_eq!(status, LibraryStatus::Ready);
                break;
            }
        }
    }
    drop(client);
    assert!(viewport.is_valid());
    assert_eq!(viewport.total(), 200);
    assert_eq!(viewport.start(), 93);
    assert_eq!(viewport.entries().len(), 15);
    assert_eq!(viewport.entries()[0].name.as_str(), "book-093.binbook");
    assert_eq!(viewport.entries()[14].name.as_str(), "book-107.binbook");
    let viewport_bytes = core::mem::size_of::<LibraryViewport>();
    assert!(
        viewport_bytes <= 2440,
        "library viewport uses {viewport_bytes} bytes"
    );

    let cursor = viewport.cursor().expect("validated cursor retained");
    let (mut window, request) =
        LibraryViewportWindowClient::begin(&mut viewport, cursor, 185, 900).unwrap();
    let Request::Read { offset, len, .. } = request.request else {
        panic!("one bounded record-window read required")
    };
    assert_eq!(usize::from(len), 15 * 139);
    scratch[..usize::from(len)]
        .copy_from_slice(&encoded[offset as usize..offset as usize + usize::from(len)]);
    assert_eq!(
        window
            .accept(response(request.sequence, Response::Read(len)), &scratch,)
            .unwrap(),
        LibraryIndexLoadUpdate::Complete(LibraryStatus::Ready)
    );
    drop(window);
    assert_eq!(viewport.start(), 185);
    assert_eq!(viewport.total(), 200);
    assert_eq!(viewport.entries()[0].name.as_str(), "book-185.binbook");
    assert_eq!(viewport.entries()[14].name.as_str(), "book-199.binbook");
}

#[test]
fn viewport_window_uses_the_cursor_exact_slot_without_resolving_latest_again() {
    use binbook_fw::{
        library::{LibraryIndexCursor, LibraryIndexSlot, INDEX_V2_HEADER_SIZE},
        storage_access::LIBRARY_INDEX_SLOT_A,
    };

    let (library, _) = maximum_index();
    let mut encoded = vec![0; library.index_v2_encoded_len()];
    library.encode_index_v2(4, &mut encoded).unwrap();
    let cursor = LibraryIndexCursor::from_v2_header(
        LibraryIndexSlot::A,
        encoded[..INDEX_V2_HEADER_SIZE].try_into().unwrap(),
        encoded.len(),
    )
    .unwrap();
    let mut viewport = LibraryViewport::empty();
    let (mut client, request) =
        LibraryViewportWindowClient::begin(&mut viewport, cursor, 30, 777).unwrap();
    let Request::Read {
        name, offset, len, ..
    } = request.request
    else {
        panic!("window read required")
    };
    assert_eq!(name.as_str(), LIBRARY_INDEX_SLOT_A);
    let mut scratch = vec![0; usize::from(len)];
    scratch.copy_from_slice(&encoded[offset as usize..offset as usize + usize::from(len)]);
    client
        .accept(response(777, Response::Read(len)), &scratch)
        .unwrap();
    drop(client);
    assert_eq!(viewport.start(), 30);
    assert_eq!(viewport.cursor(), Some(cursor));
}

#[test]
fn exact_cursor_navigation_reaches_every_record_forward_and_backward() {
    use binbook_fw::library::{
        LibraryIndexCursor, LibraryIndexSlot, INDEX_V2_HEADER_SIZE, LIBRARY_VIEWPORT_CAPACITY,
    };

    for total in [20usize, 21, 200] {
        let mut library = Library::empty();
        let mut scan = IncrementalLibraryScan::begin(&mut library);
        for index in 0..total {
            scan.push(&format!("nav-{index:03}.binbook"), index as u64);
        }
        scan.finish();
        let mut encoded = vec![0; library.index_v2_encoded_len()];
        library.encode_index_v2(3, &mut encoded).unwrap();
        let cursor = LibraryIndexCursor::from_v2_header(
            LibraryIndexSlot::B,
            encoded[..INDEX_V2_HEADER_SIZE].try_into().unwrap(),
            encoded.len(),
        )
        .unwrap();
        let mut viewport = LibraryViewport::empty();
        let mut sequence = 1u32;
        let mut visited_forward = Vec::new();
        for start in (0..total).step_by(LIBRARY_VIEWPORT_CAPACITY) {
            load_exact_window(&mut viewport, cursor, start, sequence, &encoded);
            sequence += 1;
            visited_forward.extend(
                viewport
                    .entries()
                    .iter()
                    .map(|entry| entry.name.as_str().to_owned()),
            );
        }
        let expected: Vec<_> = (0..total)
            .map(|index| format!("nav-{index:03}.binbook"))
            .collect();
        assert_eq!(visited_forward, expected);

        let mut visited_backward = Vec::new();
        let mut starts: Vec<_> = (0..total).step_by(LIBRARY_VIEWPORT_CAPACITY).collect();
        starts.reverse();
        for start in starts {
            load_exact_window(&mut viewport, cursor, start, sequence, &encoded);
            sequence += 1;
            visited_backward.extend(
                viewport
                    .entries()
                    .iter()
                    .rev()
                    .map(|entry| entry.name.as_str().to_owned()),
            );
        }
        let mut reversed = expected;
        reversed.reverse();
        assert_eq!(visited_backward, reversed);
    }
}

fn load_exact_window(
    viewport: &mut LibraryViewport,
    cursor: binbook_fw::library::LibraryIndexCursor,
    start: usize,
    sequence: u32,
    encoded: &[u8],
) {
    let (mut client, request) =
        LibraryViewportWindowClient::begin(viewport, cursor, start, sequence).unwrap();
    let Request::Read { offset, len, .. } = request.request else {
        panic!("window read required")
    };
    let bytes = &encoded[offset as usize..offset as usize + usize::from(len)];
    client
        .accept(response(sequence, Response::Read(len)), bytes)
        .unwrap();
}

#[test]
fn missing_or_wrong_sequence_index_is_non_adoptable() {
    let mut loaded = Library::empty();
    let (mut client, _) = LibraryIndexLoadClient::begin(&mut loaded, 7);
    assert_eq!(
        client.accept(
            ResponseEnvelope {
                client: StorageClient::Library,
                sequence: 7,
                response: Err(CompletionError::Backend),
            },
            None,
        ),
        Err(LibraryIndexLoadError::Completion)
    );
    drop(client);
    assert_eq!(loaded.status(), LibraryStatus::CardError);

    let mut loaded = Library::empty();
    let (mut client, _) = LibraryIndexLoadClient::begin(&mut loaded, 9);
    assert_eq!(
        client.accept(response(10, Response::FileSize(16)), None),
        Err(LibraryIndexLoadError::Correlation)
    );
    drop(client);
    assert_eq!(loaded.status(), LibraryStatus::CardError);
}

#[test]
fn corrupt_streamed_header_is_non_adoptable() {
    let (_, encoded) = maximum_index();
    let mut loaded = Library::empty();
    let (mut client, _) = LibraryIndexLoadClient::begin(&mut loaded, 20);
    let next = client
        .accept(response(20, Response::FileSize(encoded.len() as u64)), None)
        .unwrap();
    let LibraryIndexLoadUpdate::Request(header) = next else {
        panic!("header read required")
    };
    let mut bytes = encoded[..16].to_vec();
    bytes[0] ^= 1;
    assert_eq!(
        client.accept(response(header.sequence, Response::Read(16)), Some(&bytes)),
        Err(LibraryIndexLoadError::Decode(
            binbook_fw::library::LibraryIndexError::Magic
        ))
    );
    drop(client);
    assert_eq!(loaded.status(), LibraryStatus::CardError);
}

#[test]
fn prune_client_validates_all_200_index_records_before_removing_deleted_progress() {
    let (library, encoded) = maximum_index();
    let mut state = ReaderState::empty();
    let rendition = [0x55; 32];
    for (filename_hash, page) in [
        (library.entries()[1].filename_hash, 1),
        (library.entries()[199].filename_hash, 2),
        (0xdead_beef, 3),
    ] {
        state
            .set_progress(ProgressRecord {
                filename_hash,
                rendition_hash: rendition,
                page,
            })
            .unwrap();
    }

    let (mut client, mut request) = LibraryIndexPruneClient::begin(&mut state, 1_000);
    let mut scratch = vec![0u8; 4096];
    let removed = loop {
        let update = match request.request {
            Request::FileSize { .. } => client
                .accept(
                    response(request.sequence, Response::FileSize(encoded.len() as u64)),
                    None,
                )
                .unwrap(),
            Request::Read { offset, len, .. } => {
                let start = offset as usize;
                scratch[..usize::from(len)]
                    .copy_from_slice(&encoded[start..start + usize::from(len)]);
                client
                    .accept(
                        response(request.sequence, Response::Read(len)),
                        Some(&scratch),
                    )
                    .unwrap()
            }
            _ => panic!("unexpected prune request"),
        };
        match update {
            LibraryIndexPruneUpdate::Request(next) => request = next,
            LibraryIndexPruneUpdate::Complete { removed } => break removed,
        }
    };
    drop(client);

    assert_eq!(removed, 1);
    assert_eq!(state.records().len(), 2);
    assert!(state
        .records()
        .iter()
        .all(|record| record.filename_hash != 0xdead_beef));
}

#[test]
fn prune_client_does_not_mutate_state_when_index_validation_fails() {
    let (library, mut encoded) = maximum_index();
    let mut state = ReaderState::empty();
    state
        .set_progress(ProgressRecord {
            filename_hash: library.entries()[1].filename_hash,
            rendition_hash: [0x66; 32],
            page: 4,
        })
        .unwrap();
    state
        .set_progress(ProgressRecord {
            filename_hash: 0xdead_beef,
            rendition_hash: [0x77; 32],
            page: 5,
        })
        .unwrap();
    let last = encoded.len() - 1;
    encoded[last] ^= 1;

    let (mut client, mut request) = LibraryIndexPruneClient::begin(&mut state, 2_000);
    let mut scratch = vec![0u8; 4096];
    loop {
        let result = match request.request {
            Request::FileSize { .. } => client.accept(
                response(request.sequence, Response::FileSize(encoded.len() as u64)),
                None,
            ),
            Request::Read { offset, len, .. } => {
                let start = offset as usize;
                scratch[..usize::from(len)]
                    .copy_from_slice(&encoded[start..start + usize::from(len)]);
                client.accept(
                    response(request.sequence, Response::Read(len)),
                    Some(&scratch),
                )
            }
            _ => panic!("unexpected prune request"),
        };
        match result {
            Ok(LibraryIndexPruneUpdate::Request(next)) => request = next,
            Ok(LibraryIndexPruneUpdate::Complete { .. }) => panic!("corrupt index was accepted"),
            Err(LibraryIndexLoadError::Decode(_)) => break,
            Err(error) => panic!("unexpected error: {error:?}"),
        }
    }
    drop(client);

    assert_eq!(state.records().len(), 2);
    assert!(state
        .records()
        .iter()
        .any(|record| record.filename_hash == 0xdead_beef));
}

#[test]
fn prune_uses_the_exact_version_two_cursor_slot() {
    use binbook_fw::{
        library::{LibraryIndexCursor, LibraryIndexSlot, INDEX_V2_HEADER_SIZE},
        storage_access::LIBRARY_INDEX_SLOT_B,
    };

    let (library, _) = maximum_index();
    let mut encoded = vec![0; library.index_v2_encoded_len()];
    library.encode_index_v2(44, &mut encoded).unwrap();
    let cursor = LibraryIndexCursor::from_v2_header(
        LibraryIndexSlot::B,
        encoded[..INDEX_V2_HEADER_SIZE].try_into().unwrap(),
        encoded.len(),
    )
    .unwrap();
    let mut state = ReaderState::empty();
    for hash in [library.entries()[9].filename_hash, 0xfeed_face] {
        state
            .set_progress(ProgressRecord {
                filename_hash: hash,
                rendition_hash: [9; 32],
                page: 1,
            })
            .unwrap();
    }
    let (mut client, mut request) = LibraryIndexPruneClient::begin_cursor(&mut state, cursor, 80);
    let mut scratch = vec![0; 4096];
    loop {
        let Request::Read {
            ref name,
            offset,
            len,
            ..
        } = request.request
        else {
            panic!("cursor prune only reads its selected slot")
        };
        assert_eq!(name.as_str(), LIBRARY_INDEX_SLOT_B);
        let offset = offset as usize;
        let len = usize::from(len);
        scratch[..len].copy_from_slice(&encoded[offset..offset + len]);
        match client
            .accept(
                response(request.sequence, Response::Read(len as u16)),
                Some(&scratch),
            )
            .unwrap()
        {
            LibraryIndexPruneUpdate::Request(next) => request = next,
            LibraryIndexPruneUpdate::Complete { removed } => {
                assert_eq!(removed, 1);
                break;
            }
        }
    }
    drop(client);
    assert_eq!(state.records().len(), 1);
    assert_eq!(
        state.records()[0].filename_hash,
        library.entries()[9].filename_hash
    );
}
