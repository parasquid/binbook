use std::io::{Cursor, Write};

use sha1::{Digest, Sha1};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub struct Epub3Fixture {
    pub bytes: Vec<u8>,
    pub font_bytes: Vec<u8>,
}

pub fn epub3() -> Epub3Fixture {
    let identifier = "urn:uuid:12345678-1234-1234-1234-123456789abc";
    let font_bytes =
        include_bytes!("../../../binbook/assets/fonts/OpenDyslexic/OpenDyslexic-Regular.otf")
            .to_vec();
    let mut obfuscated = font_bytes.clone();
    let key: Vec<u8> = Sha1::digest(identifier.as_bytes()).to_vec();
    for (index, byte) in obfuscated.iter_mut().take(1_040).enumerate() {
        *byte ^= key[index % key.len()];
    }
    let opf = format!(
        r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="uid">{identifier}</dc:identifier><dc:title>Synthetic Three</dc:title><dc:creator>Ada Reader</dc:creator><dc:language>en</dc:language></metadata><manifest><item id="ch1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="css" href="styles/main.css" media-type="text/css"/><item id="png" href="images/pixel.png" media-type="image/png"/><item id="jpeg" href="images/pixel.jpg" media-type="image/jpeg"/><item id="webp" href="images/pixel.webp" media-type="image/webp"/><item id="svg" href="images/vector.svg" media-type="image/svg+xml"/><item id="otf" href="fonts/test.otf" media-type="font/otf"/><item id="ttf" href="fonts/test.ttf" media-type="font/ttf"/><item id="woff" href="fonts/test.woff" media-type="font/woff"/><item id="woff2" href="fonts/test.woff2" media-type="font/woff2"/></manifest><spine><itemref idref="ch1"/></spine></package>"#
    );
    let chapter = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="../styles/main.css"/><style>p { line-height: 1.4; font-size: 20px; }</style></head><body><h1 id="start">Heading</h1><p id="main" class="lead" style="font-weight: bold">Hello<img src="../images/pixel.png" alt="pixel"/><img src="../images/missing.png" alt="missing art"/></p><p class="hidden">hidden text</p></body></html>"#;
    let css = r#"@font-face { font-family: Embedded; src: url('../fonts/test.otf'); font-weight: 400; } p { font-size: 18px; float: left; } .lead { font-size: 19px; } #main { font-size: 21px; } .hidden { display: none; }"#;
    let nav = r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><nav epub:type="toc" xmlns:epub="http://www.idpf.org/2007/ops"><ol><li><a href="text/ch1.xhtml#start">Start</a><ol><li><a href="text/ch1.xhtml#main">Details</a></li></ol></li></ol></nav></body></html>"#;
    let encryption = r#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><EncryptedData><EncryptionMethod Algorithm="http://www.idpf.org/2008/embedding"/><CipherData><CipherReference URI="OPS/fonts/test.otf"/></CipherData></EncryptedData></encryption>"#;
    let bytes = build(&[
        ("OPS/package.opf", opf.as_bytes()),
        ("OPS/text/ch1.xhtml", chapter.as_bytes()),
        ("OPS/nav.xhtml", nav.as_bytes()),
        ("OPS/styles/main.css", css.as_bytes()),
        ("OPS/images/pixel.png", tiny_png()),
        ("OPS/images/pixel.jpg", b"\xff\xd8\xff\xd9"),
        ("OPS/images/pixel.webp", b"RIFF\x04\0\0\0WEBP"),
        (
            "OPS/images/vector.svg",
            b"<svg xmlns='http://www.w3.org/2000/svg'/>",
        ),
        ("OPS/fonts/test.otf", &obfuscated),
        ("OPS/fonts/test.ttf", &font_bytes),
        ("OPS/fonts/test.woff", b"wOFFsynthetic"),
        ("OPS/fonts/test.woff2", b"wOF2synthetic"),
        ("META-INF/encryption.xml", encryption.as_bytes()),
    ]);
    Epub3Fixture { bytes, font_bytes }
}

pub fn epub2() -> Vec<u8> {
    let opf = br#"<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="uid"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="uid">legacy-id</dc:identifier><dc:title>Legacy</dc:title><dc:language>en</dc:language></metadata><manifest><item id="ch" href="ch.xhtml" media-type="application/xhtml+xml"/><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest><spine toc="ncx"><itemref idref="ch"/></spine></package>"#;
    let ncx = br#"<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap><navPoint id="n1" playOrder="1"><navLabel><text>Legacy Chapter</text></navLabel><content src="ch.xhtml#legacy"/></navPoint></navMap></ncx>"#;
    build(&[
        ("OPS/package.opf", opf),
        (
            "OPS/ch.xhtml",
            b"<html><body><p id='legacy'>Legacy</p></body></html>",
        ),
        ("OPS/toc.ncx", ncx),
    ])
}

pub fn drm_epub() -> Vec<u8> {
    let opf = br#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="uid">drm</dc:identifier><dc:title>DRM</dc:title><dc:language>en</dc:language></metadata><manifest><item id="ch" href="ch.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="ch"/></spine></package>"#;
    let encryption = br#"<encryption><EncryptedData><EncryptionMethod Algorithm="urn:vendor:drm"/><CipherData><CipherReference URI="OPS/ch.xhtml"/></CipherData></EncryptedData></encryption>"#;
    build(&[
        ("OPS/package.opf", opf),
        ("OPS/ch.xhtml", b"<html><body>DRM</body></html>"),
        ("META-INF/encryption.xml", encryption),
    ])
}

fn build(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "mimetype",
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .unwrap();
    writer.write_all(b"application/epub+zip").unwrap();
    writer
        .start_file("META-INF/container.xml", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    for (path, bytes) in files {
        writer
            .start_file(*path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn tiny_png() -> &'static [u8] {
    b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\rIDATx\x9cc`\x00\x00\x00\x02\x00\x01\xe2!\xbc3\x00\x00\x00\x00IEND\xaeB`\x82"
}
