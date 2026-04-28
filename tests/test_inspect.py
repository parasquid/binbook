from __future__ import annotations

import json
import struct
from pathlib import Path

from binbook.cli import main
from binbook.constants import SectionId
from binbook.profiles import XTEINK_X4_PORTRAIT
from binbook.rle import encode_packbits
from binbook.structs import HEADER_SIZE, SECTION_ENTRY_SIZE, BinBookHeader, SectionEntry
from binbook.writer import EncodedPage, build_binbook


def test_inspect_json_outputs_structural_summary(tmp_path: Path, capsys):
    path = tmp_path / "book.binbook"
    path.write_bytes(_book_bytes())

    assert main(["inspect", str(path), "--json", "--validate"]) == 0

    payload = json.loads(capsys.readouterr().out)
    assert payload["format"] == "BinBook"
    assert payload["version"] == {"major": 0, "minor": 1}
    assert payload["page_count"] == 1
    assert payload["validation"] == {"ok": True, "errors": []}
    assert payload["sections"][0]["name"] == "STRING_TABLE"
    assert payload["pages"][0]["pixel_format"] == 2


def test_inspect_strict_validation_reports_all_detected_errors(tmp_path: Path, capsys):
    book = bytearray(_book_bytes())
    _patch_section_crc(book, SectionId.DISPLAY_PROFILE, 0)
    display = _section(book, SectionId.DISPLAY_PROFILE)
    layout = _section(book, SectionId.LAYOUT_PROFILE)
    struct.pack_into("<HH", book, display.offset + 24, 481, 800)
    struct.pack_into("<H", book, layout.offset + 20, 479)
    _patch_section_crc(book, SectionId.LAYOUT_PROFILE, 0)
    path = tmp_path / "invalid.binbook"
    path.write_bytes(book)

    assert main(["inspect", str(path), "--validate", "--strict"]) == 1

    out = capsys.readouterr().out
    assert "Validation: FAILED" in out
    assert "unsupported display profile dimensions" in out
    assert "LayoutProfile full page dimensions" in out
    assert "LayoutProfile content box" in out


def _book_bytes() -> bytes:
    packed_white_page = bytes([0xFF]) * 96_000
    compressed = encode_packbits(packed_white_page)
    page = EncodedPage(compressed=compressed, uncompressed_size=len(packed_white_page), page_crc32=0)
    return build_binbook([page], XTEINK_X4_PORTRAIT, source_name="inspect")


def _section(book: bytes | bytearray, section_id: SectionId) -> SectionEntry:
    header = BinBookHeader.unpack(bytes(book[:HEADER_SIZE]))
    for index in range(header.section_count):
        entry = SectionEntry.unpack(bytes(book), header.section_table_offset + index * SECTION_ENTRY_SIZE)
        if entry.section_id == section_id:
            return entry
    raise AssertionError(f"missing section {section_id.name}")


def _patch_section_crc(book: bytearray, section_id: SectionId, crc: int) -> None:
    header = BinBookHeader.unpack(bytes(book[:HEADER_SIZE]))
    for index in range(header.section_count):
        entry_offset = header.section_table_offset + index * SECTION_ENTRY_SIZE
        entry = SectionEntry.unpack(bytes(book), entry_offset)
        if entry.section_id == section_id:
            struct.pack_into("<I", book, entry_offset + 28, crc)
            return
    raise AssertionError(f"missing section {section_id.name}")
