#![cfg(feature = "sd-storage")]

use binbook_fw::library::{
    IncrementalLibraryScan, Library, LibraryIndexCursor, LibraryIndexCursorValidator,
    LibraryIndexDecoder, LibraryIndexError, LibraryStatus, MountFileToken, ScanErrorKind,
    INDEX_HEADER_SIZE, INDEX_RECORD_SIZE, MAX_LIBRARY_ENTRIES,
};

#[test]
fn incremental_scan_builds_200_entries_in_eight_entry_batches_without_touching_cache() {
    let cached = scan(&[("cached.binbook", 7)]);
    let mut refresh = Library::empty();
    {
        let mut builder = IncrementalLibraryScan::begin(&mut refresh);
        for batch_start in (0..200).step_by(8) {
            for index in batch_start..(batch_start + 8).min(200) {
                builder.push(&format!("book-{index:03}.binbook"), index as u64);
            }
        }
        builder.finish();
    }

    assert_eq!(refresh.status(), LibraryStatus::Ready);
    assert_eq!(refresh.entries().len(), 200);
    assert_eq!(refresh.entries()[0].name.as_str(), "book-000.binbook");
    assert_eq!(refresh.entries()[199].name.as_str(), "book-199.binbook");
    assert_eq!(cached.entries()[0].name.as_str(), "cached.binbook");
}

#[test]
fn cancelled_or_failed_incremental_scan_cannot_replace_active_cache() {
    let mut cached = scan(&[("cached.binbook", 7)]);
    let mut refresh = Library::empty();
    {
        let mut cancelled = IncrementalLibraryScan::begin(&mut refresh);
        cancelled.push("partial.binbook", 1);
    }
    let cancelled = cached.apply_refresh_in_place(&mut refresh);
    assert!(!cancelled.adopted);
    assert_eq!(cached.entries()[0].name.as_str(), "cached.binbook");

    let failed = IncrementalLibraryScan::begin(&mut refresh);
    failed.fail(LibraryStatus::NoCard);
    let failed = cached.apply_refresh_in_place(&mut refresh);
    assert!(!failed.adopted);
    assert_eq!(failed.scan_status, LibraryStatus::NoCard);
    assert_eq!(cached.entries()[0].name.as_str(), "cached.binbook");
}
use binbook_fw::reader_state::{filename_hash, ProgressRecord, ReaderState};
use binbook_storage::{Filesystem, MAX_BINBOOK_NAME_BYTES};

#[test]
fn library_status_codes_match_the_diagnostic_contract() {
    for (status, code) in [
        (LibraryStatus::Ready, 0),
        (LibraryStatus::NoCard, 1),
        (LibraryStatus::CardError, 2),
        (LibraryStatus::NoBooks, 3),
        (LibraryStatus::TooManyBooks, 4),
        (LibraryStatus::UnsupportedName, 5),
    ] {
        assert_eq!(status.code(), code);
        assert_eq!(LibraryStatus::from_code(code), status);
    }
}

struct EntriesFs {
    entries: Vec<(String, u64)>,
    error: Option<ScanErrorKind>,
}

impl Filesystem for EntriesFs {
    type Error = ScanErrorKind;

    fn for_each_entry(&mut self, visit: &mut dyn FnMut(&str, u64)) -> Result<(), Self::Error> {
        if let Some(error) = self.error {
            return Err(error);
        }
        for (name, size) in &self.entries {
            visit(name, *size);
        }
        Ok(())
    }

    fn read_at(&mut self, _: &str, _: u64, _: &mut [u8]) -> Result<(), Self::Error> {
        unreachable!()
    }

    fn file_size(&mut self, _: &str) -> Result<u64, Self::Error> {
        unreachable!()
    }
}

fn scan(entries: &[(&str, u64)]) -> Library {
    let mut fs = EntriesFs {
        entries: entries
            .iter()
            .map(|(name, size)| ((*name).into(), *size))
            .collect(),
        error: None,
    };
    Library::scan(&mut fs, |error| error)
}

#[test]
fn scan_sorts_ascii_case_insensitively_with_raw_name_tiebreak() {
    let library = scan(&[
        ("z.binbook", 1),
        ("Alpha.binbook", 2),
        ("alpha.binbook", 3),
        ("notes.txt", 4),
    ]);
    let names: Vec<&str> = library
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, ["Alpha.binbook", "alpha.binbook", "z.binbook"]);
    assert_eq!(library.status(), LibraryStatus::Ready);
}

#[test]
fn preserves_exact_121_byte_name_and_hashes_exact_utf8_bytes() {
    let name = format!("{}.binbook", "a".repeat(MAX_BINBOOK_NAME_BYTES - 8));
    assert_eq!(name.len(), MAX_BINBOOK_NAME_BYTES);
    let library = scan(&[(&name, 99)]);
    assert_eq!(library.entries()[0].name.as_str(), name);
    assert_eq!(
        library.entries()[0].filename_hash,
        filename_hash(name.as_bytes())
    );
    assert_eq!(library.entries()[0].size, 99);
}

