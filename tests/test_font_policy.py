from __future__ import annotations

import io
import json
from pathlib import Path
import struct
import zipfile

from PIL import Image

from binbook.cli import main
from binbook.constants import SectionId
from binbook.fonts import get_font
from binbook.reader import BinBookReader
from binbook.structs import StringRef
from binbook.strings import read_string


def test_encode_records_selected_font_policy(tmp_path: Path, capsys):
    epub_path = tmp_path / "book.epub"
    output = tmp_path / "book.binbook"
    _write_minimal_epub(epub_path)

    assert main(["encode", str(epub_path), "-o", str(output), "--font-family", "literata"]) == 0
    reader = BinBookReader.open(output)

    font_policy = _section_bytes(reader, SectionId.FONT_POLICY)
    string_table = _section_bytes(reader, SectionId.STRING_TABLE)
    font_mode, font_flags = struct.unpack_from("<HH", font_policy, 0)
    font_sha256 = font_policy[4:36]
    font_name = read_string(string_table, StringRef.unpack(font_policy, 36))
    font_path = read_string(string_table, StringRef.unpack(font_policy, 44))
    renderer_name = read_string(string_table, StringRef.unpack(font_policy, 52))

    selected = get_font("literata")
    assert font_mode == 2
    assert font_flags & 1
    assert font_sha256 == selected.sha256
    assert font_name == "Literata"
    assert font_path == selected.stable_path
    assert renderer_name == "Pillow"

    assert main(["inspect", str(output), "--validate", "--json"]) == 0
    payload = json.loads(capsys.readouterr().out)
    assert payload["validation"]["ok"] is True


def test_sans_serif_alias_records_opendyslexic_font_policy(tmp_path: Path):
    epub_path = tmp_path / "book.epub"
    output = tmp_path / "book.binbook"
    _write_minimal_epub(epub_path)

    assert main(["encode", str(epub_path), "-o", str(output), "--font-family", "sans-serif"]) == 0
    reader = BinBookReader.open(output)
    font_policy = _section_bytes(reader, SectionId.FONT_POLICY)
    string_table = _section_bytes(reader, SectionId.STRING_TABLE)
    selected = get_font("sans-serif")

    assert font_policy[4:36] == selected.sha256
    assert read_string(string_table, StringRef.unpack(font_policy, 36)) == "OpenDyslexic"
    assert read_string(string_table, StringRef.unpack(font_policy, 44)) == selected.stable_path


def test_sans_serif_records_default_character_spacing(tmp_path: Path):
    epub_path = tmp_path / "book.epub"
    output = tmp_path / "book.binbook"
    _write_minimal_epub(epub_path)

    assert main(["encode", str(epub_path), "-o", str(output), "--font-family", "sans-serif"]) == 0
    reader = BinBookReader.open(output)
    typography_policy = _section_bytes(reader, SectionId.TYPOGRAPHY_POLICY)

    character_spacing_milli_em = struct.unpack_from("<i", typography_policy, 20)[0]

    assert character_spacing_milli_em == -30


def test_encode_rejects_unknown_font_family(tmp_path: Path):
    epub_path = tmp_path / "book.epub"
    output = tmp_path / "book.binbook"
    _write_minimal_epub(epub_path)

    assert main(["encode", str(epub_path), "-o", str(output), "--font-family", "papyrus"]) == 1


def _section_bytes(reader: BinBookReader, section_id: SectionId) -> bytes:
    section = reader.sections[section_id]
    return reader.data[section.offset : section.offset + section.length]


def _write_minimal_epub(path: Path) -> None:
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
    <dc:identifier id="bookid">urn:uuid:font-policy-book</dc:identifier>
    <dc:title>Font Policy Book</dc:title>
    <dc:creator>Ada Font</dc:creator>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="chap1" href="Text/chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="chap1"/>
  </spine>
</package>
""",
        )
        zf.writestr("OEBPS/Text/chapter1.xhtml", "<html><body><p>Font test.</p></body></html>")
