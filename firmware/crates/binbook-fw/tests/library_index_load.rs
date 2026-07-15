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
