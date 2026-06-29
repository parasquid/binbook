import pytest

from binbook.pixels import (
    X4_CHUNK_BYTES,
    X4_CHUNKS_PER_PLANE,
    X4_ROW_BYTES,
    gray2_packed_to_x4_native_planes,
    pack_gray2,
    split_x4_plane_chunks,
)
from binbook.images import pil_image_to_packed
from binbook.profiles import get_profile
from PIL import Image


def _storage_pixel_page(gray: int, x: int = 0, y: int = 0) -> bytes:
    pixels = [3] * (800 * 480)
    pixels[y * 800 + x] = gray
    return pack_gray2(pixels, 800, 480)


@pytest.mark.parametrize(
    ("gray", "expected_msb", "expected_lsb", "expected_base"),
    [
        (0, 0x00, 0x00, 0x7F),
        (1, 0x80, 0x80, 0x7F),
        (2, 0x80, 0x00, 0x7F),
        (3, 0x00, 0x00, 0xFF),
    ],
)
def test_x4_native_planes_encode_staged_overlay_levels(
    gray: int,
    expected_msb: int,
    expected_lsb: int,
    expected_base: int,
):
    overlay_msb, overlay_lsb, fast_base = gray2_packed_to_x4_native_planes(
        _storage_pixel_page(gray, 799, 0), 800, 480
    )

    assert len(overlay_msb) == 48_000
    assert len(overlay_lsb) == 48_000
    assert len(fast_base) == 48_000
    assert overlay_msb[0] == expected_msb
    assert overlay_lsb[0] == expected_lsb
    assert fast_base[0] == expected_base


def test_split_x4_plane_chunks_returns_30_1600_byte_chunks():
    plane = bytes(range(256)) * 188
    chunks = split_x4_plane_chunks(plane[:48_000])

    assert len(chunks) == X4_CHUNKS_PER_PLANE
    assert {len(chunk) for chunk in chunks} == {X4_CHUNK_BYTES}
    assert chunks[0] == plane[:X4_CHUNK_BYTES]
    assert chunks[1] == plane[X4_CHUNK_BYTES : 2 * X4_CHUNK_BYTES]


def test_native_planes_full_pipeline():
    profile = get_profile("xteink-x4-portrait")
    img = Image.new("L", (profile.logical_width, profile.logical_height), 255)
    draw_pixels = img.load()
    for x in range(120):
        draw_pixels[x, 0] = 0
    packed = pil_image_to_packed(img, profile, dither=False)
    assert len(packed) == 800 * 480 // 4
    msb, lsb, bw = gray2_packed_to_x4_native_planes(packed, 800, 480)
    assert len(msb) == 48_000
    assert len(lsb) == 48_000
    assert len(bw) == 48_000

    row0_msb = msb[:X4_ROW_BYTES]
    row0_has_overlay = any(b != 0x00 for b in row0_msb)
    assert not row0_has_overlay, "black must not select an overlay waveform"

    row120_msb = msb[120 * X4_ROW_BYTES : 121 * X4_ROW_BYTES]
    row120_all_white = all(b == 0x00 for b in row120_msb)
    assert row120_all_white, (
        f"row 120 of MSB should be untouched white after the strip ends"
    )


def test_native_planes_top_row_black_bottom_rows_white():
    pixels = [3] * (800 * 480)
    for x in range(800):
        pixels[x] = 0
    packed = pack_gray2(pixels, 800, 480)
    msb, lsb, bw = gray2_packed_to_x4_native_planes(packed, 800, 480)

    row0_msb = msb[:X4_ROW_BYTES]
    row0_lsb = lsb[:X4_ROW_BYTES]
    row0_bw = bw[:X4_ROW_BYTES]
    assert all(b == 0x00 for b in row0_msb), "MSB row 0 should not overlay black"
    assert all(b == 0x00 for b in row0_lsb), "LSB row 0 should not overlay black"
    assert all(b == 0x00 for b in row0_bw), f"BW row 0 should be all-cleared"

    row1_msb = msb[X4_ROW_BYTES : 2 * X4_ROW_BYTES]
    row1_lsb = lsb[X4_ROW_BYTES : 2 * X4_ROW_BYTES]
    row1_bw = bw[X4_ROW_BYTES : 2 * X4_ROW_BYTES]
    assert all(b == 0x00 for b in row1_msb), "MSB row 1 should select white"
    assert all(b == 0x00 for b in row1_lsb), "LSB row 1 should select white"
    assert all(b == 0xFF for b in row1_bw), f"BW row 1 should be all-white"
