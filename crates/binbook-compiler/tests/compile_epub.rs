mod common;

use std::io::{Cursor, Write};

use binbook_compiler::{compile, CompileOptions, CompileSource, NamedInput, SourceFormat};
use binbook_core::{Book, SliceSource};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

#[test]
fn compiles_epub_metadata_navigation_fonts_and_page() {
    let epub = epub();
    let mut output = Cursor::new(Vec::new());
    let mut events = common::Events::default();
    let summary = compile(
        CompileSource::Epub(NamedInput {
            name: "book.epub",
            bytes: &epub,
        }),
        &CompileOptions::default(),
        &mut output,
        &mut events,
    )
    .unwrap();
    assert_eq!(summary.source_format, SourceFormat::Epub);
    assert!(summary.warning_count >= 1);
    assert_eq!(summary.warning_count as usize, events.warnings.len());
    assert!(events
        .warnings
        .iter()
        .all(|warning| warning.resource.is_some()));
    let bytes = output.into_inner();
    let mut scratch = [0; 1024];
    let mut book = Book::open(SliceSource::new(&bytes), &mut scratch).unwrap();
    assert!(book.page_count() >= 1);
    assert_eq!(book.nav_count(), 1);
    let metadata = book.book_metadata(&mut scratch).unwrap();
    let mut title_scratch = [0; 64];
    let title = book
        .read_string(metadata.title, &mut title_scratch)
        .unwrap();
    assert_eq!(title, b"Compiler Fixture");
    assert_eq!(
        binbook_image::decode_book_page(&bytes, 0).unwrap().width,
        800
    );
}

fn epub() -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "mimetype",
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(b"application/epub+zip").unwrap();
    zip.start_file("META-INF/container.xml", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    let files: [(&str, &[u8]); 3] = [
        ("OPS/package.opf", br#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">fixture</dc:identifier><dc:title>Compiler Fixture</dc:title><dc:creator>Author</dc:creator><dc:language>en</dc:language></metadata><manifest><item id="ch" href="ch.xhtml" media-type="application/xhtml+xml"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest><spine><itemref idref="ch"/></spine></package>"#),
        ("OPS/ch.xhtml", br#"<html><head><style>p { float: left; }</style></head><body><h1 id="start">Chapter</h1><p>Hello compiler.</p></body></html>"#),
        ("OPS/nav.xhtml", br#"<html xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="ch.xhtml#start">Chapter</a></li></ol></nav></body></html>"#),
    ];
    for (name, bytes) in files {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
