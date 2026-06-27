use binbook::BinBook;

fn open_fixture() -> BinBook<&'static [u8], [u8; 4096]> {
    let data: &[u8] = include_bytes!("fixtures/sample.binbook");
    let scratch = [0u8; 4096];
    BinBook::open(data, scratch).expect("open failed")
}

#[test]
fn opens_valid_fixture() {
    let book = open_fixture();
    assert_eq!(book.page_count(), 2);
    assert_eq!(book.nav_count(), 2);
    assert_eq!(book.chapter_count(), 2);
}

#[test]
fn rejects_invalid_magic() {
    let mut data = vec![0u8; 256];
    data[0..8].copy_from_slice(b"NOTBOOK\0");
    let scratch = [0u8; 4096];
    match BinBook::open(data.as_slice(), scratch) {
        Err(binbook::Error::InvalidMagic) => {}
        _ => panic!("expected InvalidMagic"),
    }
}

#[test]
fn rejects_too_short() {
    let scratch = [0u8; 4096];
    match BinBook::open([0u8; 100].as_slice(), scratch) {
        Err(binbook::Error::InvalidHeader) => {}
        _ => panic!("expected InvalidHeader"),
    }
}

#[test]
fn reads_page_info() {
    let mut book = open_fixture();
    let pi = book.page_info(0).unwrap();
    assert_eq!(pi.stored_width, 800);
    assert_eq!(pi.stored_height, 480);
    assert_eq!(pi.pixel_format, 2);
    assert_eq!(pi.compression_method, 1);
}

#[test]
fn reads_chapters() {
    let mut book = open_fixture();
    let ch = book.chapter(0).unwrap();
    assert_eq!(ch.index, 0);
    assert_eq!(ch.page_index, 0);
    let ch2 = book.chapter(1).unwrap();
    assert_eq!(ch2.index, 1);
    assert_eq!(ch2.page_index, 1);
}

#[test]
fn reads_nav_entries() {
    let mut book = open_fixture();
    let nav = book.nav_entry(0).unwrap();
    assert_eq!(nav.nav_type, 3);
}

#[test]
fn page_out_of_range() {
    let mut book = open_fixture();
    match book.page_info(99) {
        Err(binbook::Error::PageOutOfRange) => {}
        _ => panic!("expected PageOutOfRange"),
    }
}

#[test]
fn decompresses_rle_page() {
    let data: &[u8] = include_bytes!("fixtures/sample.binbook");
    let scratch = [0u8; 4096];
    let mut book = BinBook::open(data, scratch).unwrap();
    let mut out = vec![0u8; 48000];
    book.decompress_page(0, &mut out).unwrap();
    assert!(out.iter().all(|&b| b == 0xFF));
}

#[test]
fn decompresses_page_one() {
    let data: &[u8] = include_bytes!("fixtures/sample.binbook");
    let scratch = [0u8; 4096];
    let mut book = BinBook::open(data, scratch).unwrap();
    let mut out = vec![0u8; 48000];
    book.decompress_page(1, &mut out).unwrap();
    assert!(out.iter().all(|&b| b == 0x00));
}

#[test]
fn decompress_buffer_too_small() {
    let mut book = open_fixture();
    let mut out = vec![0u8; 10];
    match book.decompress_page(0, &mut out) {
        Err(binbook::Error::OutputBufferTooSmall) => {}
        _ => panic!("expected OutputBufferTooSmall"),
    }
}

#[test]
fn page_ref_exposes_current_packed_gray2_blob() {
    let data: &[u8] = include_bytes!("fixtures/sample.binbook");
    let scratch = [0u8; 4096];
    let mut book = BinBook::open(data, scratch).unwrap();

    let page = book.page(0).unwrap();

    assert_eq!(page.uncompressed_size, 48_000);
    let total: u32 = page.info.plane_dir.sizes.iter().sum();
    assert_eq!(page.compressed_data().len(), total as usize);
    assert_eq!(&page.compressed_data()[..8], &[0xFF; 8]);
}

#[test]
fn reads_chunk_and_transition_counts() {
    let book = open_fixture();
    assert_eq!(book.chunk_count(), 180);
    assert_eq!(book.transition_count(), 2);
}

#[test]
fn reads_chunk_entries() {
    let mut book = open_fixture();

    let c0 = book.chunk_entry(0).unwrap();
    assert_eq!(c0.page_number, 0);
    assert_eq!(c0.plane_slot, 0);
    assert_eq!(c0.chunk_index, 0);
    assert_eq!(c0.row_start, 0);
    assert_eq!(c0.row_count, 16);
    assert_eq!(c0.compressed_size, 26);
    assert_eq!(c0.uncompressed_size, 1600);

    let c1 = book.chunk_entry(1).unwrap();
    assert_eq!(c1.page_number, 0);
    assert_eq!(c1.plane_slot, 0);
    assert_eq!(c1.chunk_index, 1);
    assert_eq!(c1.row_start, 16);
    assert_eq!(c1.row_count, 16);
    assert_eq!(c1.compressed_size, 26);
    assert_eq!(c1.uncompressed_size, 1600);

    let c30 = book.chunk_entry(30).unwrap();
    assert_eq!(c30.page_number, 0);
    assert_eq!(c30.plane_slot, 1);
    assert_eq!(c30.chunk_index, 0);
}

#[test]
fn reads_transition_entries() {
    let mut book = open_fixture();

    let t0 = book.transition_entry(0).unwrap();
    assert_eq!(t0.from_page_number, 0);
    assert_eq!(t0.to_page_number, 1);
    assert_eq!(t0.changed_chunk_mask, 0x3FFFFFFF);
    assert_eq!(t0.first_changed_chunk, 0);
    assert_eq!(t0.changed_chunk_count, 30);
    assert_eq!(t0.flags, 0);

    let t1 = book.transition_entry(1).unwrap();
    assert_eq!(t1.from_page_number, 1);
    assert_eq!(t1.to_page_number, 0);
}

#[test]
fn chunk_out_of_range() {
    let mut book = open_fixture();
    match book.chunk_entry(999) {
        Err(binbook::Error::InvalidSection) => {}
        _ => panic!("expected InvalidSection for out-of-range chunk"),
    }
}

#[test]
fn transition_out_of_range() {
    let mut book = open_fixture();
    match book.transition_entry(999) {
        Err(binbook::Error::InvalidSection) => {}
        _ => panic!("expected InvalidSection for out-of-range transition"),
    }
}
