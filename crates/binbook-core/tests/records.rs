mod common;

use binbook_core::{ChunkIndex, Error, PlaneSlot};

#[test]
fn reads_typed_pages_planes_chunks_and_transitions() {
    let mut book = common::open_fixture();
    let mut record = [0_u8; 128];
    let page_number = book.page_number(0).unwrap();
    let page = book.page(page_number, &mut record).unwrap();
    assert_eq!(page.page_number, page_number);
    assert_eq!(page.stored_width, 800);
    assert_eq!(page.stored_height, 480);

    for slot in [
        PlaneSlot::OverlayMsb,
        PlaneSlot::OverlayLsb,
        PlaneSlot::FastBase,
    ] {
        let plane = page.planes.get(slot).expect("fixture plane must exist");
        assert!(plane.length.get() > 0);
    }
    assert!(page.planes.get(PlaneSlot::Reserved).is_none());

    let chunk_record = book.chunk_record_number(0).unwrap();
    let chunk = book.chunk(chunk_record, &mut record).unwrap();
    assert_eq!(chunk.page_number, page_number);
    assert_eq!(chunk.plane_slot, PlaneSlot::OverlayMsb);
    assert_eq!(chunk.chunk_index, ChunkIndex::new(0, 30).unwrap());
    assert_eq!(chunk.row_start, 0);
    assert_eq!(chunk.row_count, 16);
    assert_eq!(chunk.uncompressed_length.get(), 1600);

    let transition_number = book.transition_number(0).unwrap();
    let transition = book.transition(transition_number, &mut record).unwrap();
    assert_eq!(transition.from, page_number);
    assert_eq!(transition.to, book.page_number(1).unwrap());
    assert_eq!(transition.changed_chunk_mask, 0x3fff_ffff);
}

#[test]
fn explicit_plane_reads_do_not_concatenate_destinations() {
    let mut book = common::open_fixture();
    let mut record = [0_u8; 128];
    let page = book
        .page(book.page_number(0).unwrap(), &mut record)
        .unwrap();
    let msb = page.planes.get(PlaneSlot::OverlayMsb).unwrap();
    let lsb = page.planes.get(PlaneSlot::OverlayLsb).unwrap();
    let mut msb_bytes = vec![0_u8; msb.length.get() as usize];
    let mut lsb_bytes = vec![0_u8; lsb.length.get() as usize];
    book.read_plane(msb, &mut msb_bytes).unwrap();
    book.read_plane(lsb, &mut lsb_bytes).unwrap();
    assert_ne!(msb_bytes, lsb_bytes);

    let required = usize::try_from(msb.length.get()).unwrap();
    let mut too_small = vec![0_u8; required - 1];
    assert_eq!(
        book.read_plane(msb, &mut too_small),
        Err(Error::BufferTooSmall {
            required,
            provided: required - 1,
        })
    );
}
