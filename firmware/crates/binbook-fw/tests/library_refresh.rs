#![cfg(feature = "sd-storage")]

use binbook_fw::{
    library::{Library, LibraryStatus, MountFileToken},
    library_refresh::{LibraryRefreshClient, LibraryRefreshError, LibraryRefreshUpdate},
    storage_owner::{
        encode_scan_records, CompletionError, Request, Response, ResponseEnvelope, ScanProgress,
        ScanRecord, ScanToken, StorageClient, StorageName, SCAN_BATCH_BYTES,
    },
};

fn success(sequence: u32, response: Response) -> ResponseEnvelope {
    ResponseEnvelope {
        client: StorageClient::Library,
        sequence,
        response: Ok(response),
    }
}

#[test]
fn refresh_client_builds_200_entries_from_correlated_owner_batches() {
    let mut library = Library::empty();
    let (mut client, start) = LibraryRefreshClient::begin(&mut library, 10);
    assert!(matches!(start.request, Request::ScanStart));
    let token = ScanToken(7);
    let mut update = client
        .accept(success(10, Response::ScanStarted(token)), None)
        .unwrap();
    let mut sequence = 11;
    for batch_start in (0..200).step_by(8) {
        let records: Vec<_> = (batch_start..(batch_start + 8).min(200))
            .map(|index| ScanRecord {
                name: StorageName::new(&format!("book-{index:03}.binbook")).unwrap(),
                size: index as u64,
                mount_token: Some(MountFileToken::new(index as u16 + 1).unwrap()),
            })
            .collect();
        assert!(matches!(update, LibraryRefreshUpdate::Request(_)));
        let mut bytes = [0; SCAN_BATCH_BYTES];
        encode_scan_records(&records, &mut bytes).unwrap();
        update = client
            .accept(
                success(
                    sequence,
                    Response::ScanProgress(ScanProgress {
                        entries: records.len() as u8,
                        complete: batch_start + records.len() == 200,
                        unsupported_name: false,
                    }),
                ),
                Some(&bytes),
            )
            .unwrap();
        sequence += 1;
    }
    assert!(matches!(update, LibraryRefreshUpdate::Request(_)));
    assert_eq!(
        client
            .accept(success(sequence, Response::Ack), None)
            .unwrap(),
        LibraryRefreshUpdate::Complete(LibraryStatus::Ready)
    );
    drop(client);
    assert_eq!(library.entries().len(), 200);
    assert_eq!(library.entries()[137].mount_token.unwrap().get(), 138);
}

#[test]
fn refresh_client_configures_exact_viewport_prefetch_before_scanning() {
    use binbook_fw::library::IncrementalLibraryScan;

    let mut visible = Library::empty();
    let mut source = IncrementalLibraryScan::begin(&mut visible);
    source.push("zeta.binbook", 1);
    source.push("alpha.binbook", 2);
    source.finish();
    let mut refresh = Library::empty();
    let names: Vec<_> = visible
        .entries()
        .iter()
        .map(|entry| StorageName::new(entry.name.as_str()).unwrap())
        .collect();
    let (mut client, _) =
        LibraryRefreshClient::begin_with_prefetch(&mut refresh, &names, 100).unwrap();
    let token = ScanToken(4);
    let first = client
        .accept(success(100, Response::ScanStarted(token)), None)
        .unwrap();
    let LibraryRefreshUpdate::Request(first) = first else {
        panic!()
    };
    assert!(matches!(
        first.request,
        Request::ScanPrefetch { ref name, token: value }
            if value == token && name.as_str() == "alpha.binbook"
    ));
    let second = client
        .accept(success(first.sequence, Response::Ack), None)
        .unwrap();
    let LibraryRefreshUpdate::Request(second) = second else {
        panic!()
    };
    assert!(matches!(
        second.request,
        Request::ScanPrefetch { ref name, token: value }
            if value == token && name.as_str() == "zeta.binbook"
    ));
    let scan = client
        .accept(success(second.sequence, Response::Ack), None)
        .unwrap();
    assert!(
        matches!(scan, LibraryRefreshUpdate::Request(request) if matches!(request.request, Request::ScanStep { token: value, .. } if value == token))
    );
}

#[test]
fn refresh_client_rejects_wrong_sequence_and_leaves_refresh_non_adoptable() {
    let mut refresh = Library::empty();
    let (mut client, _) = LibraryRefreshClient::begin(&mut refresh, 20);
    assert_eq!(
        client.accept(success(21, Response::ScanStarted(ScanToken(1))), None),
        Err(LibraryRefreshError::Correlation)
    );
    drop(client);
    assert_eq!(refresh.status(), LibraryStatus::CardError);
}

#[test]
fn refresh_client_cancellation_uses_token_and_discards_partial_listing() {
    let mut refresh = Library::empty();
    let (mut client, _) = LibraryRefreshClient::begin(&mut refresh, 30);
    let token = ScanToken(9);
    client
        .accept(success(30, Response::ScanStarted(token)), None)
        .unwrap();
    let cancel = client.cancel().unwrap();
    assert!(matches!(cancel.request, Request::ScanCancel { token: value } if value == token));
    assert_eq!(
        client
            .accept(success(cancel.sequence, Response::Ack), None)
            .unwrap(),
        LibraryRefreshUpdate::Complete(LibraryStatus::CardError)
    );
    drop(client);
    assert_eq!(refresh.status(), LibraryStatus::CardError);
}

#[test]
fn refresh_client_propagates_owner_failure_without_false_completion() {
    let mut refresh = Library::empty();
    let (mut client, _) = LibraryRefreshClient::begin(&mut refresh, 40);
    let failure = ResponseEnvelope {
        client: StorageClient::Library,
        sequence: 40,
        response: Err(CompletionError::Backend),
    };
    assert_eq!(
        client.accept(failure, None),
        Err(LibraryRefreshError::Completion)
    );
}

#[test]
fn refresh_client_can_cancel_owner_scan_after_an_in_scan_failure() {
    let mut refresh = Library::empty();
    let (mut client, _) = LibraryRefreshClient::begin(&mut refresh, 50);
    let token = ScanToken(11);
    let step = client
        .accept(success(50, Response::ScanStarted(token)), None)
        .unwrap();
    let LibraryRefreshUpdate::Request(step) = step else {
        panic!()
    };
    let failure = ResponseEnvelope {
        client: StorageClient::Library,
        sequence: step.sequence,
        response: Err(CompletionError::Backend),
    };

    assert_eq!(
        client.accept(failure, None),
        Err(LibraryRefreshError::Completion)
    );
    let cancel = client
        .cancel()
        .expect("active owner scan must be cancelled");
    assert!(matches!(
        cancel.request,
        Request::ScanCancel { token: value } if value == token
    ));
}
