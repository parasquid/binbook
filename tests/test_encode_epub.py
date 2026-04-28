from __future__ import annotations

import io
import json
from pathlib import Path
import zipfile

from PIL import Image

from binbook.cli import main
from binbook.constants import PageKind, SectionId
from binbook.reader import BinBookReader


def test_encode_epub_creates_text_image_and_nav_pages(tmp_path: Path, capsys):
    epub_path = tmp_path / "book.epub"
    output = tmp_path / "book.binbook"
    decoded = tmp_path / "page0.png"
    _write_epub_with_text_image_and_nav(epub_path)

    assert main(["encode", str(epub_path), "-o", str(output)]) == 0
    reader = BinBookReader.open(output)

    assert [page.page_kind for page in reader.pages] == [PageKind.TEXT, PageKind.IMAGE, PageKind.TEXT, PageKind.TEXT]
    assert [page.source_spine_index for page in reader.pages] == [0, 0, 0, 1]
    assert reader.sections[SectionId.NAV_INDEX].record_count == 2

    assert main(["inspect", str(output), "--validate", "--json"]) == 0
    payload = json.loads(capsys.readouterr().out)
    assert payload["validation"]["ok"] is True
    assert payload["page_count"] == 4

    assert main(["decode", str(output), "--page", "0", "-o", str(decoded)]) == 0
    assert Image.open(decoded).size == (480, 800)


def _write_epub_with_text_image_and_nav(path: Path) -> None:
    image_bytes = io.BytesIO()
    Image.new("RGB", (160, 120), (32, 128, 220)).save(image_bytes, format="PNG")
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
    <dc:identifier id="bookid">urn:uuid:encode-book</dc:identifier>
    <dc:title>Encode Book</dc:title>
    <dc:creator>Ada Encoder</dc:creator>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="chap1" href="Text/chapter1.xhtml" media-type="application/xhtml+xml"/>
    <item id="chap2" href="Text/chapter2.xhtml" media-type="application/xhtml+xml"/>
    <item id="pic" href="Images/picture.png" media-type="image/png"/>
  </manifest>
  <spine>
    <itemref idref="chap1"/>
    <itemref idref="chap2"/>
  </spine>
</package>
""",
        )
        zf.writestr(
            "OEBPS/nav.xhtml",
            """<html><body><nav epub:type="toc"><ol>
<li><a href="Text/chapter1.xhtml">Chapter One</a></li>
<li><a href="Text/chapter2.xhtml">Chapter Two</a></li>
</ol></nav></body></html>""",
        )
        zf.writestr(
            "OEBPS/Text/chapter1.xhtml",
            '<html><body><h1>Chapter One</h1><p>Text before image.</p><img src="../Images/picture.png"/><p>Text after image.</p></body></html>',
        )
        zf.writestr("OEBPS/Text/chapter2.xhtml", "<html><body><h1>Chapter Two</h1><p>More text.</p></body></html>")
        zf.writestr("OEBPS/Images/picture.png", image_bytes.getvalue())