#[test]
fn status_distinguishes_all_scan_outcomes() {
    assert_eq!(scan(&[]).status(), LibraryStatus::NoBooks);
    for (error, status) in [
        (ScanErrorKind::NoCard, LibraryStatus::NoCard),
        (ScanErrorKind::CardError, LibraryStatus::CardError),
    ] {
        let mut fs = EntriesFs {
            entries: Vec::new(),
            error: Some(error),
        };
        assert_eq!(Library::scan(&mut fs, |value| value).status(), status);
    }

    let too_long = format!("{}.binbook", "b".repeat(MAX_BINBOOK_NAME_BYTES));
    assert_eq!(
        scan(&[(&too_long, 1)]).status(),
        LibraryStatus::UnsupportedName
    );

    let owned: Vec<(String, u64)> = (0..=MAX_LIBRARY_ENTRIES)
        .map(|index| (format!("{index:03}.binbook"), index as u64))
        .collect();
    let refs: Vec<(&str, u64)> = owned
        .iter()
        .map(|(name, size)| (name.as_str(), *size))
        .collect();
    assert_eq!(scan(&refs).status(), LibraryStatus::TooManyBooks);
}

#[test]
fn selection_restores_by_filename_hash_after_resort() {
    let library = scan(&[("b.binbook", 1), ("a.binbook", 2)]);
    assert_eq!(
        library.index_for_filename_hash(filename_hash(b"b.binbook")),
        Some(1)
    );
    assert_eq!(library.index_for_filename_hash(123), None);
}

#[test]
fn state_pruning_is_allowed_only_after_successful_scans() {
    let mut state = ReaderState::empty();
    state
        .set_progress(ProgressRecord {
            filename_hash: filename_hash(b"keep.binbook"),
            rendition_hash: [1; 32],
            page: 4,
        })
        .unwrap();
    state
        .set_progress(ProgressRecord {
            filename_hash: filename_hash(b"gone.binbook"),
            rendition_hash: [2; 32],
            page: 8,
        })
        .unwrap();

    let failed = {
        let mut fs = EntriesFs {
            entries: Vec::new(),
            error: Some(ScanErrorKind::CardError),
        };
        Library::scan(&mut fs, |value| value)
    };
    assert!(!failed.prune_state(&mut state));
    assert_eq!(state.records().len(), 2);

    let ready = scan(&[("keep.binbook", 1)]);
    assert!(ready.prune_state(&mut state));
    assert_eq!(state.records().len(), 1);

    let empty = scan(&[]);
    assert!(empty.prune_state(&mut state));
    assert!(state.records().is_empty());
}

#[test]
fn close_rescan_prunes_deleted_books_and_restores_the_surviving_selection() {
    let rendition = [7; 32];
    let deleted_hash = filename_hash(b"a.binbook");
    let selected_hash = filename_hash(b"b.binbook");
    let mut state = ReaderState::empty();
    state
        .set_progress(ProgressRecord {
            filename_hash: deleted_hash,
            rendition_hash: rendition,
            page: 8,
        })
        .unwrap();
    state
        .set_progress(ProgressRecord {
            filename_hash: selected_hash,
            rendition_hash: rendition,
            page: 3,
        })
        .unwrap();
    state.selected_filename_hash = selected_hash;

    let mut fs = EntriesFs {
        entries: vec![("b.binbook".into(), 20)],
        error: None,
    };
    let library = Library::scan(&mut fs, |error| error);
    assert!(library.prune_state(&mut state));

    assert_eq!(state.records().len(), 1);
    assert_eq!(state.records()[0].filename_hash, selected_hash);
    assert_eq!(library.index_for_filename_hash(selected_hash), Some(0));
}

#[test]
fn refresh_atomically_replaces_cache_only_for_usable_scan_results() {
    let mut cached = scan(&[("a.binbook", 1), ("b.binbook", 2)]);

    let changed = cached.apply_refresh(scan(&[("b.binbook", 2), ("c.binbook", 3)]));
    assert!(changed.adopted);
    assert!(changed.changed);
    assert_eq!(changed.scan_status, LibraryStatus::Ready);
    assert_eq!(
        cached
            .entries()
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["b.binbook", "c.binbook"]
    );

    let mut failed_fs = EntriesFs {
        entries: Vec::new(),
        error: Some(ScanErrorKind::NoCard),
    };
    let failed = Library::scan(&mut failed_fs, |error| error);
    let retained = cached.apply_refresh(failed);
    assert!(!retained.adopted);
    assert!(!retained.changed);
    assert_eq!(retained.scan_status, LibraryStatus::NoCard);
    assert_eq!(cached.status(), LibraryStatus::Ready);
    assert_eq!(cached.entries().len(), 2);
}

