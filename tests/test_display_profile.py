from __future__ import annotations

import struct

from binbook.constants import PixelFormat, PixelFormatFlag, SectionId
from binbook.profiles import XTEINK_X4_PORTRAIT, get_profile
from binbook.reader import BinBookReader
from binbook.rle import encode_packbits
from binbook.writer import EncodedPage, build_binbook


def test_x4_profile_metadata_is_gray2_portrait_with_explicit_rotation(tmp_path):
    packed_white_page = bytes([0xFF]) * 96_000
    compressed = encode_packbits(packed_white_page)
    page = EncodedPage(compressed=compressed, uncompressed_size=len(packed_white_page), page_crc32=0)
    path = tmp_path / "x4.binbook"
    path.write_bytes(build_binbook([page], XTEINK_X4_PORTRAIT, source_name="x4"))

    reader = BinBookReader.open(path)
    display = reader._section_data(SectionId.DISPLAY_PROFILE)
    requirements = reader._section_data(SectionId.READER_REQUIREMENTS)
    image_policy = reader._section_data(SectionId.IMAGE_POLICY)

    assert struct.unpack_from("<HHHH", display, 24) == (480, 800, 800, 480)
    assert struct.unpack_from("<BhB", display, 32) == (1, 90, 1)
    supported_formats = int(PixelFormatFlag.GRAY1_PACKED | PixelFormatFlag.GRAY2_PACKED)
    assert struct.unpack_from("<I", display, 36)[0] == supported_formats
    assert struct.unpack_from("<I", display, 40)[0] == supported_formats
    assert struct.unpack_from("<HHB", display, 48) == (4, 4, 2)

    assert struct.unpack_from("<I", requirements, 16)[0] == PixelFormatFlag.GRAY2_PACKED
    assert struct.unpack_from("<H", requirements, 20)[0] == 4
    assert struct.unpack_from("<II", requirements, 32) == (96_000, 192_000)
    assert struct.unpack_from("<H", image_policy, 2)[0] == PixelFormat.GRAY2_PACKED


def test_profile_resolve_uses_default_storage_pixel_format():
    resolved = get_profile("xteink-x4-portrait").resolve()

    assert resolved.storage_pixel_format == PixelFormat.GRAY2_PACKED
    assert resolved.grayscale_levels == 4
    assert resolved.framebuffer_bits_per_pixel == 2


def test_profile_resolve_allows_supported_storage_pixel_format_override():
    resolved = get_profile("xteink-x4-portrait").resolve("gray1")

    assert resolved.storage_pixel_format == PixelFormat.GRAY1_PACKED
    assert resolved.storage_pixel_format_flag == PixelFormatFlag.GRAY1_PACKED
    assert resolved.grayscale_levels == 2
    assert resolved.framebuffer_bits_per_pixel == 1
