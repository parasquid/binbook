from PIL import Image

from binbook.images import (
    _luma_to_gray1,
    _luma_to_gray2,
    _luma_to_gray2_pixels,
    storage_image_to_logical,
)
from binbook.profiles import XTEINK_X4_PORTRAIT


def test_luma_to_gray1_thresholds_to_black_or_white():
    assert _luma_to_gray1(0) == 0
    assert _luma_to_gray1(127) == 0
    assert _luma_to_gray1(128) == 1
    assert _luma_to_gray1(255) == 1


def test_luma_to_gray2_uses_nearest_canonical_level_thresholds():
    assert _luma_to_gray2(0) == 0
    assert _luma_to_gray2(42) == 0
    assert _luma_to_gray2(43) == 1
    assert _luma_to_gray2(127) == 1
    assert _luma_to_gray2(128) == 2
    assert _luma_to_gray2(212) == 2
    assert _luma_to_gray2(213) == 3
    assert _luma_to_gray2(255) == 3


def test_gray2_floyd_steinberg_diffuses_error_across_flat_luma():
    pixels = _luma_to_gray2_pixels(bytes([127] * 8), 8, 1, dither=True)

    assert set(pixels) == {1, 2}


def test_gray2_dither_false_preserves_direct_threshold_quantization():
    pixels = _luma_to_gray2_pixels(bytes([127] * 8), 8, 1, dither=False)

    assert pixels == [1] * 8


def test_storage_image_to_logical_uses_verified_x4_270_rotation():
    image = Image.new("L", (800, 480), 255)
    image.putpixel((779, 10), 0)

    logical = storage_image_to_logical(
        image,
        logical_width=XTEINK_X4_PORTRAIT.logical_width,
        logical_height=XTEINK_X4_PORTRAIT.logical_height,
        logical_to_physical_rotation=XTEINK_X4_PORTRAIT.logical_to_physical_rotation,
    )

    assert logical.size == (480, 800)
    assert logical.getpixel((10, 20)) == 0
