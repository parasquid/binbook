from __future__ import annotations

from pathlib import Path
from io import BytesIO

from PIL import Image, ImageOps

from .constants import PixelFormat
from .pixels import gray1_to_luma, gray2_to_luma, pack_gray1, pack_gray2, unpack_gray1, unpack_gray2
from .profiles import DisplayProfile


def png_to_packed(path: Path, profile: DisplayProfile) -> bytes:
    with Image.open(path) as image:
        return pil_image_to_packed(image, profile)


def image_bytes_to_packed(data: bytes, profile: DisplayProfile) -> bytes:
    with Image.open(BytesIO(data)) as image:
        return pil_image_to_packed(image, profile)


def pil_image_to_packed(image: Image.Image, profile: DisplayProfile) -> bytes:
    image = image.convert("L")
    image = ImageOps.contain(image, (profile.logical_width, profile.logical_height), method=Image.Resampling.LANCZOS)
    canvas = Image.new("L", (profile.logical_width, profile.logical_height), 255)
    x = (profile.logical_width - image.width) // 2
    y = (profile.logical_height - image.height) // 2
    canvas.paste(image, (x, y))
    if profile.storage_pixel_format == PixelFormat.GRAY1_PACKED:
        pixels = [_luma_to_gray1(v) for v in canvas.tobytes()]
        return pack_gray1(pixels, profile.logical_width, profile.logical_height)
    if profile.storage_pixel_format == PixelFormat.GRAY2_PACKED:
        pixels = [_luma_to_gray2(v) for v in canvas.tobytes()]
        return pack_gray2(pixels, profile.logical_width, profile.logical_height)
    raise ValueError(f"unsupported profile pixel format: {profile.storage_pixel_format}")


def packed_to_png(data: bytes, pixel_format: PixelFormat, width: int, height: int, output: Path) -> None:
    image = Image.new("L", (width, height))
    if pixel_format == PixelFormat.GRAY1_PACKED:
        pixels = unpack_gray1(data, width, height)
        image.putdata([gray1_to_luma(v) for v in pixels])
    elif pixel_format == PixelFormat.GRAY2_PACKED:
        pixels = unpack_gray2(data, width, height)
        image.putdata([gray2_to_luma(v) for v in pixels])
    else:
        raise ValueError(f"unsupported page pixel format: {pixel_format}")
    image.save(output)


def _luma_to_gray1(value: int) -> int:
    return 0 if value < 128 else 1


def _luma_to_gray2(value: int) -> int:
    if value < 43:
        return 0
    if value < 128:
        return 1
    if value < 213:
        return 2
    return 3


def png_to_gray2_packed(path: Path, profile: DisplayProfile) -> bytes:
    return png_to_packed(path, profile)


def image_bytes_to_gray2_packed(data: bytes, profile: DisplayProfile) -> bytes:
    return image_bytes_to_packed(data, profile)


def pil_image_to_gray2_packed(image: Image.Image, profile: DisplayProfile) -> bytes:
    return pil_image_to_packed(image, profile)


def gray2_packed_to_png(data: bytes, width: int, height: int, output: Path) -> None:
    packed_to_png(data, PixelFormat.GRAY2_PACKED, width, height, output)
