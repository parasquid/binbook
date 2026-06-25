from binbook.constants import MAGIC, SectionId
from binbook.structs import (
    CHAPTER_INDEX_ENTRY_SIZE,
    HEADER_SIZE,
    NAV_INDEX_ENTRY_SIZE,
    PAGE_CHUNK_INDEX_ENTRY_SIZE,
    PAGE_INDEX_ENTRY_SIZE,
    PAGE_TRANSITION_INDEX_ENTRY_SIZE,
    SECTION_ENTRY_SIZE,
    BinBookHeader,
    ChapterIndexEntry,
    NavIndexEntry,
    PageChunkIndexEntry,
    PageIndexEntry,
    PageTransitionIndexEntry,
    PlaneDir,
    SectionEntry,
    StringRef,
)


def test_fixed_struct_sizes_match_v02_spec():
    assert HEADER_SIZE == 256
    assert SECTION_ENTRY_SIZE == 40
    assert PAGE_INDEX_ENTRY_SIZE == 128
    assert NAV_INDEX_ENTRY_SIZE == 48
    assert CHAPTER_INDEX_ENTRY_SIZE == 32


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
    assert data[8:12] == bytes(4)
    assert parsed.file_size == 1000
    assert parsed.section_table_offset == 256
    assert parsed.section_count == 1
    assert set(data[76:]) == {0}


def test_section_page_and_nav_entries_roundtrip():
    section = SectionEntry(SectionId.PAGE_DATA, 65536, 10, 0, 0, 123)
    plane = PlaneDir(
        bitmap=0x07,
        compression=[1, 1, 2, 0],
        offsets=[0, 1000, 2000, 0],
        sizes=[500, 500, 800, 0],
    )
    page = PageIndexEntry(
        page_number=1,
        page_kind=2,
        pixel_format=2,
        compression_method=1,
        update_hint=0,
        page_flags=1,
        page_crc32=99,
        stored_width=480,
        stored_height=800,
        source_spine_index=0xFFFFFFFF,
        chapter_nav_index=0xFFFFFFFF,
        progress_start_ppm=0,
        progress_end_ppm=500000,
        plane_dir=plane,
    )
    nav = NavIndexEntry(nav_index=0, nav_type=1, title=StringRef(3, 4), target_page_number=0)
    chapter = ChapterIndexEntry(
        chapter_index=0,
        nav_index=2,
        title=StringRef(3, 4),
        target_page_number=6,
        level=1,
        nav_type=3,
        source_spine_index=5,
    )

    assert SectionEntry.unpack(section.pack()) == section
    page_data = page.pack()
    assert len(page_data) == 128
    restored = PageIndexEntry.unpack(page_data)
    assert restored.page_number == 1
    assert restored.plane_dir.bitmap == 0x07
    assert restored.plane_dir.compression[2] == 2
    assert restored.plane_dir.offsets[1] == 1000
    assert restored.plane_dir.sizes[2] == 800
    assert NavIndexEntry.unpack(nav.pack()) == nav
    assert ChapterIndexEntry.unpack(chapter.pack()) == chapter


def test_page_chunk_index_entry_roundtrip():
    entry = PageChunkIndexEntry(
        page_number=7,
        plane_slot=2,
        chunk_index=29,
        row_start=464,
        row_count=16,
        page_data_offset=123456,
        compressed_size=321,
        uncompressed_size=1600,
    )

    restored = PageChunkIndexEntry.unpack(entry.pack())

    assert restored == entry
    assert len(entry.pack()) == PAGE_CHUNK_INDEX_ENTRY_SIZE


def test_page_transition_index_entry_roundtrip():
    entry = PageTransitionIndexEntry(
        from_page_number=4,
        to_page_number=5,
        changed_chunk_mask=0b10101,
        first_changed_chunk=0,
        changed_chunk_count=5,
    )

    restored = PageTransitionIndexEntry.unpack(entry.pack())

    assert restored == entry
    assert len(entry.pack()) == PAGE_TRANSITION_INDEX_ENTRY_SIZE
