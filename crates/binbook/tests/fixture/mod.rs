use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use binbook_core::{Book, SliceSource};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

pub const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="black"/></svg>"#;

pub fn run<const N: usize>(args: [&str; N]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_binbook"))
        .args(args)
        .output()
        .unwrap()
}

pub fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("binbook-{label}-{}-{nonce}", std::process::id()));
    fs::create_dir(&path).unwrap();
    path
}

pub fn temporary_files(root: &Path) -> bool {
    fs::read_dir(root)
        .unwrap()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
}

pub fn page_count(path: &Path) -> u32 {
    let bytes = fs::read(path).unwrap();
    Book::open(SliceSource::new(&bytes), &mut [0; 1024])
        .unwrap()
        .page_count()
}

pub fn epub() -> Vec<u8> {
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
    for (name, bytes) in [
        ("OPS/package.opf", br#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">fixture</dc:identifier><dc:title>CLI EPUB</dc:title><dc:language>en</dc:language></metadata><manifest><item id="ch" href="ch.xhtml" media-type="application/xhtml+xml"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest><spine><itemref idref="ch"/></spine></package>"#.as_slice()),
        ("OPS/ch.xhtml", br#"<html><body><h1 id="start">Chapter</h1><p>Hello.</p></body></html>"#.as_slice()),
        ("OPS/nav.xhtml", br#"<html xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="ch.xhtml#start">Chapter</a></li></ol></nav></body></html>"#.as_slice()),
    ] { zip.start_file(name, SimpleFileOptions::default()).unwrap(); zip.write_all(bytes).unwrap(); }
    zip.finish().unwrap().into_inner()
}
