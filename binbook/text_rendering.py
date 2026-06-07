from __future__ import annotations

from PIL import Image, ImageDraw, ImageFont

from .fonts import FontInfo, PairKerningTable, get_font
from .images import pil_image_to_packed
from .profiles import DisplayProfile

DEFAULT_FONT = get_font("literata")
DEFAULT_FONT_PATH = DEFAULT_FONT.path
TEXT_FEATURES = ["kern", "-liga"]


def render_text_to_packed(text: str, profile: DisplayProfile, font_info: FontInfo = DEFAULT_FONT) -> bytes:
    supersample_factor = 2
    supersampled_width = profile.logical_width * supersample_factor
    supersampled_height = profile.logical_height * supersample_factor

    image = Image.new("L", (supersampled_width, supersampled_height), 255)
    draw = ImageDraw.Draw(image)
    font = load_font(24 * supersample_factor, font_info)
    character_spacing_milli_em = font_info.default_character_spacing_milli_em
    pair_kerning_milli_em = font_info.pair_kerning_milli_em
    x = 24 * supersample_factor
    y = 24 * supersample_factor
    right = supersampled_width - (24 * supersample_factor)
    line_height = 32 * supersample_factor

    for paragraph in text.splitlines() or [text]:
        for line in wrap_text_to_width(paragraph, draw, font, right - x, character_spacing_milli_em, pair_kerning_milli_em) or [""]:
            if y + line_height > supersampled_height - (24 * supersample_factor):
                break
            draw_text(draw, (x, y), line, font, character_spacing_milli_em, fill=0, pair_kerning_milli_em=pair_kerning_milli_em)
            y += line_height
        y += 8 * supersample_factor

    downsampled_image = image.resize(
        (profile.logical_width, profile.logical_height),
        resample=Image.Resampling.LANCZOS,
    )
    return pil_image_to_packed(downsampled_image, profile, dither=False)


def wrap_text_to_width(
    text: str,
    draw: ImageDraw.ImageDraw,
    font: ImageFont.ImageFont,
    max_width: int,
    character_spacing_milli_em: int = 0,
    pair_kerning_milli_em: PairKerningTable | None = None,
) -> list[str]:
    words = text.split()
    lines: list[str] = []
    current = ""
    for word in words:
        candidates = _split_word_to_width(word, draw, font, max_width, character_spacing_milli_em, pair_kerning_milli_em)
        for candidate in candidates:
            proposed = candidate if not current else f"{current} {candidate}"
            if measure_text(draw, proposed, font, character_spacing_milli_em, pair_kerning_milli_em) <= max_width:
                current = proposed
            else:
                if current:
                    lines.append(current)
                current = candidate
    if current:
        lines.append(current)
    return lines


def measure_text(
    draw: ImageDraw.ImageDraw,
    text: str,
    font: ImageFont.ImageFont,
    character_spacing_milli_em: int = 0,
    pair_kerning_milli_em: PairKerningTable | None = None,
) -> int:
    if not text:
        return 0
    if character_spacing_milli_em == 0:
        return int(round(draw.textlength(text, font=font, features=TEXT_FEATURES)))

    spacing_px = character_spacing_px(font, character_spacing_milli_em)
    width = 0.0
    for index, character in enumerate(text):
        width += draw.textlength(character, font=font, features=TEXT_FEATURES)
        if index != len(text) - 1:
            width += spacing_px + pair_kerning_px(font, text[index], text[index + 1], pair_kerning_milli_em)
    return max(0, int(round(width)))


def character_spacing_px(font: ImageFont.ImageFont, character_spacing_milli_em: int) -> float:
    size = getattr(font, "size", 24)
    return size * (character_spacing_milli_em / 1000)


def pair_kerning_px(
    font: ImageFont.ImageFont,
    left: str,
    right: str,
    pair_kerning_milli_em: PairKerningTable | None,
) -> float:
    if not pair_kerning_milli_em:
        return 0.0
    size = getattr(font, "size", 24)
    return size * (pair_kerning_milli_em.get((left, right), 0) / 1000)


def draw_text(
    draw: ImageDraw.ImageDraw,
    xy: tuple[int, int],
    text: str,
    font: ImageFont.ImageFont,
    character_spacing_milli_em: int,
    *,
    fill: int,
    pair_kerning_milli_em: PairKerningTable | None = None,
) -> None:
    if character_spacing_milli_em == 0:
        draw.text(xy, text, fill=fill, font=font, features=TEXT_FEATURES)
        return
    x, y = xy
    spacing_px = character_spacing_px(font, character_spacing_milli_em)
    for index, character in enumerate(text):
        draw.text((x, y), character, fill=fill, font=font, features=TEXT_FEATURES)
        x += draw.textlength(character, font=font, features=TEXT_FEATURES)
        if index != len(text) - 1:
            x += spacing_px + pair_kerning_px(font, character, text[index + 1], pair_kerning_milli_em)


def load_font(size: int, font_info: FontInfo = DEFAULT_FONT) -> ImageFont.ImageFont:
    try:
        return ImageFont.truetype(font_info.path, size)
    except OSError as exc:
        raise OSError(
            f"Failed to load font '{font_info.display_name}' from {font_info.path}. "
            f"Original error: {exc}. "
            f"Make sure the font file exists and is a valid font file."
        ) from exc


def _split_word_to_width(
    word: str,
    draw: ImageDraw.ImageDraw,
    font: ImageFont.ImageFont,
    max_width: int,
    character_spacing_milli_em: int = 0,
    pair_kerning_milli_em: PairKerningTable | None = None,
) -> list[str]:
    if measure_text(draw, word, font, character_spacing_milli_em, pair_kerning_milli_em) <= max_width:
        return [word]
    parts: list[str] = []
    current = ""
    for char in word:
        proposed = current + char
        if current and measure_text(draw, proposed, font, character_spacing_milli_em, pair_kerning_milli_em) > max_width:
            parts.append(current)
            current = char
        else:
            current = proposed
    if current:
        parts.append(current)
    return parts
