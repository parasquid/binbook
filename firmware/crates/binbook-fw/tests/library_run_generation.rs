#![cfg(feature = "sd-storage")]

use binbook_fw::{
    library::{LibraryStatus, MountFileToken},
    library_external_sort::{LibraryRefreshWorkspace, SPOOL_RECORD_SIZE},
    library_run_generation::{LibraryRunGenerationClient, LibraryRunGenerationUpdate},
    storage_owner::{
        encode_scan_records, CompletionError, Request, Response, ResponseEnvelope, ScanProgress,
        ScanRecord, ScanToken, StorageClient, StorageName,
    },
};

fn response(sequence: u32, response: Response) -> ResponseEnvelope {
    ResponseEnvelope {
        client: StorageClient::Library,
        sequence,
        response: Ok(response),
    }
}

fn drive(count: usize) -> (LibraryStatus, LibraryRefreshWorkspace, Vec<u8>) {
    let mut workspace = LibraryRefreshWorkspace::new();
    let (mut client, mut request) =
        LibraryRunGenerationClient::begin(&mut workspace, &[], 10).unwrap();
    let mut buffer = [0; 4096];
    let mut scratch = Vec::new();
    let mut scan_offset = 0usize;
    let status = loop {
        let sequence = request.sequence;
        let response_value = match request.request {
            Request::LibraryScratchCreate => {
                scratch.clear();
                Response::Ack
            }
            Request::ScanStart => Response::ScanStarted(ScanToken(7)),
            Request::ScanStep { .. } => {
                let end = (scan_offset + 8).min(count);
                let records: Vec<_> = (scan_offset..end)
                    .rev()
                    .map(|index| {
                        let name = format!("book-{index:03}.binbook");
                        ScanRecord {
                            name: StorageName::new(&name).unwrap(),
                            size: index as u64,
                            mount_token: MountFileToken::new(index as u16 + 1),
                        }
                    })
                    .collect();
                encode_scan_records(&records, &mut buffer).unwrap();
                scan_offset = end;
                Response::ScanProgress(ScanProgress {
                    entries: records.len() as u8,
                    complete: scan_offset == count,
                    unsupported_name: false,
                })
            }
            Request::LibraryScratchWrite {
                offset,
                buffer_offset,
                len,
                ..
            } => {
                let offset = offset as usize;
                let source = usize::from(buffer_offset);
                let len = usize::from(len);
                scratch.resize(scratch.len().max(offset + len), 0);
                scratch[offset..offset + len].copy_from_slice(&buffer[source..source + len]);
                Response::Written(len as u16)
            }
            Request::ScanFinish { .. }
            | Request::LibraryScratchFlush
            | Request::ScanCancel { .. } => Response::Ack,
            Request::LibraryScratchDelete => {
                scratch.clear();
                Response::Ack
            }
            Request::UploadBegin { .. } | Request::UploadWrite { .. } => {
                panic!("run generation must not publish a candidate")
            }
            ref other => panic!("unexpected request {other:?}"),
        };
        match client
            .accept(response(sequence, response_value), Some(&mut buffer))
            .unwrap()
        {
            LibraryRunGenerationUpdate::Request(next) => request = next,
            LibraryRunGenerationUpdate::Complete(status) => break status,
        }
    };
    drop(client);
    (status, workspace, scratch)
}

#[test]
fn correlated_run_generation_covers_every_boundary_without_a_full_catalog() {
    for count in [0, 1, 15, 16, 20, 21, 200] {
        let (status, mut workspace, scratch) = drive(count);
        assert_eq!(
            status,
            if count == 0 {
                LibraryStatus::NoBooks
            } else {
                LibraryStatus::Ready
            }
        );
        assert_eq!(workspace.entry_count(), count);
        assert_eq!(scratch.len(), count * SPOOL_RECORD_SIZE);

        let read = |index: u16, out: &mut [u8; SPOOL_RECORD_SIZE]| {
            let start = usize::from(index) * SPOOL_RECORD_SIZE;
            out.copy_from_slice(&scratch[start..start + SPOOL_RECORD_SIZE]);
            Ok(())
        };
        workspace.begin_merge(read).unwrap();
        for expected in 0..count {
            let entry = workspace.pop_min(read).unwrap().unwrap();
            assert_eq!(
                entry.entry.name.as_str(),
                format!("book-{expected:03}.binbook")
            );
            assert_eq!(entry.mount_token.unwrap().get(), expected as u16 + 1);
        }
        assert!(workspace.pop_min(read).unwrap().is_none());
    }
}

#[test]
fn entry_201_cancels_scan_deletes_scratch_and_never_starts_candidate() {
    let (status, workspace, scratch) = drive(201);
    assert_eq!(status, LibraryStatus::TooManyBooks);
    assert_eq!(workspace.entry_count(), 200);
    assert!(scratch.is_empty());
}

