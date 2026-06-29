#!/usr/bin/env python3
"""Build a deterministic four-page navigation probe fixture for Xteink X4 firmware testing.

Page 0: full-panel orientation and grayscale diagnostic target
Page 1: checkerboard pattern (160px cells)
Page 2: four-tone vertical stripes
Page 3: lorem ipsum text rendered through Pillow

All pages use X4 native 3-plane (bitmap=0x07) storage.
"""

from __future__ import annotations

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

# Resolve repo root so imports work when run from any directory.
REPO_ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(REPO_ROOT))

from binbook.constants import PixelFormat
from binbook.images import pil_image_to_packed
from binbook.page_compiler import encoded_page
from binbook.profiles import get_profile
from binbook.reader import BinBookReader
from binbook.writer import build_binbook

FIXTURE_DIR = REPO_ROOT / "firmware" / "crates" / "binbook-fw" / "fixtures"
OUTPUT_FIXTURE = FIXTURE_DIR / "nav_probe.binbook"

PROFILE_NAME = "xteink-x4-portrait"
CHECKER_CELL = 160  # logical pixels per checker cell
LOREM_FONT_SIZE = 64
LOREM_LINE_HEIGHT = 82
ORIENTATION_FONT_SIZE = 64
LOREM_TEXT = (
    "Lorem ipsum dolor sit amet, consectetur adipiscing elit. "
    "Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. "
    "Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris "
    "nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in "
    "reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla "
    "pariatur. Excepteur sint occaecat cupidatat non proident, sunt in "
    "culpa qui officia deserunt mollit anim id est laborum. "
    "Curabitur pretium tincidunt lacus. Nulla gravida orci a odio. "
    "Nullam varius, turpis et commodo pharetra, est eros bibendum elit, "
    "nec luctus magna felis sollicitudin mauris. Integer in mauris eu "
    "nibh euismod gravida. Duis ac tellus et risus vulputate vehicula. "
    "Donec lobortis risus a elit. Etiam tempor. Ut ullamcorper, ligula "
    "ut dictum pharetra, nisi nunc fringilla magna, in commodo elit erat "
    "nec turpis. Ut pharetra augue nec augue. Nam elit agna, endrerit "
    "sit amet, tincidunt ac, viverra sed, nulla. Donec porta diam eu "
    "quam. Praesent malesuada diam vitae nisi. Nullam pulvinar semper "
    "arcu. Mauris tempor. Donec et libero. Aenean rutrum, nibh ac "
    "bibendum sodales, mauris nunc semper arcu, id vehicula augue eros "
    "velit. Pellentesque habitant morbi tristique senectus et netus et "
    "malesuada fames ac turpis egestas. Fusce aliquet pede non pede. "
    "Suspendisse dapibus lorem pellentesque magna. Integer nulla. "
    "Donec blandit feugiat ligula. Donec hendrerit, felis et imperdiet "
    "euismod, purus ipsum pretium metus, in lacinia nulla nisl eget "
    "sapien. Donec ut est in lectus consequat consequat. "
    "Vestibulum suscipit nulla eu orci. Sed ipsum. Pellentesque commodo "
    "tempor eros. Praesent magna nulla, ornare eget, sagittis non, "
    "blandit id, tellus. Mauris vulputate. Donec blandit, duiidunt eget "
    "commodo convallis, nibh lectus lacinia nisl, vitae tristique dolor "
    "nibh non mauris. Fusce lacinia orci at nibh. Maecenas aliquam "
    "convallis elit. Sed vitae augue. In elit. In odio. Cras mollis "
    "metus a odio. Sed vitae ante. Fusce porttitor. Praesent vulputate "
    "arcu. Donec et velit. Sed at nibh. Aliquam erat volutpat. "
    "Phasellus hendrerit molestie sapien. Sed accumsan diam quis odio. "
    "Nulla facilisi. Nam faucibus, arcu vitae pretium vehicula, purus "
    "nisl aliquet nulla, vel sodales enim velit eu nulla. Integer ac "
    "leo. In congue. Praesent viverra. Ut ornare gravida arcu. "
    "Maecenas at massa. Maecenas sed nulla. Suspendisse potenti. "
    "Curabitur accumsan. Pellentesque suscipit. Donec consectetuer. "
    "Etiam vel tortor. Integer tempor. Vivamus ac diam. Donec quam "
    "libero, cursus in, blandit quis, posuere posuere, nulla. "
    "Suspendisse potenti. Nullam sit amet magna in magna gravida "
    "vehicula. Integer mattis blandit odio. In semper consequat nisi. "
    "Sociis natoque penatibus et magnis dis parturient montes, nascetur "
    "ridiculus mus. Proin quam nisl, fringilla a, faucibus ut, rhoncus "
    "vitae, sem. Maecenas malesuada. Praesent congue erat at massa. "
    "Sed cursus turpis vitae tortor. Donec posuere vulputate arcu. "
    "Phasellus accumsan cursus velit. Vestibulum ante ipsum primis in "
    "faucibus orci luctus et ultrices posuere cubilia Curae."
)


