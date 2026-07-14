use binbook_fw::reader_state::{
    filename_hash, select_newer, ProgressRecord, ReaderState, ReaderStateError, SavedMode,
    MAX_PROGRESS_RECORDS, MAX_STATE_BYTES, STATE_HEADER_SIZE, STATE_RECORD_SIZE,
};

const HASH_A: [u8; 32] = [0x11; 32];
const HASH_B: [u8; 32] = [0x22; 32];

fn record(filename_hash: u64, rendition_hash: [u8; 32], page: u32) -> ProgressRecord {
    ProgressRecord {
        filename_hash,
        rendition_hash,
        page,
    }
}

#[test]
fn empty_state_has_exact_header_layout_and_roundtrips() {
    let state = ReaderState::empty();
    let mut bytes = [0xcc; MAX_STATE_BYTES];
    let used = state.encode(&mut bytes).unwrap();

    assert_eq!(used, STATE_HEADER_SIZE);
    assert_eq!(&bytes[0..4], b"BBST");
    assert_eq!(&bytes[4..6], &1_u16.to_le_bytes());
    assert_eq!(&bytes[6..8], &(STATE_HEADER_SIZE as u16).to_le_bytes());
    assert_eq!(bytes[12], 0);
    assert_eq!(&bytes[13..16], &[0; 3]);
    assert_eq!(&bytes[64..66], &0_u16.to_le_bytes());
    assert_eq!(&bytes[66..68], &[0; 2]);
    assert_eq!(ReaderState::decode(&bytes[..used]).unwrap(), state);
}

#[test]
fn record_layout_is_exact_and_encoding_is_sorted() {
    let mut state = ReaderState::empty();
    state.set_progress(record(9, HASH_B, 7)).unwrap();
    state.set_progress(record(2, HASH_A, 3)).unwrap();
    let mut bytes = [0; MAX_STATE_BYTES];
    let used = state.encode(&mut bytes).unwrap();

    assert_eq!(used, STATE_HEADER_SIZE + 2 * STATE_RECORD_SIZE);
    assert_eq!(&bytes[72..80], &2_u64.to_le_bytes());
    assert_eq!(&bytes[80..112], &HASH_A);
    assert_eq!(&bytes[112..116], &3_u32.to_le_bytes());
    assert_eq!(&bytes[116..124], &9_u64.to_le_bytes());
}

#[test]
fn fnv1a_filename_hash_has_locked_vectors() {
    assert_eq!(filename_hash(b""), 0xcbf29ce484222325);
    assert_eq!(filename_hash(b"a"), 0xaf63dc4c8601ec8c);
    assert_eq!(filename_hash(b"hello"), 0xa430d84680aabd0b);
    assert_ne!(
        filename_hash(b"Book.binbook"),
        filename_hash(b"book.binbook")
    );
}

#[test]
fn lookup_requires_filename_and_rendition_and_replacement_starts_fresh() {
    let mut state = ReaderState::empty();
    state.set_progress(record(7, HASH_A, 12)).unwrap();
    assert_eq!(state.lookup_page(7, &HASH_A), Some(12));
    assert_eq!(state.lookup_page(7, &HASH_B), None);
    assert_eq!(state.lookup_page(8, &HASH_A), None);
}

#[test]
fn capacity_is_200_and_201_is_rejected() {
    let mut state = ReaderState::empty();
    for index in 0..MAX_PROGRESS_RECORDS {
        let mut hash = HASH_A;
        hash[0..8].copy_from_slice(&(index as u64).to_le_bytes());
        state
            .set_progress(record(index as u64 + 1, hash, index as u32))
            .unwrap();
    }
    assert_eq!(state.records().len(), MAX_PROGRESS_RECORDS);
    assert_eq!(
        state.set_progress(record(999, HASH_B, 1)),
        Err(ReaderStateError::TooManyRecords)
    );
}

#[test]
fn decode_rejects_crc_and_structural_corruption() {
    let mut bytes = [0; MAX_STATE_BYTES];
    let used = ReaderState::empty().encode(&mut bytes).unwrap();
    for offset in [0, 4, 6, 12, 13, 66] {
        let mut corrupt = bytes[..used].to_vec();
        corrupt[offset] ^= 0xff;
        assert!(ReaderState::decode(&corrupt).is_err(), "offset {offset}");
    }
    let mut corrupt = bytes[..used].to_vec();
    corrupt.push(0);
    assert_eq!(ReaderState::decode(&corrupt), Err(ReaderStateError::Length));
}

#[test]
fn decode_rejects_mode_reserved_and_count_after_valid_crc_rewrite() {
    let mut bytes = [0; MAX_STATE_BYTES];
    let used = ReaderState::empty().encode(&mut bytes).unwrap();

    bytes[12] = 2;
    binbook_fw::reader_state::rewrite_crc(&mut bytes[..used]);
    assert_eq!(
        ReaderState::decode(&bytes[..used]),
        Err(ReaderStateError::Mode)
    );

    bytes[12] = 0;
    bytes[13] = 1;
    binbook_fw::reader_state::rewrite_crc(&mut bytes[..used]);
    assert_eq!(
        ReaderState::decode(&bytes[..used]),
        Err(ReaderStateError::Reserved)
    );

    bytes[13] = 0;
    bytes[64..66].copy_from_slice(&201_u16.to_le_bytes());
    binbook_fw::reader_state::rewrite_crc(&mut bytes[..used]);
    assert_eq!(
        ReaderState::decode(&bytes[..used]),
        Err(ReaderStateError::TooManyRecords)
    );
}

