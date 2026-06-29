mod common;

use binbook_core::{ChunkIndex, Error, FormatError, PlaneSlot};

#[test]
fn typed_identifiers_reject_out_of_range_values() {
    let book = common::open_fixture();
    assert_eq!(book.page_number(16), Err(FormatError::PageOutOfRange));
    assert_eq!(
        book.chunk_record_number(1440),
        Err(FormatError::ChunkOutOfRange)
    );
    assert_eq!(
        book.transition_number(30),
        Err(FormatError::TransitionOutOfRange)
    );
    assert_eq!(ChunkIndex::new(30, 30), Err(FormatError::ChunkOutOfRange));
    assert_eq!(
        PlaneSlot::try_from(4),
        Err(FormatError::PlaneSlotOutOfRange)
    );
}

#[test]
fn record_reads_report_exact_buffer_sizes() {
    let mut book = common::open_fixture();
    let mut record = [0_u8; 127];
    assert_eq!(
        book.page(book.page_number(0).unwrap(), &mut record),
        Err(Error::BufferTooSmall {
            required: 128,
            provided: 127,
        })
    );
}
