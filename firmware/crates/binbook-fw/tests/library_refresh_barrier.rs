use binbook_fw::library_refresh_barrier::LibraryRefreshBarrier;

#[test]
fn catalog_stays_pending_until_display_adoption_settles_it() {
    let barrier = LibraryRefreshBarrier::new_pending();

    assert!(barrier.is_pending());
    barrier.mark_settled();
    assert!(!barrier.is_pending());

    barrier.mark_pending();
    assert!(barrier.is_pending());
    barrier.mark_settled();
    assert!(!barrier.is_pending());
}