#[test]
fn refresh_adopts_successful_empty_scan_and_detects_unchanged_listing() {
    let mut cached = scan(&[("a.binbook", 1)]);
    let unchanged = cached.apply_refresh(scan(&[("a.binbook", 1)]));
    assert!(unchanged.adopted);
    assert!(!unchanged.changed);

    let emptied = cached.apply_refresh(scan(&[]));
    assert!(emptied.adopted);
    assert!(emptied.changed);
    assert_eq!(cached.status(), LibraryStatus::NoBooks);
    assert!(cached.entries().is_empty());
}

#[test]
fn in_place_refresh_swaps_buffers_and_retains_cache_on_failure() {
    let mut cached = scan(&[("old.binbook", 1)]);
    let mut refreshed = scan(&[("new.binbook", 2)]);
    let result = cached.apply_refresh_in_place(&mut refreshed);
    assert!(result.adopted);
    assert_eq!(cached.entries()[0].name.as_str(), "new.binbook");
    assert_eq!(refreshed.entries()[0].name.as_str(), "old.binbook");

    let mut failed_fs = EntriesFs {
        entries: Vec::new(),
        error: Some(ScanErrorKind::CardError),
    };
    refreshed = Library::scan(&mut failed_fs, |error| error);
    let failed = cached.apply_refresh_in_place(&mut refreshed);
    assert!(!failed.adopted);
    assert_eq!(cached.entries()[0].name.as_str(), "new.binbook");
}

#[test]
fn persisted_index_round_trips_complete_entries() {
    let library = scan(&[
        ("Zulu.binbook", 9_001),
        ("alpha.binbook", 42),
        ("A-very-long-but-supported-book-name.binbook", 1_234_567),
    ]);
    let mut encoded = vec![0xa5; library.index_encoded_len()];

    let used = library.encode_index(&mut encoded).unwrap();
    assert_eq!(used, INDEX_HEADER_SIZE + 3 * INDEX_RECORD_SIZE);
    assert_eq!(&encoded[..4], b"BBLI");
    assert_eq!(Library::decode_index(&encoded).unwrap(), library);
}

#[test]
fn persisted_index_never_restores_mount_local_file_tokens() {
    let mut live = Library::empty();
    let mut scan = IncrementalLibraryScan::begin(&mut live);
    scan.push_mounted("cached.binbook", 42, Some(MountFileToken::new(9).unwrap()));
    scan.finish();
    let mut encoded = vec![0; live.index_encoded_len()];
    live.encode_index(&mut encoded).unwrap();

    let decoded = Library::decode_index(&encoded).unwrap();
    assert_eq!(decoded.entries()[0].mount_token, None);
    assert_eq!(decoded.entries()[0].name, live.entries()[0].name);
    assert_eq!(decoded.entries()[0].size, live.entries()[0].size);
}

#[test]
fn adding_mount_tokens_does_not_report_visible_library_content_changed() {
    let mut cached = scan(&[("cached.binbook", 42)]);
    let mut refreshed = Library::empty();
    let mut scan = IncrementalLibraryScan::begin(&mut refreshed);
    scan.push_mounted("cached.binbook", 42, Some(MountFileToken::new(1).unwrap()));
    scan.finish();

    let outcome = cached.apply_refresh(refreshed);
    assert!(outcome.adopted);
    assert!(!outcome.changed);
    assert_eq!(cached.entries()[0].mount_token.unwrap().get(), 1);
}

#[test]
fn persisted_empty_index_round_trips_as_no_books() {
    let library = Library::empty();
    let mut encoded = vec![0; library.index_encoded_len()];
    library.encode_index(&mut encoded).unwrap();

    let decoded = Library::decode_index(&encoded).unwrap();
    assert_eq!(decoded.status(), LibraryStatus::NoBooks);
    assert!(decoded.entries().is_empty());
}

#[test]
fn persisted_index_rejects_corruption_truncation_and_forged_hash() {
    let library = scan(&[("actual.binbook", 99)]);
    let mut encoded = vec![0; library.index_encoded_len()];
    library.encode_index(&mut encoded).unwrap();

    let mut corrupt = encoded.clone();
    corrupt[INDEX_HEADER_SIZE + 2] ^= 0x80;
    assert_eq!(Library::decode_index(&corrupt), Err(LibraryIndexError::Crc));
    assert_eq!(
        Library::decode_index(&encoded[..encoded.len() - 1]),
        Err(LibraryIndexError::Length)
    );

    let mut forged = encoded;
    forged[INDEX_HEADER_SIZE + 10] ^= 1;
    forged[12..16].fill(0);
    let crc = binbook_fw::crc32::iso_hdlc(&forged);
    forged[12..16].copy_from_slice(&crc.to_le_bytes());
    assert_eq!(
        Library::decode_index(&forged),
        Err(LibraryIndexError::FilenameHash)
    );
}

