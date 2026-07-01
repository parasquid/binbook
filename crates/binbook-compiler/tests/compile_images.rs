mod common;

use std::io::Cursor;

use binbook_compiler::{compile, CompileOptions, CompilePhase, CompileSource, NamedInput};
use binbook_core::{validate_all, Book, SliceSource, ValidationIssue, ValidationVisitor};

#[derive(Default)]
struct Issues(Vec<ValidationIssue>);
impl ValidationVisitor for Issues {
    fn visit(&mut self, issue: ValidationIssue) {
        self.0.push(issue);
    }
}

#[test]
fn compiles_image_sequence_to_strict_decodable_book_with_progress() {
    let inputs = [
        NamedInput {
            name: "01.svg",
            bytes: common::IMAGE,
        },
        NamedInput {
            name: "02.svg",
            bytes: common::IMAGE,
        },
    ];
    let mut output = Cursor::new(Vec::new());
    let mut events = common::Events::default();
    let summary = compile(
        CompileSource::ImageSequence(&inputs),
        &CompileOptions::default(),
        &mut output,
        &mut events,
    )
    .unwrap();
    assert_eq!(summary.page_count, 2);
    assert_eq!(summary.output_bytes, output.get_ref().len() as u64);
    assert_eq!(events.phases.first(), Some(&CompilePhase::ReadSource));
    assert_eq!(events.phases.last(), Some(&CompilePhase::Validate));
    let bytes = output.into_inner();
    let mut scratch = [0; 1024];
    assert_eq!(
        Book::open(SliceSource::new(&bytes), &mut scratch)
            .unwrap()
            .page_count(),
        2
    );
    let mut issues = Issues::default();
    validate_all(
        SliceSource::new(&bytes),
        &mut scratch,
        &mut [0; 256],
        &mut issues,
    )
    .unwrap();
    assert!(issues.0.is_empty());
    let decoded = binbook_image::decode_book_page(&bytes, 1).unwrap();
    assert_eq!((decoded.width, decoded.height), (800, 480));
}
