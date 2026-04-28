from binbook.constants import MAGIC, SectionId
from binbook.structs import (
    HEADER_SIZE,
    NAV_INDEX_ENTRY_SIZE,
    PAGE_INDEX_ENTRY_SIZE,
    SECTION_ENTRY_SIZE,
    BinBookHeader,
    NavIndexEntry,
    PageIndexEntry,
    SectionEntry,
    StringRef,
)


def test_fixed_struct_sizes_match_v01_spec():
    assert HEADER_SIZE == 256
    assert SECTION_ENTRY_SIZE == 40
    assert PAGE_INDEX_ENTRY_SIZE == 76
    assert NAV_INDEX_ENTRY_SIZE == 48


def test_header_roundtrips_and_zero_fills_reserved_bytes():
    header = BinBookHeader(
        file_size=1000,
        section_table_offset=256,
        section_table_length=40,
        section_count=1,
        page_data_offset=65536,
        page_data_length=12,
    )

    data = header.pack()
    parsed = BinBookHeader.unpack(data)

    assert data[:8] == MAGIC
    assert parsed.file_size == 1000
    assert parsed.section_table_offset == 256
    assert parsed.section_count == 1
    assert set(data[76:]) == {0}


def test_section_page_and_nav_entries_roundtrip():
    section = SectionEntry(SectionId.PAGE_DATA, 65536, 10, 0, 0, 123)
    page = PageIndexEntry(
        page_number=1,
        page_kind=2,
        pixel_format=2,
        compression_method=1,
        relative_blob_offset=5,
        compressed_size=10,
        uncompressed_size=20,
        page_crc32=99,
        stored_width=480,
        stored_height=800,
        source_spine_index=0xFFFFFFFF,
        chapter_nav_index=0xFFFFFFFF,
        progress_start_ppm=0,
        progress_end_ppm=500000,
    )
    nav = NavIndexEntry(nav_index=0, nav_type=1, title=StringRef(3, 4), target_page_number=0)

    assert SectionEntry.unpack(section.pack()) == section
    assert PageIndexEntry.unpack(page.pack()) == page
    assert NavIndexEntry.unpack(nav.pack()) == nav