#[test]
fn unsupported_name_cancels_and_deletes_without_publishing_partial_records() {
    let mut workspace = LibraryRefreshWorkspace::new();
    let (mut client, _) = LibraryRunGenerationClient::begin(&mut workspace, &[], 1).unwrap();
    let mut buffer = [0; 4096];
    let next = client
        .accept(response(1, Response::Ack), Some(&mut buffer))
        .unwrap();
    let LibraryRunGenerationUpdate::Request(start) = next else {
        panic!()
    };
    let next = client
        .accept(
            response(start.sequence, Response::ScanStarted(ScanToken(5))),
            Some(&mut buffer),
        )
        .unwrap();
    let LibraryRunGenerationUpdate::Request(step) = next else {
        panic!()
    };
    let next = client
        .accept(
            response(
                step.sequence,
                Response::ScanProgress(ScanProgress {
                    entries: 0,
                    complete: false,
                    unsupported_name: true,
                }),
            ),
            Some(&mut buffer),
        )
        .unwrap();
    let LibraryRunGenerationUpdate::Request(cancel) = next else {
        panic!()
    };
    assert!(matches!(cancel.request, Request::ScanCancel { .. }));
    let next = client
        .accept(response(cancel.sequence, Response::Ack), Some(&mut buffer))
        .unwrap();
    let LibraryRunGenerationUpdate::Request(delete) = next else {
        panic!()
    };
    assert_eq!(delete.request, Request::LibraryScratchDelete);
    assert_eq!(
        client
            .accept(response(delete.sequence, Response::Ack), Some(&mut buffer))
            .unwrap(),
        LibraryRunGenerationUpdate::Complete(LibraryStatus::UnsupportedName)
    );
}

#[test]
fn every_run_generation_request_failure_cancels_and_deletes_before_retry() {
    let request_count = fault_generation(None);
    assert!(request_count > 5);
    for fault_at in 0..request_count {
        fault_generation(Some(fault_at));
    }
}

fn fault_generation(fault_at: Option<usize>) -> usize {
    let mut workspace = LibraryRefreshWorkspace::new();
    let (mut client, mut request) =
        LibraryRunGenerationClient::begin(&mut workspace, &[], 1).unwrap();
    let mut buffer = [0; 4096];
    let mut scratch = Vec::new();
    let mut scan_offset = 0usize;
    let mut scan_active = false;
    let mut request_index = 0usize;
    loop {
        let sequence = request.sequence;
        if fault_at == Some(request_index) {
            assert!(client
                .accept(
                    ResponseEnvelope {
                        client: StorageClient::Library,
                        sequence,
                        response: Err(CompletionError::Backend),
                    },
                    Some(&mut buffer),
                )
                .is_err());
            let mut cleanup = client.cleanup(LibraryStatus::CardError);
            while let Some(cleanup_request) = cleanup {
                let response_value = match cleanup_request.request {
                    Request::ScanCancel { .. } => {
                        scan_active = false;
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
                    Ok(LibraryRunGenerationUpdate::Request(next)) => Some(next),
                    Ok(LibraryRunGenerationUpdate::Complete(status)) => {
                        assert_eq!(status, LibraryStatus::CardError);
                        None
                    }
                    Err(error) => panic!("cleanup failed: {error:?}"),
                };
            }
            break;
        }
        request_index += 1;
        let response_value = match request.request {
            Request::LibraryScratchCreate => {
                scratch.clear();
                Response::Ack
            }
            Request::ScanStart => {
                assert!(!scan_active);
                scan_active = true;
                Response::ScanStarted(ScanToken(8))
            }
            Request::ScanStep { .. } => {
                let end = (scan_offset + 8).min(21);
                let records: Vec<_> = (scan_offset..end)
                    .map(|index| ScanRecord {
                        name: StorageName::new(&format!("fault-{index:03}.binbook")).unwrap(),
                        size: index as u64,
                        mount_token: None,
                    })
                    .collect();
                encode_scan_records(&records, &mut buffer).unwrap();
                scan_offset = end;
                Response::ScanProgress(ScanProgress {
                    entries: records.len() as u8,
                    complete: end == 21,
                    unsupported_name: false,
                })
            }
            Request::LibraryScratchWrite {
                offset,
                buffer_offset,
                len,
                ..
            } => {
                let offset = offset as usize;
                let source = usize::from(buffer_offset);
                let len = usize::from(len);
                scratch.resize(scratch.len().max(offset + len), 0);
                scratch[offset..offset + len].copy_from_slice(&buffer[source..source + len]);
                Response::Written(len as u16)
            }
            Request::ScanFinish { .. } => {
                scan_active = false;
                Response::Ack
            }
            Request::LibraryScratchFlush => Response::Ack,
            ref other => panic!("unexpected request {other:?}"),
        };
        match client
            .accept(response(sequence, response_value), Some(&mut buffer))
            .unwrap()
        {
            LibraryRunGenerationUpdate::Request(next) => request = next,
            LibraryRunGenerationUpdate::Complete(status) => {
                assert_eq!(status, LibraryStatus::Ready);
                break;
            }
        }
    }
    drop(client);
    assert!(!scan_active, "fault {fault_at:?} left scan active");
    if fault_at.is_some() {
        assert!(scratch.is_empty());
    }
    request_index
}
