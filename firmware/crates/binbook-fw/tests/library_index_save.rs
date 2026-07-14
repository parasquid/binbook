#![cfg(feature = "sd-storage")]

use binbook_fw::{
    library::{IncrementalLibraryScan, Library},
    library_index_save::{LibraryIndexSaveClient, LibraryIndexSaveUpdate},
    storage_owner::{Request, Response, ResponseEnvelope, StorageClient},
};

fn response(sequence: u32, value: Response) -> ResponseEnvelope {
    ResponseEnvelope {
        client: StorageClient::Library,
        sequence,
        response: Ok(value),
    }
}

#[test]
fn index_save_client_streams_maximum_index_through_bounded_writes() {
    let mut library = Library::empty();
    let mut scan = IncrementalLibraryScan::begin(&mut library);
    for index in 0..200 {
        scan.push(&format!("book-{index:03}.binbook"), index as u64);
    }
    scan.finish();
    let mut expected = vec![0; library.index_encoded_len()];
    library.encode_index(&mut expected).unwrap();

    let (mut client, mut request) = LibraryIndexSaveClient::begin(&library, 100);
    let mut scratch = [0u8; 4096];
    let mut written = Vec::new();
    let mut record_writes = 0;
    loop {
        let update = match request.request {
            Request::UploadBegin { expected_size, .. } => {
                assert_eq!(expected_size as usize, expected.len());
                client
                    .accept(
                        response(request.sequence, Response::UploadStarted(77)),
                        &mut scratch,
                    )
                    .unwrap()
            }
            Request::UploadWrite { offset, len, .. } => {
                assert_eq!(offset as usize, written.len());
                assert!(usize::from(len) <= scratch.len());
                written.extend_from_slice(&scratch[..usize::from(len)]);
                if offset != 0 {
                    record_writes += 1;
                    assert!(usize::from(len) <= 512);
                }
                client
                    .accept(
                        response(request.sequence, Response::Written(len)),
                        &mut scratch,
                    )
                    .unwrap()
            }
            Request::UploadCommit { .. } => client
                .accept(response(request.sequence, Response::Ack), &mut scratch)
                .unwrap(),
            _ => panic!("unexpected index-save request"),
        };
        match update {
            LibraryIndexSaveUpdate::Request(next) => request = next,
            LibraryIndexSaveUpdate::Complete => break,
        }
    }
    assert!(record_writes > 1);
    assert_eq!(written, expected);
}
