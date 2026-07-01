mod common;

use std::io::Cursor;

use binbook_core::{validate_all, Book, SliceSource, ValidationIssue, ValidationVisitor};

use common::builder;

#[derive(Default)]
struct Issues(Vec<ValidationIssue>);

impl ValidationVisitor for Issues {
    fn visit(&mut self, issue: ValidationIssue) {
        self.0.push(issue);
    }
}

#[test]
fn generated_book_opens_and_passes_strict_validation() {
    let mut output = Cursor::new(Vec::new());
    builder().write_to(&mut output).unwrap();
    let bytes = output.into_inner();

    let mut scratch = [0_u8; 1024];
    let book = Book::open(SliceSource::new(&bytes), &mut scratch).unwrap();
    assert_eq!(book.page_count(), 2);
    assert_eq!(book.chunk_count(), 180);
    assert_eq!(book.transition_count(), 2);
    assert_eq!(book.nav_count(), 1);
    assert_eq!(book.chapter_count(), 1);

    let mut issues = Issues::default();
    let mut record = [0_u8; 256];
    validate_all(
        SliceSource::new(&bytes),
        &mut scratch,
        &mut record,
        &mut issues,
    )
    .unwrap();
    assert_eq!(issues.0, []);
}
