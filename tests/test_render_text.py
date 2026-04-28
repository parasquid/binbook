from __future__ import annotations

from pathlib import Path

from binbook.pixels import unpack_gray2
from binbook.profiles import XTEINK_X4_PORTRAIT
from binbook.render import DEFAULT_FONT_PATH, _font, _render_text_to_packed


def test_rendered_text_respects_right_margin_for_wide_glyphs():
    packed = _render_text_to_packed("W" * 160, XTEINK_X4_PORTRAIT)
    pixels = unpack_gray2(packed, XTEINK_X4_PORTRAIT.logical_width, XTEINK_X4_PORTRAIT.logical_height)

    right_margin_start = XTEINK_X4_PORTRAIT.logical_width - 24
    dark_pixels_in_right_margin = 0
    for y in range(XTEINK_X4_PORTRAIT.logical_height):
        row_offset = y * XTEINK_X4_PORTRAIT.logical_width
        for x in range(right_margin_start, XTEINK_X4_PORTRAIT.logical_width):
            if pixels[row_offset + x] < 3:
                dark_pixels_in_right_margin += 1

    assert dark_pixels_in_right_margin == 0


def test_default_font_uses_bundled_literata():
    assert DEFAULT_FONT_PATH.exists()

    loaded = _font(24)

    assert Path(loaded.path) == DEFAULT_FONT_PATH
