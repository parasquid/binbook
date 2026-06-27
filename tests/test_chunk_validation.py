import json
from pathlib import Path

from binbook.cli import main
from binbook.constants import PageKind, UINT32_MAX
from binbook.page_compiler import encoded_page
from binbook.pixels import pack_gray2
from binbook.profiles import get_profile
from binbook.reader import BinBookReader
from binbook.writer import build_binbook


def test_inspect_includes_chunk_and_transition_counts(tmp_path, capsys):
    profile = get_profile("xteink-x4-portrait")
    white = pack_gray2([3] * (480 * 800), 480, 800)
    black = pack_gray2([0] * (480 * 800), 480, 800)
    path = tmp_path / "inspect.binbook"

    path.write_bytes(
        build_binbook(
            [
                encoded_page(white, PageKind.TEXT, UINT32_MAX),
                encoded_page(black, PageKind.TEXT, UINT32_MAX),
            ],
            profile,
            source_name="inspect-test",
        )
    )

    assert main(["inspect", str(path), "--json", "--validate"]) == 0
    payload = json.loads(capsys.readouterr().out)

    assert payload["chunk_count"] == 180
    assert payload["transition_count"] == 2


def test_validate_rejects_chunk_outside_page_data(tmp_path):
    from binbook.constants import SectionId
    from binbook.structs import (
        HEADER_SIZE,
        SECTION_ENTRY_SIZE,
        BinBookHeader,
        SectionEntry,
        PAGE_CHUNK_INDEX_ENTRY_SIZE,
    )
    import struct

    profile = get_profile("xteink-x4-portrait")
    white = pack_gray2([3] * (480 * 800), 480, 800)
    path = tmp_path / "invalid_chunks.binbook"

    path.write_bytes(
        build_binbook(
            [
                encoded_page(white, PageKind.TEXT, UINT32_MAX),
            ],
            profile,
            source_name="chunk-validation",
        )
    )

    book = bytearray(path.read_bytes())
    header = BinBookHeader.unpack(bytes(book[:HEADER_SIZE]))
    chunk_section_offset = None
    for index in range(header.section_count):
        entry_offset = header.section_table_offset + index * SECTION_ENTRY_SIZE
        entry = SectionEntry.unpack(bytes(book), entry_offset)
        if entry.section_id == SectionId.PAGE_CHUNK_INDEX:
            chunk_section_offset = entry.offset
            break
    assert chunk_section_offset is not None
    page_data_offset_field = chunk_section_offset + 16
    struct.pack_into("<I", book, page_data_offset_field, 999_999)
    path.write_bytes(bytes(book))

    reader = BinBookReader.open(path, validate=False)
    errors = []
    try:
        reader.validate()
    except ValueError as e:
        errors.append(str(e))
    assert any(
        "PAGE_DATA" in e or "outside" in e.lower() or "chunk" in e.lower()
        for e in errors
    )
