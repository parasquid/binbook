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
    let mut out = vec![0u8; 96000];
    book.decompress_page(0, &mut out).unwrap();
    assert!(out.iter().all(|&b| b == 0xFF));
}

#[test]
fn decompresses_page_one() {
    let data: &[u8] = include_bytes!("fixtures/sample.binbook");
    let scratch = [0u8; 4096];
    let mut book = BinBook::open(data, scratch).unwrap();
    let mut out = vec![0u8; 96000];
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