def _centered_text(draw, y: int, text: str, font, *, fill: int = 0) -> None:
    bbox = draw.textbbox((0, 0), text, font=font, stroke_width=2)
    width = bbox[2] - bbox[0]
    draw.text(
        ((480 - width) // 2, y),
        text,
        fill=fill,
        font=font,
        stroke_width=2,
        stroke_fill=fill,
    )


def _rotated_edge_label(image: Image.Image, text: str, x: int, y: int, font) -> None:
    bbox = font.getbbox(text, stroke_width=2)
    mask = Image.new("L", (bbox[2] - bbox[0] + 8, bbox[3] - bbox[1] + 8), 0)
    mask_draw = ImageDraw.Draw(mask)
    mask_draw.text(
        (4 - bbox[0], 4 - bbox[1]),
        text,
        fill=255,
        font=font,
        stroke_width=2,
        stroke_fill=255,
    )
    mask = mask.rotate(90, expand=True)
    image.paste(0, (x, y), mask)


def _make_orientation_target(profile) -> Image.Image:
    """Create a full-panel target with unambiguous orientation and coverage."""
    width, height = profile.logical_width, profile.logical_height
    assert (width, height) == (480, 800)
    image = Image.new("L", (width, height), 255)
    draw = ImageDraw.Draw(image)
    font_path = REPO_ROOT / "binbook" / "assets" / "fonts" / "Literata" / "Literata.ttf"
    label_font = ImageFont.truetype(str(font_path), ORIENTATION_FONT_SIZE)
    info_font = ImageFont.truetype(str(font_path), 32)
    swatch_font = ImageFont.truetype(str(font_path), 24)

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

    draw.text((65, 18), "TL", fill=0, font=label_font, stroke_width=2, stroke_fill=0)
    draw.text((330, 18), "TR", fill=0, font=label_font, stroke_width=2, stroke_fill=0)
    draw.text((65, 704), "BL", fill=0, font=label_font, stroke_width=2, stroke_fill=0)
    draw.text((326, 704), "BR", fill=0, font=label_font, stroke_width=2, stroke_fill=0)
    _centered_text(draw, 88, "TOP", label_font)
    _centered_text(draw, 622, "BOTTOM", label_font)
    _rotated_edge_label(image, "LEFT", 20, 275, label_font)
    _rotated_edge_label(image, "RIGHT", 390, 260, label_font)

    _centered_text(draw, 198, "PAGE 0", info_font)
    _centered_text(draw, 242, "PORTRAIT 480x800", info_font)

    swatches = (
        (110, 500, 170, 560, 0, "B"),
        (175, 500, 235, 560, 85, "D"),
        (240, 500, 300, 560, 170, "L"),
        (305, 500, 365, 560, 255, "W"),
    )
    for x0, y0, x1, y1, level, label in swatches:
        draw.rectangle((x0, y0, x1, y1), fill=level, outline=0, width=2)
        bbox = draw.textbbox((0, 0), label, font=swatch_font)
        draw.text(((x0 + x1 - (bbox[2] - bbox[0])) // 2, 565), label, fill=0, font=swatch_font)

    draw.rectangle((0, 0, width - 1, height - 1), outline=0, width=10)
    return image


def _make_checkerboard(profile) -> Image.Image:
    """Create a logical-size checkerboard image with 160px cells."""
    width, height = profile.logical_width, profile.logical_height
    img = Image.new("L", (width, height), 255)
    pixels = img.load()
    for y in range(height):
        for x in range(width):
            cx = (x // CHECKER_CELL) % 2
            cy = (y // CHECKER_CELL) % 2
            level = (0, 255)[(cx + cy) % 2]
            pixels[x, y] = level
    return img


def _make_stripes(profile) -> Image.Image:
    """Create a logical-size image with four vertical stripes (one per gray level)."""
    width, height = profile.logical_width, profile.logical_height
    img = Image.new("L", (width, height), 255)
    pixels = img.load()
    stripe_width = width // 4
    levels = [0, 85, 170, 255]  # black, dark gray, light gray, white
    for y in range(height):
        for x in range(width):
            stripe_idx = min(x // stripe_width, 3)
            pixels[x, y] = levels[stripe_idx]
    return img


def _make_lorem_ipsum(profile) -> Image.Image:
    """Render lorem ipsum text on a white logical-size canvas."""
    width, height = profile.logical_width, profile.logical_height
    img = Image.new("L", (width, height), 255)
    draw = ImageDraw.Draw(img)

    font_path = REPO_ROOT / "binbook" / "assets" / "fonts" / "Literata" / "Literata.ttf"
    font = ImageFont.truetype(str(font_path), LOREM_FONT_SIZE)

    margin = 20
    y = margin
    line_height = LOREM_LINE_HEIGHT
    words = LOREM_TEXT.split()
    line = ""
    for word in words:
        test = f"{line} {word}".strip()
        bbox = draw.textbbox((0, 0), test, font=font)
        tw = bbox[2] - bbox[0]
        if tw > width - 2 * margin:
            draw.text((margin, y), line, fill=0, font=font)
            y += line_height
            if y > height - margin:
                break
            line = word
        else:
            line = test
    if y <= height - margin and line:
        draw.text((margin, y), line, fill=0, font=font)
    return img


def main() -> None:
    profile = get_profile(PROFILE_NAME)

    # Page 0: full-panel orientation and grayscale target.
    orientation_img = _make_orientation_target(profile)
    orientation_packed = pil_image_to_packed(orientation_img, profile, dither=False)
    page0 = encoded_page(orientation_packed, 0, 0)

    # Page 1: checkerboard
    checker_img = _make_checkerboard(profile)
    checker_packed = pil_image_to_packed(checker_img, profile, dither=False)
    page1 = encoded_page(checker_packed, 0, 0)

    # Page 2: stripes
    stripes_img = _make_stripes(profile)
    stripes_packed = pil_image_to_packed(stripes_img, profile, dither=False)
    page2 = encoded_page(stripes_packed, 0, 0)

    # Page 3: lorem ipsum
    lorem_img = _make_lorem_ipsum(profile)
    lorem_packed = pil_image_to_packed(lorem_img, profile, dither=False)
    page3 = encoded_page(lorem_packed, 0, 0)

    pages = [page0, page1, page2, page3]
    book_bytes = build_binbook(pages, profile, source_name="nav-probe")
    OUTPUT_FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT_FIXTURE.write_bytes(book_bytes)

    # --- Self-checks ---
    reader = BinBookReader.open(OUTPUT_FIXTURE, validate=True)
    assert len(reader.pages) == 4, f"expected 4 pages, got {len(reader.pages)}"
    for i, page in enumerate(reader.pages):
        assert page.pixel_format == PixelFormat.GRAY2_PACKED, (
            f"page {i} wrong pixel format: {page.pixel_format}"
        )
        assert (page.stored_width, page.stored_height) == (800, 480), (
            f"page {i} wrong stored dimensions: {page.stored_width}x{page.stored_height}"
        )
        assert page.plane_dir.bitmap == 0x07, (
            f"page {i} wrong plane bitmap: {page.plane_dir.bitmap:#x}"
        )
    assert len(reader.page_chunks) == len(reader.pages) * 3 * 30, (
        f"expected {len(reader.pages) * 3 * 30} chunk records, got {len(reader.page_chunks)}"
    )
    expected_transitions = max(0, len(reader.pages) - 1) * 2
    assert len(reader.page_transitions) == expected_transitions, (
        f"expected {expected_transitions} transition records, got {len(reader.page_transitions)}"
    )

    sizes = [[page.plane_dir.sizes[s] for s in range(4)] for page in reader.pages]
    print(
        f"nav_probe.binbook: {len(reader.pages)} pages, {len(reader.page_chunks)} chunks, {len(reader.page_transitions)} transitions"
    )
    for i, s in enumerate(sizes):
        print(f"  page {i} plane sizes: {s}")


if __name__ == "__main__":
    main()
