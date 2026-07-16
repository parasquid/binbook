#![cfg(feature = "sd-storage")]

use binbook_fw::{
    library::{
        IncrementalLibraryScan, Library, LibraryEntry, LibraryIndexSlot, MountFileToken,
        LIBRARY_VIEWPORT_CAPACITY,
    },
    library_external_sort::{LibraryRefreshWorkspace, MAX_RUN_BYTES},
    library_publication::{
        LibraryPublicationClient, LibraryPublicationError, LibraryPublicationPhase,
        LibraryPublicationUpdate,
    },
    reader_state::filename_hash,
    storage_owner::{Request, Response, ResponseEnvelope, StorageClient},
};

#[cfg(not(feature = "library-index-fault-after-first-record"))]
use binbook_fw::{
    library::{select_library_index_cursor, validate_library_index_bytes},
    library_external_sort::SPOOL_RECORD_SIZE,
    storage_owner::CompletionError,
};

fn response(sequence: u32, response: Response) -> ResponseEnvelope {
    ResponseEnvelope {
        client: StorageClient::Library,
        sequence,
        response: Ok(response),
    }
}

fn fixture(count: usize) -> (LibraryRefreshWorkspace, Vec<u8>, Vec<u8>) {
    let mut workspace = LibraryRefreshWorkspace::new();
    let mut spool = Vec::new();
    let mut run = [0; MAX_RUN_BYTES];
    for index in (0..count).rev() {
        if workspace.buffered_count() == LIBRARY_VIEWPORT_CAPACITY {
            let used = workspace.flush_run(&mut run).unwrap();
            spool.extend_from_slice(&run[..used]);
        }
        let name = format!("book-{index:03}.binbook");
        workspace
            .push(LibraryEntry {
                name: name.as_str().try_into().unwrap(),
                size: index as u64 * 31,
                filename_hash: filename_hash(name.as_bytes()),
                mount_token: (index < LIBRARY_VIEWPORT_CAPACITY)
                    .then(|| MountFileToken::new(index as u16 + 1).unwrap()),
            })
            .unwrap();
    }
    let used = workspace.flush_run(&mut run).unwrap();
    spool.extend_from_slice(&run[..used]);

    let mut library = Library::empty();
    let mut scan = IncrementalLibraryScan::begin(&mut library);
    for index in 0..count {
        scan.push(&format!("book-{index:03}.binbook"), index as u64 * 31);
    }
    scan.finish();
    let mut expected = vec![0; library.index_v2_encoded_len()];
    library.encode_index_v2(17, &mut expected).unwrap();
    (workspace, spool, expected)
}

#[cfg(not(feature = "library-index-fault-after-first-record"))]
fn drive(
    count: usize,
    corrupt_after_commit: bool,
) -> Result<(Vec<u8>, binbook_fw::library_publication::LibraryPublication), LibraryPublicationError>
{
    let (mut workspace, mut scratch, expected) = fixture(count);
    assert_eq!(scratch.len(), count * SPOOL_RECORD_SIZE);
    let (mut client, mut request) =
        LibraryPublicationClient::begin(&mut workspace, LibraryIndexSlot::B, 17, 100).unwrap();
    let mut candidate = Vec::new();
    let mut buffer = [0; 4096];
    let mut committed = false;
    let publication = loop {
        let sequence = request.sequence;
        let response_value = match request.request {
            Request::UploadBegin { expected_size, .. } => {
                candidate.resize(expected_size as usize, 0);
                Response::UploadStarted(9)
            }
            Request::UploadWrite {
                offset,
                buffer_offset,
                len,
                ..
            } => {
                let offset = offset as usize;
                let source = usize::from(buffer_offset);
                let len = usize::from(len);
                candidate[offset..offset + len].copy_from_slice(&buffer[source..source + len]);
                Response::Written(len as u16)
            }
            Request::LibraryScratchRead {
                offset,
                buffer_offset,
                len,
                ..
            } => {
                let offset = offset as usize;
                let target = usize::from(buffer_offset);
                let len = usize::from(len);
                buffer[target..target + len].copy_from_slice(&scratch[offset..offset + len]);
                Response::Read(len as u16)
            }
            Request::UploadCommit { .. } => {
                committed = true;
                if corrupt_after_commit && !candidate.is_empty() {
                    let last = candidate.len() - 1;
                    candidate[last] ^= 1;
                }
                Response::Ack
            }
            Request::FileSize { .. } => {
                assert!(committed);
                Response::FileSize(candidate.len() as u64)
            }
            Request::Read {
                offset,
                buffer_offset,
                len,
                ..
            } => {
                let offset = offset as usize;
                let target = usize::from(buffer_offset);
                let len = usize::from(len);
                buffer[target..target + len].copy_from_slice(&candidate[offset..offset + len]);
                Response::Read(len as u16)
            }
            Request::LibraryScratchDelete => {
                scratch.clear();
                Response::Ack
            }
            ref other => panic!("unexpected request {other:?}"),
        };
        match client.accept(response(sequence, response_value), Some(&mut buffer))? {
            LibraryPublicationUpdate::Request(next) => request = next,
            LibraryPublicationUpdate::Complete(publication) => break publication,
        }
    };
    drop(client);
    assert!(scratch.is_empty());
    if !corrupt_after_commit {
        assert_eq!(candidate, expected);
    }
    Ok((candidate, publication))
}

