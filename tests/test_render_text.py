from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageChops, ImageDraw

import binbook.fonts as fonts_module
from binbook.fonts import get_font
from binbook.pixels import unpack_gray2
from binbook.profiles import XTEINK_X4_PORTRAIT
from binbook.render import DEFAULT_FONT_PATH, _draw_text, _font, _render_text_to_packed, _text_width


def test_rendered_text_respects_right_margin_for_wide_glyphs():
    packed = _render_text_to_packed("W" * 160, XTEINK_X4_PORTRAIT)
    pixels = unpack_gray2(packed, XTEINK_X4_PORTRAIT.logical_width, XTEINK_X4_PORTRAIT.logical_height)

    right_margin_start = XTEINK_X4_PORTRAIT.logical_width - 24
    dark_pixels_in_right_margin = 0
    for y in range(XTEINK_X4_PORTRAIT.logical_height):
        row_offset = y * XTEINK_X4_PORTRAIT.logical_width
        for x in range(right_margin_start, XTEINK_X4_PORTRAIT.logical_width):
            if pixels[row_offset + x] != 3:
                dark_pixels_in_right_margin += 1

    assert dark_pixels_in_right_margin == 0


def test_default_font_uses_bundled_literata():
    assert DEFAULT_FONT_PATH.exists()

    loaded = _font(24)

    assert Path(loaded.path) == DEFAULT_FONT_PATH


def test_opendyslexic_default_character_spacing_tightens_measurement():
    image = Image.new("L", (480, 800), 255)
    draw = ImageDraw.Draw(image)
    font_info = get_font("sans-serif")
    font = _font(24, font_info)
    text = "serviceable"

    normal_width = _text_width(draw, text, font)
    spaced_width = _text_width(draw, text, font, font_info.default_character_spacing_milli_em)

    assert spaced_width < normal_width


def test_opendyslexic_default_character_spacing_is_tighter_for_proofing():
    assert get_font("opendyslexic").default_character_spacing_milli_em == -160
    assert get_font("sans-serif").default_character_spacing_milli_em == -160


def test_text_width_uses_kerning_without_ligatures():
    image = Image.new("L", (200, 100), 255)
    draw = ImageDraw.Draw(image)
    font = _font(72, get_font("literata"))

    unkerned_width = draw.textlength("AV", font=font, features=["-kern", "-liga"])

    assert _text_width(draw, "AV", font) < unkerned_width


def test_draw_text_keeps_ligature_prone_characters():
    font_info = get_font("literata")
    font = _font(72, font_info)

    actual = Image.new("L", (360, 160), 255)
    actual_draw = ImageDraw.Draw(actual)
    _draw_text(actual_draw, (20, 40), "office", font, 0, fill=0)

    expected = Image.new("L", (360, 160), 255)
    expected_draw = ImageDraw.Draw(expected)
    expected_draw.text((20, 40), "office", font=font, fill=0, features=["kern", "-liga"])

    actual_bbox = ImageChops.invert(actual).getbbox()
    expected_bbox = ImageChops.invert(expected).getbbox()

    assert actual_bbox is not None
    assert expected_bbox is not None
    assert actual_bbox[2] - actual_bbox[0] >= expected_bbox[2] - expected_bbox[0] - 2


def test_opendyslexic_pair_kerning_tightens_you_without_global_spacing_change():
    image = Image.new("L", (400, 160), 255)
    draw = ImageDraw.Draw(image)
    font_info = get_font("opendyslexic")
    font = _font(72, font_info)
    spacing = font_info.default_character_spacing_milli_em

    without_pair_adjustment = _text_width(draw, "You", font, spacing)
    with_pair_adjustment = _text_width(draw, "You", font, spacing, font_info.pair_kerning_milli_em)

    assert without_pair_adjustment - with_pair_adjustment >= 8


def test_opendyslexic_pair_kerning_supports_lowercase_yo():
    image = Image.new("L", (400, 160), 255)
    draw = ImageDraw.Draw(image)
    font_info = get_font("opendyslexic")
    font = _font(72, font_info)
    spacing = font_info.default_character_spacing_milli_em

    without_pair_adjustment = _text_width(draw, "you", font, spacing)
    with_pair_adjustment = _text_width(draw, "you", font, spacing, font_info.pair_kerning_milli_em)

    assert without_pair_adjustment - with_pair_adjustment >= 4


def test_literata_has_no_font_specific_pair_kerning_override():
    assert get_font("literata").pair_kerning_milli_em == {}


def test_sans_serif_alias_has_no_font_specific_pair_kerning_override():
    assert get_font("sans-serif").pair_kerning_milli_em == {}


def test_get_font_loads_family_specific_pair_kerning_json(tmp_path, monkeypatch):
    kerning_dir = tmp_path / "font_kerning"
    kerning_dir.mkdir()
    (kerning_dir / "literata.json").write_text('{"AV": -90}\n')

    monkeypatch.setattr(fonts_module, "FONT_KERNING_DIR", kerning_dir)

    assert get_font("literata").pair_kerning_milli_em == {("A", "V"): -90}


def test_draw_text_applies_same_pair_kerning_as_measurement():
    font_info = get_font("opendyslexic")
    font = _font(72, font_info)
    spacing = font_info.default_character_spacing_milli_em

    without_pairs = Image.new("L", (400, 160), 255)
    draw_without_pairs = ImageDraw.Draw(without_pairs)
    _draw_text(draw_without_pairs, (20, 40), "You", font, spacing, fill=0)

    with_pairs = Image.new("L", (400, 160), 255)
    draw_with_pairs = ImageDraw.Draw(with_pairs)
    _draw_text(draw_with_pairs, (20, 40), "You", font, spacing, fill=0, pair_kerning_milli_em=font_info.pair_kerning_milli_em)

    without_bbox = ImageChops.invert(without_pairs).getbbox()
    with_bbox = ImageChops.invert(with_pairs).getbbox()

    assert without_bbox is not None
    assert with_bbox is not None
    assert (without_bbox[2] - without_bbox[0]) - (with_bbox[2] - with_bbox[0]) >= 7
