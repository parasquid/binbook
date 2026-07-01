mod fixture;

use std::fs;

#[test]
fn encodes_image_file_and_mixed_directory_without_partial_files() {
    let root = fixture::temp_dir("images");
    let single = root.join("single.svg");
    fs::write(&single, fixture::SVG).unwrap();
    let single_out = root.join("single.binbook");
    let output = fixture::run([
        "encode",
        single.to_str().unwrap(),
        "-o",
        single_out.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(single_out.exists());

    let pages = root.join("pages");
    fs::create_dir(&pages).unwrap();
    fs::write(pages.join("02.svg"), fixture::SVG).unwrap();
    fs::write(pages.join("01.svg"), fixture::SVG).unwrap();
    fs::write(pages.join("skip.txt"), b"unsupported").unwrap();
    let directory_out = root.join("directory.binbook");
    let output = fixture::run([
        "encode",
        pages.to_str().unwrap(),
        "-o",
        directory_out.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("skipping"));
    assert_eq!(fixture::page_count(&directory_out), 2);
    assert!(!fixture::temporary_files(&root));
}

#[test]
fn encodes_epub_and_rejects_mismatch_unsupported_and_all_skipped() {
    let root = fixture::temp_dir("formats");
    let epub = root.join("book.epub");
    fs::write(&epub, fixture::epub()).unwrap();
    let book = root.join("book.binbook");
    let output = fixture::run([
        "encode",
        epub.to_str().unwrap(),
        "-o",
        book.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mismatch = root.join("mismatch.binbook");
    let output = fixture::run([
        "encode",
        epub.to_str().unwrap(),
        "-o",
        mismatch.to_str().unwrap(),
        "--input-format",
        "image",
    ]);
    assert!(!output.status.success());
    assert!(!mismatch.exists());

    let bad = root.join("bad.txt");
    let unsupported = root.join("unsupported.binbook");
    fs::write(&bad, b"bad").unwrap();
    fs::write(&unsupported, b"existing").unwrap();
    let output = fixture::run([
        "encode",
        bad.to_str().unwrap(),
        "-o",
        unsupported.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert_eq!(fs::read(&unsupported).unwrap(), b"existing");
    let malformed_epub = root.join("malformed.epub");
    let preserved = root.join("preserved.binbook");
    fs::write(&malformed_epub, b"PK\x03\x04application/epub+zip").unwrap();
    fs::write(&preserved, b"preserve me").unwrap();
    let output = fixture::run([
        "encode",
        malformed_epub.to_str().unwrap(),
        "-o",
        preserved.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert_eq!(fs::read(&preserved).unwrap(), b"preserve me");
    let empty = root.join("empty");
    fs::create_dir(&empty).unwrap();
    fs::write(empty.join("bad.txt"), b"bad").unwrap();
    let empty_out = root.join("empty.binbook");
    let output = fixture::run([
        "encode",
        empty.to_str().unwrap(),
        "-o",
        empty_out.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert!(!empty_out.exists());
    assert!(!fixture::temporary_files(&root));
}

#[test]
fn inspect_json_decode_and_error_paths_are_process_clean() {
    let root = fixture::temp_dir("read");
    let source = root.join("page.svg");
    let book = root.join("page.binbook");
    fs::write(&source, fixture::SVG).unwrap();
    assert!(fixture::run([
        "encode",
        source.to_str().unwrap(),
        "-o",
        book.to_str().unwrap()
    ])
    .status
    .success());

    let inspect = fixture::run([
        "inspect",
        book.to_str().unwrap(),
        "--validate",
        "--strict",
        "--json",
    ]);
    assert!(inspect.status.success());
    assert!(inspect.stderr.is_empty());
    let json: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(json["page_count"], 1);
    assert_eq!(json["valid"], true);

    let png = root.join("page.png");
    assert!(fixture::run([
        "decode",
        book.to_str().unwrap(),
        "--page",
        "0",
        "-o",
        png.to_str().unwrap()
    ])
    .status
    .success());
    assert!(fs::read(&png).unwrap().starts_with(b"\x89PNG"));
    let invalid_page = root.join("invalid.png");
    assert!(!fixture::run([
        "decode",
        book.to_str().unwrap(),
        "--page",
        "9",
        "-o",
        invalid_page.to_str().unwrap()
    ])
    .status
    .success());
    assert!(!invalid_page.exists());

    let corrupt = root.join("corrupt.binbook");
    fs::write(&corrupt, b"not a book").unwrap();
    assert!(
        !fixture::run(["inspect", corrupt.to_str().unwrap(), "--strict"])
            .status
            .success()
    );
}
