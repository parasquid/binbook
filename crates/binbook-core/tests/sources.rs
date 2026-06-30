use binbook_core::{ReadAt, SliceReadError, SliceSource};

mod common;

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

#[test]
fn plane_ranges_support_caller_selected_stream_buffers() {
    let mut book = common::open_fixture();
    let page_number = book.page_number(0).unwrap();
    let mut record = [0_u8; binbook_core::PAGE_RECORD_SIZE];
    let page = book.page(page_number, &mut record).unwrap();
    let plane = page
        .planes
        .get(binbook_core::PlaneSlot::OverlayMsb)
        .unwrap();
    let length = usize::try_from(plane.length.get()).unwrap();
    let mut whole = vec![0_u8; length];
    book.read_plane(plane, &mut whole).unwrap();
    let split = length / 2;
    let mut streamed = vec![0_u8; length];
    book.read_plane_range(plane, 0, &mut streamed[..split])
        .unwrap();
    book.read_plane_range(
        plane,
        u32::try_from(split).unwrap(),
        &mut streamed[split..length],
    )
    .unwrap();
    assert_eq!(&streamed[..length], &whole[..length]);
    assert!(book
        .read_plane_range(plane, plane.length.get(), &mut streamed[..1])
        .is_err());
}
