mod common;

use std::io::{self, Cursor, Seek, SeekFrom, Write};

use binbook_compiler::{compile, CompileError, CompileOptions, CompileSource, NamedInput};

#[test]
fn preserves_output_sink_error_category() {
    let inputs = [NamedInput {
        name: "page.png",
        bytes: common::IMAGE,
    }];
    let error = compile(
        CompileSource::ImageSequence(&inputs),
        &CompileOptions::default(),
        &mut FailingSink,
        &mut common::Events::default(),
    )
    .unwrap_err();
    assert!(matches!(error, CompileError::Output(_)));
}

struct FailingSink;
impl Write for FailingSink {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("injected"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Seek for FailingSink {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}

#[test]
fn rejects_empty_sequences_without_partial_output() {
    let mut output = Cursor::new(Vec::new());
    let error = compile(
        CompileSource::ImageSequence(&[]),
        &CompileOptions::default(),
        &mut output,
        &mut common::Events::default(),
    )
    .unwrap_err();
    assert!(matches!(error, CompileError::EmptySource));
    assert!(output.into_inner().is_empty());
}
