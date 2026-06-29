use binbook_core::{ReadAt, SliceReadError, SliceSource};

#[test]
fn slice_source_has_typed_bounds_errors() {
    let mut source = SliceSource::new(&[1, 2, 3]);
    assert_eq!(source.len(), Ok(3));
    let mut out = [0_u8; 2];
    assert_eq!(source.read_exact_at(1, &mut out), Ok(()));
    assert_eq!(out, [2, 3]);
    assert_eq!(
        source.read_exact_at(2, &mut out),
        Err(SliceReadError::OutOfBounds {
            offset: 2,
            length: 2,
            source_length: 3,
        })
    );
}