#[test]
#[cfg(not(feature = "library-index-fault-after-first-record"))]
fn merge_publication_is_byte_exact_and_readback_validated_at_all_boundaries() {
    for count in [0, 1, 15, 16, 20, 21, 200] {
        let (_, publication) = drive(count, false).unwrap();
        let cursor = publication.cursor.unwrap();
        assert_eq!(cursor.slot(), LibraryIndexSlot::B);
        assert_eq!(cursor.generation(), 17);
        assert_eq!(cursor.entry_count(), count);
        assert_eq!(publication.book_count, count as u16);
        assert_eq!(publication.prepared.len(), count.min(15));
        for (absolute, prepared) in publication.prepared.iter().enumerate() {
            assert_eq!(prepared.absolute_index, absolute as u16);
            assert_eq!(prepared.token.get(), absolute as u16 + 1);
        }
    }
}

#[test]
#[cfg(not(feature = "library-index-fault-after-first-record"))]
fn corrupt_committed_candidate_is_not_published_or_cleaned_up_as_success() {
    assert!(matches!(
        drive(21, true),
        Err(LibraryPublicationError::Codec(_))
    ));
}

#[test]
fn publication_exposes_the_exact_failed_phase_before_cleanup() {
    let (mut workspace, _, _) = fixture(1);
    let (mut client, request) =
        LibraryPublicationClient::begin(&mut workspace, LibraryIndexSlot::B, 17, 100).unwrap();
    assert!(LibraryPublicationClient::request_needs_buffer(
        &request.request
    ));
    assert_eq!(client.phase(), LibraryPublicationPhase::Begin);

    let error = client
        .accept(response(100, Response::UploadStarted(9)), None)
        .unwrap_err();
    assert_eq!(error, LibraryPublicationError::MissingBuffer);
    assert_eq!(error.code(), 3);
    assert_eq!(client.phase(), LibraryPublicationPhase::ProvisionalHeader);
    assert!(matches!(
        client.cleanup().unwrap().request,
        Request::UploadAbort { .. }
    ));
}

#[test]
#[cfg(not(feature = "library-index-fault-after-first-record"))]
fn every_prepublication_request_failure_cleans_up_and_keeps_a_valid_authority() {
    let successful_request_count = fault_publication(None).0;
    assert!(successful_request_count > 20);
    for fault_at in 0..successful_request_count {
        let (_, selected, scratch_empty, upload_inactive) = fault_publication(Some(fault_at));
        assert!(scratch_empty, "fault {fault_at} leaked scratch");
        assert!(upload_inactive, "fault {fault_at} leaked an upload session");
        assert!(matches!(
            selected.map(|cursor| cursor.slot()),
            Some(LibraryIndexSlot::A | LibraryIndexSlot::B)
        ));
    }
}

#[test]
#[cfg(feature = "library-index-fault-after-first-record")]
fn injected_fault_occurs_after_exactly_one_candidate_record_and_aborts_cleanly() {
    let (mut workspace, mut scratch, _) = fixture(21);
    let (mut client, mut request) =
        LibraryPublicationClient::begin(&mut workspace, LibraryIndexSlot::B, 2, 1).unwrap();
    let mut candidate = Vec::new();
    let mut buffer = [0; 4096];
    let mut record_writes = 0usize;
    loop {
        let response_value = match request.request {
            Request::UploadBegin { expected_size, .. } => {
                candidate.resize(expected_size as usize, 0);
                Response::UploadStarted(7)
            }
            Request::UploadWrite {
                offset,
                buffer_offset,
                len,
                ..
            } => {
                if offset >= binbook_fw::library::INDEX_V2_HEADER_SIZE as u64 {
                    record_writes += 1;
                }
                let offset = offset as usize;
                let source = usize::from(buffer_offset);
                let len = usize::from(len);
                candidate[offset..offset + len].copy_from_slice(&buffer[source..source + len]);
                Response::Written(len as u16)
            }
            Request::LibraryScratchRead {
                offset,
                buffer_offset,
                len,
                ..
            } => {
                let offset = offset as usize;
                let target = usize::from(buffer_offset);
                let len = usize::from(len);
                buffer[target..target + len].copy_from_slice(&scratch[offset..offset + len]);
                Response::Read(len as u16)
            }
            ref other => panic!("unexpected request before injected fault: {other:?}"),
        };
        match client.accept(
            response(request.sequence, response_value),
            Some(&mut buffer),
        ) {
            Ok(LibraryPublicationUpdate::Request(next)) => request = next,
            Err(LibraryPublicationError::InjectedFault) => break,
            other => panic!("unexpected publication result {other:?}"),
        }
    }
    assert_eq!(record_writes, 1);
    let abort = client.cleanup().unwrap();
    assert!(matches!(abort.request, Request::UploadAbort { .. }));
    let next = client
        .accept(response(abort.sequence, Response::Ack), Some(&mut buffer))
        .unwrap();
    let LibraryPublicationUpdate::Request(delete) = next else {
        panic!("scratch cleanup required")
    };
    assert_eq!(delete.request, Request::LibraryScratchDelete);
    scratch.clear();
    assert!(client
        .accept(response(delete.sequence, Response::Ack), Some(&mut buffer))
        .is_err());
    assert!(scratch.is_empty());
}

