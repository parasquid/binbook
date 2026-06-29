use gray2_render::{ordered_bw, CanonicalGray2};

#[test]
fn ordered_dither_changes_gray_decisions_at_adjacent_coordinates() {
    assert_ne!(
        ordered_bw(CanonicalGray2::DarkGray, 0, 0),
        ordered_bw(CanonicalGray2::DarkGray, 1, 0)
    );
    assert!(ordered_bw(CanonicalGray2::Black, 0, 0));
    assert!(!ordered_bw(CanonicalGray2::White, 0, 0));
}
