mod common;

use binbook_core::{Book, Error, FormatError, SliceSource};

const EXPECTED_RENDITION_HASH: [u8; 32] = [
    0x8e, 0xb7, 0x90, 0x3b, 0xfa, 0x65, 0x1b, 0xa4, 0x89, 0x52, 0xeb, 0xb8, 0x35, 0x5b, 0xc4, 0x1d,
    0x7c, 0x49, 0xb6, 0x98, 0x17, 0xb0, 0x19, 0x4d, 0x58, 0x41, 0xda, 0x87, 0x83, 0xd9, 0x90, 0x8b,
];

const SECTION_TABLE_OFFSET: usize = 256;
const SECTION_RECORD_SIZE: usize = 40;
const RENDITION_SECTION_INDEX: usize = 6;

fn rendition_table_entry_offset() -> usize {
    SECTION_TABLE_OFFSET + RENDITION_SECTION_INDEX * SECTION_RECORD_SIZE
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

#[test]
fn reads_exact_rendition_hash_from_fixture() {
    let mut book = common::open_fixture();
    let mut scratch = [0_u8; 312];

    let identity = book.rendition_identity(&mut scratch).unwrap();

    assert_eq!(identity.rendition_hash, EXPECTED_RENDITION_HASH);
}

#[test]
fn rejects_short_rendition_identity_section() {
    let mut fixture = common::FIXTURE.to_vec();
    let entry = rendition_table_entry_offset();
    fixture[entry + 12..entry + 20].copy_from_slice(&31_u64.to_le_bytes());
    let mut section_scratch = [0_u8; 1024];
    let mut book = Book::open(SliceSource::new(&fixture), &mut section_scratch).unwrap();
    let mut record = [0_u8; 312];

    assert_eq!(
        book.rendition_identity(&mut record),
        Err(Error::Format(FormatError::InvalidSection))
    );
}

#[test]
fn rejects_all_zero_rendition_hash() {
    let mut fixture = common::FIXTURE.to_vec();
    let entry = rendition_table_entry_offset();
    let section_offset = read_u64(&fixture, entry + 4) as usize;
    fixture[section_offset..section_offset + 32].fill(0);
    let mut section_scratch = [0_u8; 1024];
    let mut book = Book::open(SliceSource::new(&fixture), &mut section_scratch).unwrap();
    let mut record = [0_u8; 312];

    assert_eq!(
        book.rendition_identity(&mut record),
        Err(Error::Format(FormatError::InvalidSection))
    );
}

#[test]
fn reports_required_prefix_buffer_size() {
    let mut book = common::open_fixture();
    let mut short = [0_u8; 31];

    assert_eq!(
        book.rendition_identity(&mut short),
        Err(Error::BufferTooSmall {
            required: 32,
            provided: 31,
        })
    );
}
