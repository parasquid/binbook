use super::*;

#[test]
fn odd_width_panels_partition_every_source_column_once() {
    // Given/When: an odd-width source is split into two panels.
    let left = panel_bounds(5, 0, 2);
    let right = panel_bounds(5, 1, 2);

    // Then: the left owns the center column and the ranges are contiguous.
    assert_eq!(left, (0, 3));
    assert_eq!(right, (3, 2));
    assert_eq!(left.1 + right.1, 5);
}
