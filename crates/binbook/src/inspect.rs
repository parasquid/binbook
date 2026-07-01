use std::fs;
use std::path::Path;

use binbook_core::{validate_all, Book, SliceSource, ValidationIssue, ValidationVisitor};

use crate::error::io;
use crate::CliError;

pub fn run_inspect(path: &Path, validate: bool, strict: bool, json: bool) -> Result<(), CliError> {
    let bytes = fs::read(path).map_err(|source| io(path, source))?;
    let mut section_scratch = [0_u8; 1024];
    let mut book = Book::open(SliceSource::new(&bytes), &mut section_scratch)
        .map_err(|_| CliError::InvalidBook)?;
    let counts = (book.page_count(), book.nav_count(), book.chapter_count());
    let mut record = [0_u8; 256];
    let metadata = book
        .book_metadata(&mut record)
        .map_err(|_| CliError::InvalidBook)?;
    let title = read_text(&mut book, metadata.title)?;
    let author = read_text(&mut book, metadata.author)?;
    let language = read_text(&mut book, metadata.language)?;
    let mut issues = Issues::default();
    if validate || strict {
        validate_all(
            SliceSource::new(&bytes),
            &mut section_scratch,
            &mut [0_u8; 256],
            &mut issues,
        )
        .map_err(|_| CliError::InvalidBook)?;
    }
    let valid = issues.0.is_empty();
    if json {
        println!(
            "{}",
            serde_json::json!({
                "page_count": counts.0,
                "navigation_count": counts.1,
                "chapter_count": counts.2,
                "title": title,
                "author": author,
                "language": language,
                "valid": valid,
                "issues": issues.0.iter().map(|issue| format!("{:?}", issue.code)).collect::<Vec<_>>(),
            })
        );
    } else {
        println!(
            "pages={} navigation={} chapters={} title={title:?} author={author:?} language={language:?} valid={valid}",
            counts.0, counts.1, counts.2,
        );
        for issue in &issues.0 {
            println!(
                "issue={:?} section={:?} record={:?}",
                issue.code, issue.section_id, issue.record_index
            );
        }
    }
    if strict && !valid {
        Err(CliError::InvalidBook)
    } else {
        Ok(())
    }
}

fn read_text(
    book: &mut Book<SliceSource<'_>>,
    reference: binbook_core::StringRef,
) -> Result<String, CliError> {
    let mut bytes = vec![0_u8; reference.length as usize];
    let value = book
        .read_string(reference, &mut bytes)
        .map_err(|_| CliError::InvalidBook)?;
    String::from_utf8(value.to_vec()).map_err(|_| CliError::InvalidBook)
}

#[derive(Default)]
struct Issues(Vec<ValidationIssue>);

impl ValidationVisitor for Issues {
    fn visit(&mut self, issue: ValidationIssue) {
        self.0.push(issue);
    }
}
