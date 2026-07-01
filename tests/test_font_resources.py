from binbook.constants import SectionId
from binbook.structs import FONT_RESOURCE_INDEX_ENTRY_SIZE, FontResourceIndexEntry


def test_font_resource_index_record_roundtrips_all_fields():
    entry = FontResourceIndexEntry(
        font_index=0,
        source_kind=2,
        flags=0b1011,
        weight=700,
        stretch_milli=1000,
        style=1,
        family_offset=4,
        family_length=8,
        source_path_offset=12,
        source_path_length=16,
        sha256=bytes(range(32)),
        face_index=3,
    )

    packed = entry.pack()

    assert SectionId.FONT_RESOURCE_INDEX == 35
    assert len(packed) == FONT_RESOURCE_INDEX_ENTRY_SIZE == 80
    assert FontResourceIndexEntry.unpack(packed) == entry
