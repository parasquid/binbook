from binbook.images import _luma_to_gray1, _luma_to_gray2


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
