#!/usr/bin/env python3

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(REPO_ROOT))

from binbook.constants import PixelFormat, SectionId, WaveformHint
from binbook.profiles import DisplayProfile, get_profile
from binbook.reader import BinBookReader

OUTPUT_FIXTURES = (
    REPO_ROOT / "firmware" / "crates" / "binbook-fw" / "fixtures" / "nav_probe.binbook",
    REPO_ROOT / "crates" / "binbook-core" / "tests" / "fixtures" / "nav_probe.binbook",
    REPO_ROOT
    / "crates"
    / "xteink-x4-display"
    / "tests"
    / "fixtures"
    / "nav_probe.binbook",
)
FONT_PATH = REPO_ROOT / "binbook" / "assets" / "fonts" / "Literata" / "Literata.ttf"
PROFILE_NAME = "xteink-x4-portrait"
CONTENT_BOX = (12, 12, 468, 788)
PAGE_LABEL_BOX = (70, 170, 410, 360)
CHECKER_CELL = 160
GRAY_LEVELS = (0, 85, 170, 255)


def _font(size: int) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(str(FONT_PATH), size)


def _centered_text(
    draw: ImageDraw.ImageDraw,
    y: int,
    text: str,
    font: ImageFont.FreeTypeFont,
    *,
    fill: int = 0,
    stroke_width: int = 2,
    stroke_fill: int | None = None,
) -> None:
    bbox = draw.textbbox((0, 0), text, font=font, stroke_width=stroke_width)
    draw.text(
        ((480 - (bbox[2] - bbox[0])) // 2, y),
        text,
        fill=fill,
        font=font,
        stroke_width=stroke_width,
        stroke_fill=fill if stroke_fill is None else stroke_fill,
    )


def _rotated_edge_label(
    image: Image.Image, text: str, x: int, y: int, font: ImageFont.FreeTypeFont
) -> None:
    bbox = font.getbbox(text, stroke_width=2)
    mask = Image.new("L", (bbox[2] - bbox[0] + 8, bbox[3] - bbox[1] + 8), 0)
    ImageDraw.Draw(mask).text(
        (4 - bbox[0], 4 - bbox[1]),
        text,
        fill=255,
        font=font,
        stroke_width=2,
        stroke_fill=255,
    )
    image.paste(0, (x, y), mask.rotate(90, expand=True))


def _draw_page_number(
    image: Image.Image, page_number: int, font: ImageFont.FreeTypeFont
) -> None:
    draw = ImageDraw.Draw(image)
    text = f"PAGE {page_number:02d}"
    bbox = draw.textbbox((0, 0), text, font=font, stroke_width=10)
    x = (480 - (bbox[2] - bbox[0])) // 2
    y = (
        PAGE_LABEL_BOX[1]
        + (PAGE_LABEL_BOX[3] - PAGE_LABEL_BOX[1] - (bbox[3] - bbox[1])) // 2
    )
    draw.text((x, y), text, fill=0, font=font, stroke_width=10, stroke_fill=255)


def _draw_common_frame(
    image: Image.Image, page_number: int, profile: DisplayProfile
) -> None:
    width, height = profile.logical_width, profile.logical_height
    assert (width, height) == (480, 800)
    draw = ImageDraw.Draw(image)
    edge_font = _font(42)
    info_font = _font(22)
    swatch_font = _font(20)

    for x in range(50, width, 50):
        draw.line((x, 10, x, height - 11), fill=170, width=1)
    for y in range(50, height, 50):
        draw.line((10, y, width - 11, y), fill=170, width=1)
    draw.line((240, 10, 240, 789), fill=0, width=3)
    draw.line((10, 400, 469, 400), fill=0, width=3)
    for offset in range(50, 480, 50):
        tick = 24 if offset % 100 == 0 else 15
        draw.line((offset, 10, offset, 10 + tick), fill=0, width=3)
        draw.line((offset, 789 - tick, offset, 789), fill=0, width=3)
    for offset in range(50, 800, 50):
        tick = 24 if offset % 100 == 0 else 15
        draw.line((10, offset, 10 + tick, offset), fill=0, width=3)
        draw.line((469 - tick, offset, 469, offset), fill=0, width=3)

    draw.polygon(((18, 55), (55, 18), (55, 55)), fill=0)
    draw.ellipse((425, 18, 461, 54), fill=0)
    draw.rectangle((18, 745, 54, 781), fill=0)
    draw.polygon(((443, 744), (462, 763), (443, 782), (424, 763)), fill=0)
    draw.text((65, 18), "TL", fill=0, font=edge_font, stroke_width=2, stroke_fill=0)
    draw.text((350, 18), "TR", fill=0, font=edge_font, stroke_width=2, stroke_fill=0)
    draw.text((65, 720), "BL", fill=0, font=edge_font, stroke_width=2, stroke_fill=0)
    draw.text((350, 720), "BR", fill=0, font=edge_font, stroke_width=2, stroke_fill=0)
    _centered_text(draw, 82, "TOP", edge_font)
    _centered_text(draw, 650, "BOTTOM", edge_font)
    _rotated_edge_label(image, "LEFT", 18, 290, edge_font)
    _rotated_edge_label(image, "RIGHT", 404, 275, edge_font)
    _centered_text(draw, 370, "PORTRAIT 480x800", info_font)

    for x0, level, label in zip((110, 175, 240, 305), GRAY_LEVELS, "BDLW", strict=True):
        draw.rectangle((x0, 500, x0 + 60, 560), fill=level, outline=0, width=2)
        draw.text((x0 + 21, 565), label, fill=0, font=swatch_font)
    _draw_page_number(image, page_number, _font(72))
    draw.rectangle((0, 0, width - 1, height - 1), outline=0, width=10)


def _checker(draw: ImageDraw.ImageDraw, *, inverse: bool = False) -> None:
    for y in range(12, 788, CHECKER_CELL):
        for x in range(12, 468, CHECKER_CELL):
            phase = ((x // CHECKER_CELL) + (y // CHECKER_CELL)) % 2
            level = (255, 0)[phase ^ inverse]
            draw.rectangle((x, y, min(x + 159, 467), min(y + 159, 787)), fill=level)


def _pattern_page(page_number: int, profile: DisplayProfile) -> Image.Image:
    image = Image.new("L", (profile.logical_width, profile.logical_height), 255)
    draw = ImageDraw.Draw(image)
    x0, y0, x1, y1 = CONTENT_BOX
    if page_number == 1:
        _checker(draw)
    elif page_number == 2:
        for index, level in enumerate(GRAY_LEVELS):
            draw.rectangle((index * 120, y0, index * 120 + 119, y1), fill=level)
    elif page_number == 3:
        draw.multiline_text(
            (24, 390),
            "LITERATA\nNAVIGATION\nDIAGNOSTIC",
            fill=0,
            font=_font(48),
            spacing=16,
        )
    elif page_number == 4:
        for index, level in enumerate(GRAY_LEVELS):
            draw.rectangle((x0, index * 200, x1, index * 200 + 199), fill=level)
    elif page_number in (5, 6, 7):
        for offset in range(-400, 1200, 80):
            if page_number in (5, 7):
                draw.line((x0, offset, x1, offset - (x1 - x0)), fill=0, width=8)
            if page_number in (6, 7):
                draw.line((x0, offset, x1, offset + (x1 - x0)), fill=0, width=8)
    elif page_number == 8:
        for inset in range(40, 220, 35):
            draw.rectangle((inset, inset, 480 - inset, 800 - inset), outline=0, width=8)
    elif page_number == 9:
        for x in range(x0, x1, 20):
            draw.rectangle((x, y0, x + 19, y1), fill=(0, 255)[(x // 20) % 2])
    elif page_number == 10:
        for y in range(y0, y1, 20):
            draw.rectangle((x0, y, x1, y + 19), fill=(0, 255)[(y // 20) % 2])
    elif page_number == 11:
        draw.rectangle((x0, y0, 239, 399), fill=0)
        draw.rectangle((241, y0, x1, 399), fill=85)
        draw.rectangle((x0, 401, 239, y1), fill=170)
    elif page_number in (12, 13):
        step = 72 if page_number == 12 else 28
        radius = 5 if page_number == 12 else 7
        for y in range(390, y1, step):
            for x in range(24, x1, step):
                draw.ellipse((x - radius, y - radius, x + radius, y + radius), fill=0)
    elif page_number == 14:
        draw.polygon(((x0, y0), (140, y0), (x0, 140)), fill=0)
        draw.line((30, 30, 450, 770), fill=0, width=18)
        draw.line((450, 30, 30, 770), fill=0, width=18)
    elif page_number == 15:
        _checker(draw, inverse=True)
    _draw_common_frame(image, page_number, profile)
    return image


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--compiler", required=True, type=Path)
    args = parser.parse_args(argv)
    compiler = args.compiler
    if not compiler.is_absolute():
        compiler = REPO_ROOT / compiler
    profile = get_profile(PROFILE_NAME)
    images = [_pattern_page(page_number, profile) for page_number in range(16)]
    with tempfile.TemporaryDirectory(prefix="binbook-nav-probe-") as temporary:
        root = Path(temporary)
        pages = root / "pages"
        pages.mkdir()
        for page_number, image in enumerate(images):
            image.save(pages / f"{page_number:02d}.png")
        generated = root / "nav_probe.binbook"
        subprocess.run(
            [
                str(compiler),
                "encode",
                str(pages),
                "-o",
                str(generated),
                "--profile",
                PROFILE_NAME,
                "--pixel-format",
                "gray2",
                "--no-dither",
            ],
            cwd=REPO_ROOT,
            check=True,
        )
        for output in OUTPUT_FIXTURES:
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(generated, output)

    reader = BinBookReader.open(OUTPUT_FIXTURES[0], validate=True)
    assert len(reader.pages) == 16
    assert len(reader.page_chunks) == 1_440
    assert len(reader.page_transitions) == 30
    display = reader._section_data(SectionId.DISPLAY_PROFILE)
    assert display[53] == WaveformHint.SSD1677_STAGED_GRAY2
    for page in reader.pages:
        assert page.pixel_format == PixelFormat.GRAY2_PACKED
        assert (page.stored_width, page.stored_height) == (800, 480)
        assert page.plane_dir.bitmap == 0x07
    print(
        f"nav_probe.binbook: {len(reader.pages)} pages, "
        f"{len(reader.page_chunks)} chunks, {len(reader.page_transitions)} transitions"
    )


if __name__ == "__main__":
    main()
