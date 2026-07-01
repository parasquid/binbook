mod common;

use binbook_core::{BookMetadata, DisplayProfile};

#[test]
fn reads_real_profile_and_book_metadata_strings() {
    let mut book = common::open_fixture();
    let mut record = [0_u8; 128];
    let profile: DisplayProfile = book.display_profile(&mut record).unwrap();
    assert_eq!(profile.logical_width, 480);
    assert_eq!(profile.logical_height, 800);
    assert_eq!(profile.physical_width, 800);
    assert_eq!(profile.physical_height, 480);
    assert_eq!(profile.logical_to_physical_rotation, 270);

    let mut text = [0_u8; 32];
    assert_eq!(
        book.read_string(profile.profile_id, &mut text).unwrap(),
        b"xteink-x4-portrait"
    );

    let metadata: BookMetadata = book.book_metadata(&mut record).unwrap();
    assert_eq!(
        book.read_string(metadata.title, &mut text).unwrap(),
        b""
    );
    assert_eq!(metadata.series_index_milli, 0);
}

#[test]
fn string_reads_report_exact_buffer_sizes() {
    let mut book = common::open_fixture();
    let mut record = [0_u8; 128];
    let profile = book.display_profile(&mut record).unwrap();
    let mut text = [0_u8; 17];
    assert_eq!(
        book.read_string(profile.profile_id, &mut text),
        Err(binbook_core::Error::BufferTooSmall {
            required: 18,
            provided: 17,
        })
    );
}
