from __future__ import annotations

from pathlib import Path
import zipfile

from binbook.epub import read_epub


def test_read_epub_extracts_metadata_hashes_and_spine(tmp_path: Path):
    epub_path = tmp_path / "book.epub"
    _write_minimal_epub(epub_path)

    book = read_epub(epub_path)

    assert book.path == epub_path
    assert book.metadata.title == "Example Book"
    assert book.metadata.author == "Ada Writer"
    assert book.metadata.language == "en"
    assert book.metadata.package_identifier == "urn:uuid:example-book"
    assert book.file_size == epub_path.stat().st_size
    assert len(book.md5) == 16
    assert len(book.sha256) == 32
    assert [item.idref for item in book.spine] == ["chap1", "chap2"]
    assert [item.href for item in book.spine] == ["Text/chapter1.xhtml", "Text/chapter2.xhtml"]
    assert book.spine[0].html.startswith("<html")
    assert book.manifest["cover"].media_type == "image/png"


def test_rough_page_sequence_follows_spine_order(tmp_path: Path):
    epub_path = tmp_path / "book.epub"
    _write_minimal_epub(epub_path)

    book = read_epub(epub_path)
    pages = book.rough_page_sequence()

    assert [page.source_spine_index for page in pages] == [0, 1]
    assert [page.href for page in pages] == ["Text/chapter1.xhtml", "Text/chapter2.xhtml"]
    assert pages[0].text == "Chapter One First paragraph."
    assert pages[1].text == "Chapter Two Second paragraph."


def test_rough_page_sequence_ignores_head_style_and_script_text(tmp_path: Path):
    epub_path = tmp_path / "book.epub"
    _write_minimal_epub(
        epub_path,
        chapter1_html="""<html>
<head>
  <title>Cover Page</title>
  <style>@page { margin: 0; } body { padding: 0; }</style>
  <script>window.fake = true;</script>
</head>
<body><h1>Chapter One</h1><p>First paragraph.</p></body>
</html>""",
    )

    book = read_epub(epub_path)
    pages = book.rough_page_sequence()

    assert pages[0].text == "Chapter One First paragraph."


def test_read_epub_extracts_ncx_nav_points(tmp_path: Path):
    epub_path = tmp_path / "book.epub"
    _write_minimal_epub(epub_path, nav_kind="ncx")

    book = read_epub(epub_path)

    assert [(point.title, point.full_path) for point in book.nav_points] == [
        ("Chapter One", "OEBPS/Text/chapter1.xhtml"),
        ("Chapter Two", "OEBPS/Text/chapter2.xhtml"),
    ]


def _write_minimal_epub(
    path: Path,
    *,
    chapter1_html: str = "<html><body><h1>Chapter One</h1><p>First paragraph.</p></body></html>",
    nav_kind: str | None = None,
) -> None:
    with zipfile.ZipFile(path, "w") as zf:
        zf.writestr("mimetype", "application/epub+zip")
        zf.writestr(
            "META-INF/container.xml",
            """<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
""",
        )
        zf.writestr(
            "OEBPS/content.opf",
            """<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="bookid" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">urn:uuid:example-book</dc:identifier>
    <dc:title>Example Book</dc:title>
    <dc:creator>Ada Writer</dc:creator>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="chap1" href="Text/chapter1.xhtml" media-type="application/xhtml+xml"/>
    <item id="chap2" href="Text/chapter2.xhtml" media-type="application/xhtml+xml"/>
    <item id="cover" href="Images/cover.png" media-type="image/png"/>
    {nav_item}
  </manifest>
  <spine{spine_toc}>
    <itemref idref="chap1"/>
    <itemref idref="chap2"/>
  </spine>
</package>
""".format(
                nav_item=(
                    '<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>'
                    if nav_kind == "ncx"
                    else ""
                ),
                spine_toc=' toc="ncx"' if nav_kind == "ncx" else "",
            ),
        )
        zf.writestr("OEBPS/Text/chapter1.xhtml", chapter1_html)
        zf.writestr("OEBPS/Text/chapter2.xhtml", "<html><body><h1>Chapter Two</h1><p>Second paragraph.</p></body></html>")
        zf.writestr("OEBPS/Images/cover.png", b"not-a-real-png-for-parser")
        if nav_kind == "ncx":
            zf.writestr(
                "OEBPS/toc.ncx",
                """<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/">
  <navMap>
    <navPoint id="navPoint-1" playOrder="1">
      <navLabel><text>Chapter One</text></navLabel>
      <content src="Text/chapter1.xhtml#start"/>
    </navPoint>
    <navPoint id="navPoint-2" playOrder="2">
      <navLabel><text>Chapter Two</text></navLabel>
      <content src="Text/chapter2.xhtml"/>
    </navPoint>
  </navMap>
</ncx>
""",
            )
