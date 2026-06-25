#!/usr/bin/env python3
"""Build a deterministic four-page navigation probe fixture for Xteink X4 firmware testing.

Page 0: gray-band page preserved byte-for-byte from gray2_probe.binbook
Page 1: checkerboard pattern (160px cells)
Page 2: four-tone vertical stripes
Page 3: lorem ipsum text rendered through Pillow
"""
from __future__ import annotations

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

# Resolve repo root so imports work when run from any directory.
REPO_ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(REPO_ROOT))

from binbook.constants import CompressionMethod, PixelFormat
from binbook.images import pil_image_to_packed
from binbook.profiles import get_profile
from binbook.rle import encode_packbits
from binbook.reader import BinBookReader
from binbook.writer import EncodedPage, build_binbook
from binbook.checksums import crc32

FIXTURE_DIR = REPO_ROOT / "firmware" / "crates" / "binbook-fw" / "fixtures"
SOURCE_FIXTURE = FIXTURE_DIR / "gray2_probe.binbook"
OUTPUT_FIXTURE = FIXTURE_DIR / "nav_probe.binbook"

PROFILE_NAME = "xteink-x4-portrait"
CHECKER_CELL = 160  # logical pixels per checker cell
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


def _page_data_slice(reader: BinBookReader, page_index: int) -> bytes:
    """Return the raw compressed page-data bytes for a single plane (slot 0)."""
    page = reader.pages[page_index]
    pd = page.plane_dir
    offset = reader.header.page_data_offset + pd.offsets[0]
    size = pd.sizes[0]
    return reader.data[offset : offset + size]


def _make_checkerboard(profile) -> Image.Image:
    """Create a logical-size checkerboard image with 160px cells."""
    width, height = profile.logical_width, profile.logical_height
    img = Image.new("L", (width, height), 255)
    pixels = img.load()
    levels = [0, 85, 170, 255]  # black, dark gray, light gray, white
    for y in range(height):
        for x in range(width):
            cx = (x // CHECKER_CELL) % 2
            cy = (y // CHECKER_CELL) % 2
            level = levels[(cx + cy) % 2 * 2]  # alternating black/white
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
    font = ImageFont.truetype(str(font_path), 14)

    margin = 20
    y = margin
    line_height = 18
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
    if not SOURCE_FIXTURE.exists():
        print(f"Source fixture not found: {SOURCE_FIXTURE}", file=sys.stderr)
        sys.exit(1)

    original = BinBookReader.open(SOURCE_FIXTURE, validate=True)
    profile = get_profile(PROFILE_NAME)

    # Page 0: preserve original gray-band payload byte-for-byte.
    original_compressed = _page_data_slice(original, 0)
    page0_uncompressed = profile.logical_width * profile.logical_height // 4
    page0 = EncodedPage(
        compressed=original_compressed,
        uncompressed_size=page0_uncompressed,
        page_crc32=original.pages[0].page_crc32,
        page_kind=original.pages[0].page_kind,
    )

    # Page 1: checkerboard
    checker_img = _make_checkerboard(profile)
    checker_packed = pil_image_to_packed(checker_img, profile, dither=False)
    checker_compressed = encode_packbits(checker_packed)
    page1 = EncodedPage(
        compressed=checker_compressed,
        uncompressed_size=len(checker_packed),
        page_crc32=crc32(checker_compressed),
    )

    # Page 2: stripes
    stripes_img = _make_stripes(profile)
    stripes_packed = pil_image_to_packed(stripes_img, profile, dither=False)
    stripes_compressed = encode_packbits(stripes_packed)
    page2 = EncodedPage(
        compressed=stripes_compressed,
        uncompressed_size=len(stripes_packed),
        page_crc32=crc32(stripes_compressed),
    )

    # Page 3: lorem ipsum
    lorem_img = _make_lorem_ipsum(profile)
    lorem_packed = pil_image_to_packed(lorem_img, profile, dither=False)
    lorem_compressed = encode_packbits(lorem_packed)
    page3 = EncodedPage(
        compressed=lorem_compressed,
        uncompressed_size=len(lorem_packed),
        page_crc32=crc32(lorem_compressed),
    )

    pages = [page0, page1, page2, page3]
    book_bytes = build_binbook(pages, profile, source_name="nav-probe")
    OUTPUT_FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT_FIXTURE.write_bytes(book_bytes)

    # --- Self-checks ---
    reader = BinBookReader.open(OUTPUT_FIXTURE, validate=True)
    assert len(reader.pages) == 4, f"expected 4 pages, got {len(reader.pages)}"
    assert reader.pages[0].plane_dir.sizes[0] == original.pages[0].plane_dir.sizes[0], \
        f"page 0 size mismatch: {reader.pages[0].plane_dir.sizes[0]} != {original.pages[0].plane_dir.sizes[0]}"
    assert _page_data_slice(reader, 0) == original_compressed, "page 0 payload byte-for-byte mismatch"
    for page in reader.pages:
        assert page.pixel_format == PixelFormat.GRAY2_PACKED, f"wrong pixel format: {page.pixel_format}"
        assert (page.stored_width, page.stored_height) == (800, 480), \
            f"wrong stored dimensions: {page.stored_width}x{page.stored_height}"
        assert page.plane_dir.bitmap == 0x01, f"wrong plane bitmap: {page.plane_dir.bitmap}"

    sizes = [p.plane_dir.sizes[0] for p in reader.pages]
    print(f"nav_probe.binbook: {len(reader.pages)} pages, compressed sizes: {sizes}")


if __name__ == "__main__":
    main()