#[test]
fn decode_rejects_duplicate_records_and_zero_rendition() {
    let mut state = ReaderState::empty();
    state.set_progress(record(1, HASH_A, 2)).unwrap();
    state.set_progress(record(2, HASH_B, 3)).unwrap();
    let mut bytes = [0; MAX_STATE_BYTES];
    let used = state.encode(&mut bytes).unwrap();

    let duplicate: [u8; STATE_RECORD_SIZE] = bytes[72..116].try_into().unwrap();
    bytes[116..160].copy_from_slice(&duplicate);
    binbook_fw::reader_state::rewrite_crc(&mut bytes[..used]);
    assert_eq!(
        ReaderState::decode(&bytes[..used]),
        Err(ReaderStateError::Duplicate)
    );

    state = ReaderState::empty();
    assert_eq!(
        state.set_progress(record(1, [0; 32], 2)),
        Err(ReaderStateError::ZeroRenditionHash)
    );
}

#[test]
fn reading_mode_requires_matching_active_progress() {
    let mut state = ReaderState::empty();
    state.set_progress(record(3, HASH_A, 4)).unwrap();
    state.set_reading(3, HASH_A).unwrap();
    state.selected_filename_hash = 3;
    let mut bytes = [0; MAX_STATE_BYTES];
    let used = state.encode(&mut bytes).unwrap();
    let decoded = ReaderState::decode(&bytes[..used]).unwrap();
    assert_eq!(decoded.saved_mode, SavedMode::Reading);

    bytes[24..32].copy_from_slice(&4_u64.to_le_bytes());
    binbook_fw::reader_state::rewrite_crc(&mut bytes[..used]);
    assert_eq!(
        ReaderState::decode(&bytes[..used]),
        Err(ReaderStateError::InvalidModeState)
    );
}

#[test]
fn prune_removes_deleted_filenames_only_after_caller_requests_it() {
    let mut state = ReaderState::empty();
    state.set_progress(record(1, HASH_A, 2)).unwrap();
    state.set_progress(record(2, HASH_B, 3)).unwrap();
    state.prune(|hash| hash == 2);
    assert_eq!(state.records(), &[record(2, HASH_B, 3)]);
}

#[test]
fn prune_clears_deleted_selection_and_falls_back_from_deleted_active_book() {
    let mut state = ReaderState::empty();
    state.set_progress(record(1, HASH_A, 2)).unwrap();
    state.selected_filename_hash = 1;
    state.set_reading(1, HASH_A).unwrap();

    state.prune(|_| false);

    assert_eq!(state.saved_mode, SavedMode::Menu);
    assert_eq!(state.selected_filename_hash, 0);
    assert_eq!(state.active_filename_hash, 0);
    assert_eq!(state.active_rendition_hash, [0; 32]);
}

#[test]
fn newer_slot_selection_uses_wrapping_generation_order() {
    let mut old = ReaderState::empty();
    old.generation = u32::MAX;
    let mut wrapped = ReaderState::empty();
    wrapped.generation = 0;
    assert_eq!(select_newer(Some(&old), Some(&wrapped)), Some(1));
    assert_eq!(select_newer(Some(&wrapped), None), Some(0));
    assert_eq!(select_newer(None, None), None);
}

#[test]
fn visible_page_commit_and_close_preserve_independent_book_progress() {
    let mut state = ReaderState::empty();

    state.commit_visible_page(11, HASH_A, 2).unwrap();
    assert_eq!(state.saved_mode, SavedMode::Reading);
    state.set_menu();
    state.commit_visible_page(22, HASH_B, 1).unwrap();
    state.set_menu();

    let mut bytes = [0; MAX_STATE_BYTES];
    let used = state.encode(&mut bytes).unwrap();
    let restored = ReaderState::decode(&bytes[..used]).unwrap();
    assert_eq!(restored.saved_mode, SavedMode::Menu);
    assert_eq!(restored.selected_filename_hash, 22);
    assert_eq!(restored.lookup_page(11, &HASH_A), Some(2));
    assert_eq!(restored.lookup_page(22, &HASH_B), Some(1));
}

#[test]
fn committing_replacement_rendition_discards_old_progress_for_same_filename() {
    let mut state = ReaderState::empty();
    state.commit_visible_page(11, HASH_A, 9).unwrap();
    state.commit_visible_page(11, HASH_B, 0).unwrap();

    assert_eq!(state.lookup_page(11, &HASH_A), None);
    assert_eq!(state.lookup_page(11, &HASH_B), Some(0));
    assert_eq!(state.records().len(), 1);
}
