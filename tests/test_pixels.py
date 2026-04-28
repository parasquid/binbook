import pytest

from binbook.pixels import pack_gray2, unpack_gray2, xteink_xth_value


def test_gray2_packs_leftmost_pixel_in_highest_bits():
    packed = pack_gray2([0, 1, 2, 3], width=4, height=1)
    assert packed == bytes([0b00011011])
    assert unpack_gray2(packed, width=4, height=1) == [0, 1, 2, 3]


def test_gray2_zero_fills_unused_row_bits():
    packed = pack_gray2([3, 2, 1], width=3, height=1)
    assert packed == bytes([0b11100100])
    assert unpack_gray2(packed, width=3, height=1) == [3, 2, 1]


def test_gray2_rejects_out_of_range_pixel_values():
    with pytest.raises(ValueError):
        pack_gray2([4], width=1, height=1)


def test_xteink_xth_mapping_is_display_backend_only_mapping():
    assert [xteink_xth_value(v) for v in [0, 1, 2, 3]] == [3, 1, 2, 0]
