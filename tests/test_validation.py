from __future__ import annotations

import struct
from pathlib import Path

import pytest

from binbook.constants import PixelFormat, SectionId
from binbook.profiles import XTEINK_X4_PORTRAIT
from binbook.reader import BinBookReader
from binbook.rle import encode_packbits
from binbook.structs import HEADER_SIZE, SECTION_ENTRY_SIZE, BinBookHeader, SectionEntry
from binbook.writer import EncodedPage, build_binbook


def test_rejects_unsupported_required_reader_feature(tmp_path: Path):
    book = bytearray(_book_bytes())
    section = _section(book, SectionId.READER_REQUIREMENTS)
    current = struct.unpack_from("<Q", book, section.offset + 8)[0]
    struct.pack_into("<Q", book, section.offset + 8, current | (1 << 63))
    _patch_section_crc(book, SectionId.READER_REQUIREMENTS, 0)

    path = _write(tmp_path, book)

    with pytest.raises(ValueError, match="unsupported required reader features"):
        BinBookReader.open(path)


def test_rejects_invalid_string_ref_bounds(tmp_path: Path):
    book = bytearray(_book_bytes())
    section = _section(book, SectionId.DISPLAY_PROFILE)
    struct.pack_into("<II", book, section.offset, 999_999, 4)
    _patch_section_crc(book, SectionId.DISPLAY_PROFILE, 0)

    path = _write(tmp_path, book)

    with pytest.raises(ValueError, match="StringRef is outside"):
        BinBookReader.open(path)


def test_rejects_layout_dimensions_that_do_not_match_display_profile(tmp_path: Path):
    book = bytearray(_book_bytes())
    section = _section(book, SectionId.LAYOUT_PROFILE)
    struct.pack_into("<H", book, section.offset, 481)
    _patch_section_crc(book, SectionId.LAYOUT_PROFILE, 0)

    path = _write(tmp_path, book)

    with pytest.raises(ValueError, match="LayoutProfile full page dimensions"):
        BinBookReader.open(path)


def test_rejects_layout_content_box_inconsistent_with_margins(tmp_path: Path):
    book = bytearray(_book_bytes())
    section = _section(book, SectionId.LAYOUT_PROFILE)
    struct.pack_into("<H", book, section.offset + 20, 479)
    _patch_section_crc(book, SectionId.LAYOUT_PROFILE, 0)

    path = _write(tmp_path, book)

    with pytest.raises(ValueError, match="LayoutProfile content box"):
        BinBookReader.open(path)


def test_rejects_page_data_offset_before_metadata_end(tmp_path: Path):
    book = bytearray(_book_bytes())
    metadata_end = _section(book, SectionId.NAV_INDEX).offset
    invalid_offset = metadata_end - 1
    struct.pack_into("<Q", book, 44, invalid_offset)
    _patch_section_offset(book, SectionId.PAGE_DATA, invalid_offset)

    path = _write(tmp_path, book)

    with pytest.raises(ValueError, match="page_data_offset is before end of metadata"):
        BinBookReader.open(path)


def test_rejects_gray4_page_for_x4_profile(tmp_path: Path):
    book = bytearray(_book_bytes())
    page_index = _section(book, SectionId.PAGE_INDEX)
    struct.pack_into("<H", book, page_index.offset + 6, PixelFormat.GRAY4_PACKED)
    _patch_section_crc(book, SectionId.PAGE_INDEX, 0)

    path = _write(tmp_path, book)

    with pytest.raises(ValueError, match="unsupported page pixel format"):
        BinBookReader.open(path)


def test_rejects_section_crc_mismatch_when_nonzero(tmp_path: Path):
    book = bytearray(_book_bytes())
    section = _section(book, SectionId.BOOK_METADATA)
    book[section.offset] ^= 0x01

    path = _write(tmp_path, book)

    with pytest.raises(ValueError, match="section BOOK_METADATA CRC32 mismatch"):
        BinBookReader.open(path)


def _book_bytes() -> bytes:
    packed_white_page = bytes([0xFF]) * 96_000
    compressed = encode_packbits(packed_white_page)
    page = EncodedPage(compressed=compressed, uncompressed_size=len(packed_white_page), page_crc32=0)
    return build_binbook([page], XTEINK_X4_PORTRAIT, source_name="validation")


def _write(tmp_path: Path, book: bytes | bytearray) -> Path:
    path = tmp_path / "corrupt.binbook"
    path.write_bytes(bytes(book))
    return path


def _section(book: bytes | bytearray, section_id: SectionId) -> SectionEntry:
    header = BinBookHeader.unpack(bytes(book[:HEADER_SIZE]))
    for index in range(header.section_count):
        entry = SectionEntry.unpack(bytes(book), header.section_table_offset + index * SECTION_ENTRY_SIZE)
        if entry.section_id == section_id:
            return entry
    raise AssertionError(f"missing section {section_id.name}")


def _patch_section_offset(book: bytearray, section_id: SectionId, offset: int) -> None:
    header = BinBookHeader.unpack(bytes(book[:HEADER_SIZE]))
    for index in range(header.section_count):
        entry_offset = header.section_table_offset + index * SECTION_ENTRY_SIZE
        entry = SectionEntry.unpack(bytes(book), entry_offset)
        if entry.section_id == section_id:
            struct.pack_into("<Q", book, entry_offset + 4, offset)
            return
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