#[test]
fn persisted_index_maximum_is_fixed_and_under_32_kib() {
    assert_eq!(
        Library::max_index_bytes(),
        INDEX_HEADER_SIZE + MAX_LIBRARY_ENTRIES * INDEX_RECORD_SIZE
    );
    assert!(Library::max_index_bytes() < 32 * 1024);
}

#[test]
fn persisted_index_decodes_one_fixed_record_at_a_time() {
    let library = scan(&[("a.binbook", 1), ("b.binbook", 2), ("c.binbook", 3)]);
    let mut encoded = vec![0; library.index_encoded_len()];
    library.encode_index(&mut encoded).unwrap();

    let mut decoded = Library::empty();
    let mut decoder = LibraryIndexDecoder::new(
        encoded[..INDEX_HEADER_SIZE].try_into().unwrap(),
        encoded.len(),
        &mut decoded,
    )
    .unwrap();
    for record in encoded[INDEX_HEADER_SIZE..].chunks_exact(INDEX_RECORD_SIZE) {
        decoder.push_record(record.try_into().unwrap()).unwrap();
    }

    decoder.finish().unwrap();
    assert_eq!(decoded, library);
}

#[test]
fn persisted_index_decoder_does_not_own_a_library_buffer() {
    assert!(core::mem::size_of::<LibraryIndexDecoder<'static>>() < 64);
}

#[test]
fn validated_cursor_reads_a_200_book_viewport_without_owning_a_library() {
    let owned: Vec<(String, u64)> = (0..200)
        .map(|index| (format!("book-{index:03}.binbook"), index as u64))
        .collect();
    let refs: Vec<_> = owned
        .iter()
        .map(|(name, size)| (name.as_str(), *size))
        .collect();
    let library = scan(&refs);
    let mut encoded = vec![0; library.index_encoded_len()];
    library.encode_index(&mut encoded).unwrap();
    let header: &[u8; INDEX_HEADER_SIZE] = encoded[..INDEX_HEADER_SIZE].try_into().unwrap();
    let cursor = LibraryIndexCursor::from_header(header, encoded.len()).unwrap();
    assert_eq!(cursor.entry_count(), 200);
    assert!(core::mem::size_of::<LibraryIndexCursor>() <= 16);

    let mut validator = LibraryIndexCursorValidator::new(cursor, header);
    for index in 0..cursor.entry_count() {
        let range = cursor.record_range(index).unwrap();
        validator
            .push_record(&encoded[range.start..range.end].try_into().unwrap())
            .unwrap();
    }
    validator.finish().unwrap();

    let names: Vec<_> = (95..105)
        .map(|index| {
            let range = cursor.record_range(index).unwrap();
            cursor
                .decode_record(&encoded[range.start..range.end].try_into().unwrap())
                .unwrap()
                .name
        })
        .collect();
    assert_eq!(names.first().unwrap().as_str(), "book-095.binbook");
    assert_eq!(names.last().unwrap().as_str(), "book-104.binbook");
}

#[test]
fn cursor_validation_rejects_a_corrupt_record_before_viewport_use() {
    let library = scan(&[("one.binbook", 1), ("two.binbook", 2)]);
    let mut encoded = vec![0; library.index_encoded_len()];
    library.encode_index(&mut encoded).unwrap();
    let header: [u8; INDEX_HEADER_SIZE] = encoded[..INDEX_HEADER_SIZE].try_into().unwrap();
    let cursor = LibraryIndexCursor::from_header(&header, encoded.len()).unwrap();
    encoded[INDEX_HEADER_SIZE + 2] ^= 0x80;
    let mut validator = LibraryIndexCursorValidator::new(cursor, &header);
    for index in 0..cursor.entry_count() {
        let range = cursor.record_range(index).unwrap();
        validator
            .push_record(&encoded[range.start..range.end].try_into().unwrap())
            .unwrap();
    }
    assert_eq!(validator.finish(), Err(LibraryIndexError::Crc));
}

#[test]
fn persisted_index_encodes_with_only_header_and_record_scratch() {
    let library = scan(&[("a.binbook", 1), ("b.binbook", 2)]);
    let mut expected = vec![0; library.index_encoded_len()];
    library.encode_index(&mut expected).unwrap();

    let mut streamed = Vec::new();
    let mut header = [0; INDEX_HEADER_SIZE];
    library.encode_index_header(&mut header).unwrap();
    streamed.extend_from_slice(&header);
    let mut record = [0; INDEX_RECORD_SIZE];
    for index in 0..library.entries().len() {
        library.encode_index_record(index, &mut record).unwrap();
        streamed.extend_from_slice(&record);
    }

    assert_eq!(streamed, expected);
}