#[cfg(not(feature = "library-index-fault-after-first-record"))]
fn fault_publication(
    fault_at: Option<usize>,
) -> (
    usize,
    Option<binbook_fw::library::LibraryIndexCursor>,
    bool,
    bool,
) {
    let (mut workspace, mut scratch, _) = fixture(21);
    let mut old_library = Library::empty();
    let mut old_scan = IncrementalLibraryScan::begin(&mut old_library);
    old_scan.push("old.binbook", 1);
    old_scan.finish();
    let mut old_bytes = vec![0; old_library.index_v2_encoded_len()];
    old_library.encode_index_v2(16, &mut old_bytes).unwrap();
    let old = validate_library_index_bytes(LibraryIndexSlot::A, &old_bytes).unwrap();

    let (mut client, mut request) =
        LibraryPublicationClient::begin(&mut workspace, LibraryIndexSlot::B, 17, 1).unwrap();
    let mut buffer = [0; 4096];
    let mut candidate = Vec::new();
    let mut upload_active = false;
    let mut committed = false;
    let mut request_index = 0usize;
    loop {
        let sequence = request.sequence;
        if fault_at == Some(request_index) {
            let failed = ResponseEnvelope {
                client: StorageClient::Library,
                sequence,
                response: Err(CompletionError::Backend),
            };
            assert!(client.accept(failed, Some(&mut buffer)).is_err());
            let mut cleanup = client.cleanup();
            while let Some(cleanup_request) = cleanup {
                let response_value = match cleanup_request.request {
                    Request::UploadAbort { .. } => {
                        upload_active = false;
                        if !committed {
                            candidate.clear();
                        }
                        Response::Ack
                    }
                    Request::LibraryScratchDelete => {
                        scratch.clear();
                        Response::Ack
                    }
                    ref other => panic!("unexpected cleanup request {other:?}"),
                };
                cleanup = match client.accept(
                    response(cleanup_request.sequence, response_value),
                    Some(&mut buffer),
                ) {
                    Ok(LibraryPublicationUpdate::Request(next)) => Some(next),
                    _ => None,
                };
            }
            break;
        }
        request_index += 1;
        let response_value = match request.request {
            Request::UploadBegin { expected_size, .. } => {
                upload_active = true;
                candidate.resize(expected_size as usize, 0);
                Response::UploadStarted(3)
            }
            Request::UploadWrite {
                offset,
                buffer_offset,
                len,
                ..
            } => {
                let offset = offset as usize;
                let source = usize::from(buffer_offset);
                let len = usize::from(len);
                candidate[offset..offset + len].copy_from_slice(&buffer[source..source + len]);
                Response::Written(len as u16)
            }
            Request::LibraryScratchRead {
                offset,
                buffer_offset,
                len,
                ..
            } => {
                let offset = offset as usize;
                let target = usize::from(buffer_offset);
                let len = usize::from(len);
                buffer[target..target + len].copy_from_slice(&scratch[offset..offset + len]);
                Response::Read(len as u16)
            }
            Request::UploadCommit { .. } => {
                upload_active = false;
                committed = true;
                Response::Ack
            }
            Request::FileSize { .. } => Response::FileSize(candidate.len() as u64),
            Request::Read {
                offset,
                buffer_offset,
                len,
                ..
            } => {
                let offset = offset as usize;
                let target = usize::from(buffer_offset);
                let len = usize::from(len);
                buffer[target..target + len].copy_from_slice(&candidate[offset..offset + len]);
                Response::Read(len as u16)
            }
            Request::LibraryScratchDelete => {
                scratch.clear();
                Response::Ack
            }
            ref other => panic!("unexpected request {other:?}"),
        };
        match client
            .accept(response(sequence, response_value), Some(&mut buffer))
            .unwrap()
        {
            LibraryPublicationUpdate::Request(next) => request = next,
            LibraryPublicationUpdate::Complete(_) => break,
        }
    }
    drop(client);
    let candidate_cursor = committed
        .then(|| validate_library_index_bytes(LibraryIndexSlot::B, &candidate).ok())
        .flatten();
    (
        request_index,
        select_library_index_cursor(Some(old), candidate_cursor, None),
        scratch.is_empty(),
        !upload_active,
    )
}
