#![cfg(feature = "sd-storage")]

use binbook_fw::{
    library::{LibraryEntry, MountFileToken, LIBRARY_VIEWPORT_CAPACITY, MAX_LIBRARY_ENTRIES},
    library_external_sort::{
        ExternalSortError, LibraryRefreshWorkspace, MAX_RUN_BYTES, SPOOL_RECORD_SIZE,
    },
    reader_state::filename_hash,
};

fn entry(index: usize) -> LibraryEntry {
    let name = format!("book-{index:03}.binbook");
    LibraryEntry {
        filename_hash: filename_hash(name.as_bytes()),
        name: name.as_str().try_into().unwrap(),
        size: index as u64 * 17,
        mount_token: MountFileToken::new(index as u16 + 1),
    }
}

fn sort_indices(indices: &[usize]) -> (LibraryRefreshWorkspace, Vec<u8>) {
    let mut workspace = LibraryRefreshWorkspace::new();
    let mut spool = Vec::new();
    let mut run = [0; MAX_RUN_BYTES];
    for index in indices {
        if workspace.buffered_count() == LIBRARY_VIEWPORT_CAPACITY {
            let used = workspace.flush_run(&mut run).unwrap();
            spool.extend_from_slice(&run[..used]);
        }
        workspace.push(entry(*index)).unwrap();
    }
    let used = workspace.flush_run(&mut run).unwrap();
    spool.extend_from_slice(&run[..used]);
    (workspace, spool)
}

fn merge(mut workspace: LibraryRefreshWorkspace, spool: &[u8]) -> Vec<LibraryEntry> {
    let read = |index: u16, out: &mut [u8; SPOOL_RECORD_SIZE]| {
        let start = usize::from(index) * SPOOL_RECORD_SIZE;
        out.copy_from_slice(&spool[start..start + SPOOL_RECORD_SIZE]);
        Ok(())
    };
    workspace.begin_merge(read).unwrap();
    let mut merged = Vec::new();
    while let Some(value) = workspace.pop_min(read).unwrap() {
        assert_eq!(value.entry.mount_token, value.mount_token);
        merged.push(value.entry);
    }
    merged
}

#[test]
fn workspace_is_bounded_to_one_viewport_and_fourteen_runs() {
    assert!(core::mem::size_of::<LibraryRefreshWorkspace>() <= 3 * 1024);
    let indices: Vec<_> = (0..MAX_LIBRARY_ENTRIES).rev().collect();
    let (workspace, spool) = sort_indices(&indices);
    assert_eq!(workspace.entry_count(), 200);
    assert_eq!(workspace.run_count(), 14);
    assert_eq!(workspace.buffered_count(), 0);
    assert_eq!(spool.len(), 200 * SPOOL_RECORD_SIZE);
}

#[test]
fn external_sort_matches_reference_for_all_run_boundaries() {
    for count in [0, 1, 15, 16, 20, 21, 200] {
        let indices: Vec<_> = (0..count).rev().collect();
        let (workspace, spool) = sort_indices(&indices);
        let merged = merge(workspace, &spool);
        let actual: Vec<_> = merged.iter().map(|entry| entry.name.as_str()).collect();
        let expected: Vec<_> = (0..count)
            .map(|index| format!("book-{index:03}.binbook"))
            .collect();
        assert_eq!(actual, expected);
    }
}

#[test]
fn entry_201_is_rejected_before_any_fifteenth_run_exists() {
    let indices: Vec<_> = (0..MAX_LIBRARY_ENTRIES).rev().collect();
    let (mut workspace, _) = sort_indices(&indices);
    assert_eq!(
        workspace.push(entry(MAX_LIBRARY_ENTRIES)),
        Err(ExternalSortError::TooManyBooks)
    );
    assert_eq!(workspace.run_count(), 14);
}

#[test]
fn merge_rejects_exact_duplicates_but_preserves_casefold_tiebreaks() {
    let names = ["alpha.binbook", "Alpha.binbook", "alpha.binbook"];
    let mut workspace = LibraryRefreshWorkspace::new();
    let mut run = [0; MAX_RUN_BYTES];
    for (index, name) in names.into_iter().enumerate() {
        workspace
            .push(LibraryEntry {
                name: name.try_into().unwrap(),
                size: index as u64,
                filename_hash: filename_hash(name.as_bytes()),
                mount_token: None,
            })
            .unwrap();
    }
    workspace.flush_run(&mut run).unwrap();
    let read = |index: u16, out: &mut [u8; SPOOL_RECORD_SIZE]| {
        let start = usize::from(index) * SPOOL_RECORD_SIZE;
        out.copy_from_slice(&run[start..start + SPOOL_RECORD_SIZE]);
        Ok(())
    };
    workspace.begin_merge(read).unwrap();
    assert_eq!(
        workspace.pop_min(read).unwrap().unwrap().entry.name,
        "Alpha.binbook"
    );
    assert!(workspace.pop_min(read).is_ok());
    assert_eq!(
        workspace.pop_min(read),
        Err(ExternalSortError::InvalidSpoolRecord(
            binbook_fw::library::LibraryIndexError::DuplicateName
        ))
    );
}
