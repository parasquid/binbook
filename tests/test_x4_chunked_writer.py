from binbook.constants import PageKind, UINT32_MAX
from binbook.page_compiler import encoded_page
from binbook.pixels import pack_gray2
from binbook.profiles import get_profile
from binbook.reader import BinBookReader
from binbook.writer import build_binbook


def test_x4_writer_emits_three_chunked_planes(tmp_path):
    profile = get_profile("xteink-x4-portrait")
    packed = pack_gray2([3] * (480 * 800), 480, 800)
    page = encoded_page(packed, PageKind.TEXT, UINT32_MAX)
    path = tmp_path / "chunked.binbook"

    path.write_bytes(build_binbook([page], profile, source_name="chunked-test"))
    reader = BinBookReader.open(path, validate=True)

    assert reader.pages[0].plane_dir.bitmap == 0x07
    assert reader.pages[0].plane_dir.sizes[0] > 0
    assert reader.pages[0].plane_dir.sizes[1] > 0
    assert reader.pages[0].plane_dir.sizes[2] > 0
    assert len(reader.page_chunks) == 90
    assert {entry.uncompressed_size for entry in reader.page_chunks} == {1600}


def test_adjacent_transition_index_marks_changed_chunks(tmp_path):
    profile = get_profile("xteink-x4-portrait")
    white = pack_gray2([3] * (480 * 800), 480, 800)
    black = pack_gray2([0] * (480 * 800), 480, 800)
    path = tmp_path / "transitions.binbook"

    path.write_bytes(build_binbook([
        encoded_page(white, PageKind.TEXT, UINT32_MAX),
        encoded_page(black, PageKind.TEXT, UINT32_MAX),
    ], profile, source_name="transition-test"))
    reader = BinBookReader.open(path, validate=True)

    forward = next(t for t in reader.page_transitions if t.from_page_number == 0 and t.to_page_number == 1)
    backward = next(t for t in reader.page_transitions if t.from_page_number == 1 and t.to_page_number == 0)
    assert forward.changed_chunk_mask == (1 << 30) - 1
    assert backward.changed_chunk_mask == (1 << 30) - 1
    assert forward.first_changed_chunk == 0
    assert forward.changed_chunk_count == 30
