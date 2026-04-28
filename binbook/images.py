from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageOps

from .pixels import gray2_to_luma, pack_gray2, unpack_gray2
from .profiles import DisplayProfile


def png_to_gray2_packed(path: Path, profile: DisplayProfile) -> bytes:
    with Image.open(path) as image:
        image = image.convert("L")
        image = ImageOps.contain(image, (profile.logical_width, profile.logical_height), method=Image.Resampling.LANCZOS)
        canvas = Image.new("L", (profile.logical_width, profile.logical_height), 255)
        x = (profile.logical_width - image.width) // 2
        y = (profile.logical_height - image.height) // 2
        canvas.paste(image, (x, y))
        pixels = [_luma_to_gray2(v) for v in canvas.tobytes()]
        return pack_gray2(pixels, profile.logical_width, profile.logical_height)


def gray2_packed_to_png(data: bytes, width: int, height: int, output: Path) -> None:
    pixels = unpack_gray2(data, width, height)
    image = Image.new("L", (width, height))
    image.putdata([gray2_to_luma(v) for v in pixels])
    image.save(output)


def _luma_to_gray2(value: int) -> int:
    if value < 43:
        return 0
    if value < 128:
        return 1
    if value < 213:
        return 2
    return 3
