use binbook_core::{Book, BookIndex, SliceSource};

const FIXTURE: &[u8] = include_bytes!("fixtures/nav_probe.binbook");

#[test]
fn source_independent_index_matches_book_page_chunk_and_rendition_parsing() {
    let header = BookIndex::parse_header(&FIXTURE[..256], FIXTURE.len() as u64).unwrap();
    let table_range = header.section_table_range();
    assert!(table_range.length() <= 8192);
    let table = &FIXTURE
        [table_range.offset() as usize..table_range.offset() as usize + table_range.length()];
    let index = BookIndex::parse(header, table).unwrap();

    let mut scratch = [0u8; 8192];
    let mut book = Book::open(SliceSource::new(FIXTURE), &mut scratch).unwrap();
    assert_eq!(index.page_count(), book.page_count());

    let profile_range = index.display_profile_range();
    let profile_bytes = &FIXTURE
        [profile_range.offset() as usize..profile_range.offset() as usize + profile_range.length()];
    assert_eq!(
        index.parse_display_profile(profile_bytes).unwrap(),
        book.display_profile(&mut scratch).unwrap()
    );

    let page_number = book.page_number(0).unwrap();
    let page_range = index.page_record_range(0).unwrap();
    let page_bytes =
        &FIXTURE[page_range.offset() as usize..page_range.offset() as usize + page_range.length()];
    assert_eq!(
        index.parse_page(0, page_bytes).unwrap(),
        book.page(page_number, &mut scratch).unwrap()
    );

    let chunk_range = index.chunk_record_range(0).unwrap();
    let chunk_bytes = &FIXTURE
        [chunk_range.offset() as usize..chunk_range.offset() as usize + chunk_range.length()];
    let chunk = index.parse_chunk(chunk_bytes).unwrap();
    assert_eq!(
        chunk,
        book.chunk(book.chunk_record_number(0).unwrap(), &mut scratch)
            .unwrap()
    );
    let data_range = index
        .page_data_range(chunk.offset.get(), chunk.compressed_length.get())
        .unwrap();
    let mut via_book = vec![0; data_range.length()];
    book.read_chunk(chunk, &mut via_book).unwrap();
    assert_eq!(
        &FIXTURE[data_range.offset() as usize..data_range.offset() as usize + data_range.length()],
        via_book
    );

    let rendition_range = index.rendition_identity_range();
    let rendition_bytes = &FIXTURE[rendition_range.offset() as usize
        ..rendition_range.offset() as usize + rendition_range.length()];
    assert_eq!(
        index.parse_rendition_identity(rendition_bytes).unwrap(),
        book.rendition_identity(&mut scratch).unwrap()
    );
}

#[test]
fn source_independent_index_rejects_out_of_range_records_and_short_bytes() {
    let header = BookIndex::parse_header(&FIXTURE[..256], FIXTURE.len() as u64).unwrap();
    let table_range = header.section_table_range();
    let table = &FIXTURE
        [table_range.offset() as usize..table_range.offset() as usize + table_range.length()];
    let index = BookIndex::parse(header, table).unwrap();

    assert!(index.page_record_range(index.page_count()).is_err());
    assert!(index.chunk_record_range(index.chunk_count()).is_err());
    assert!(index.parse_page(0, &[0; 79]).is_err());
    assert!(index.parse_chunk(&[0; 23]).is_err());
}
