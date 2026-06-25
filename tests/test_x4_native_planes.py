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


def test_x4_native_planes_map_black_pixel_to_all_planes():
    msb, lsb, bw = gray2_packed_to_x4_native_planes(_storage_pixel_page(0, 799, 0), 800, 480)

    assert len(msb) == 48_000
    assert len(lsb) == 48_000
    assert len(bw) == 48_000
    # Storage (799, 0) -> ram_x = 799 - 799 = 0 -> byte 0, bit 0x80
    assert msb[0] == 0x7F
    assert lsb[0] == 0x7F
    assert bw[0] == 0x7F


def test_x4_native_planes_map_gray_levels():
    dark_msb, dark_lsb, dark_bw = gray2_packed_to_x4_native_planes(
        _storage_pixel_page(1, 799, 0), 800, 480
    )
    light_msb, light_lsb, light_bw = gray2_packed_to_x4_native_planes(
        _storage_pixel_page(2, 799, 0), 800, 480
    )

    # gray=1 (dark gray): MSB cleared, LSB set -> dark_msb[0]=0x7F, dark_lsb[0]=0xFF
    assert dark_msb[0] == 0x7F
    assert dark_lsb[0] == 0xFF
    assert dark_bw[0] == 0x7F
    # gray=2 (light gray): MSB set, LSB cleared -> light_msb[0]=0xFF, light_lsb[0]=0x7F
    assert light_msb[0] == 0xFF
    assert light_lsb[0] == 0x7F
    assert light_bw[0] == 0xFF


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
    row0_has_cleared = any(b != 0xFF for b in row0_msb)
    assert row0_has_cleared, "row 0 of MSB should have cleared bits from the black strip"

    row120_msb = msb[120 * X4_ROW_BYTES : 121 * X4_ROW_BYTES]
    row120_all_white = all(b == 0xFF for b in row120_msb)
    assert row120_all_white, f"row 120 of MSB should be untouched white after the strip ends"


def test_native_planes_top_row_black_bottom_rows_white():
    pixels = [3] * (800 * 480)
    for x in range(800):
        pixels[x] = 0
    packed = pack_gray2(pixels, 800, 480)
    msb, lsb, bw = gray2_packed_to_x4_native_planes(packed, 800, 480)

    row0_msb = msb[:X4_ROW_BYTES]
    row0_lsb = lsb[:X4_ROW_BYTES]
    row0_bw = bw[:X4_ROW_BYTES]
    assert all(b == 0x00 for b in row0_msb), f"MSB row 0 should be all-cleared"
    assert all(b == 0x00 for b in row0_lsb), f"LSB row 0 should be all-cleared"
    assert all(b == 0x00 for b in row0_bw), f"BW row 0 should be all-cleared"

    row1_msb = msb[X4_ROW_BYTES : 2 * X4_ROW_BYTES]
    row1_lsb = lsb[X4_ROW_BYTES : 2 * X4_ROW_BYTES]
    row1_bw = bw[X4_ROW_BYTES : 2 * X4_ROW_BYTES]
    assert all(b == 0xFF for b in row1_msb), f"MSB row 1 should be all-white"
    assert all(b == 0xFF for b in row1_lsb), f"LSB row 1 should be all-white"
    assert all(b == 0xFF for b in row1_bw), f"BW row 1 should be all-white"
