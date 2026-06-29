use binbook_core::{Book, SliceSource};

pub const FIXTURE: &[u8] = include_bytes!("../fixtures/nav_probe.binbook");

pub fn open_fixture() -> Book<SliceSource<'static>> {
    let mut scratch = [0_u8; 1024];
    Book::open(SliceSource::new(FIXTURE), &mut scratch).expect("fixture must open")
}
