from __future__ import annotations

from pathlib import Path
from io import BytesIO

from PIL import Image, ImageOps

from .constants import PixelFormat
from .pixels import (
    gray1_to_luma,
    gray2_to_luma,
    pack_gray1,
    pack_gray2,
    unpack_gray1,
    unpack_gray2,
)
from .profiles import DisplayProfile


def png_to_packed(path: Path, profile: DisplayProfile, *, dither: bool = True) -> bytes:
    with Image.open(path) as image:
        return pil_image_to_packed(image, profile, dither=dither)


def image_bytes_to_packed(
    data: bytes, profile: DisplayProfile, *, dither: bool = True
) -> bytes:
    with Image.open(BytesIO(data)) as image:
        return pil_image_to_packed(image, profile, dither=dither)


def pil_image_to_packed(
    image: Image.Image, profile: DisplayProfile, *, dither: bool = True
) -> bytes:
    image = image.convert("L")
    image = ImageOps.contain(
        image,
        (profile.logical_width, profile.logical_height),
        method=Image.Resampling.LANCZOS,
    )
    canvas = Image.new("L", (profile.logical_width, profile.logical_height), 255)
    x = (profile.logical_width - image.width) // 2
    y = (profile.logical_height - image.height) // 2
    canvas.paste(image, (x, y))
    luma = canvas.tobytes()
    if profile.storage_pixel_format == PixelFormat.GRAY1_PACKED:
        pixels = _luma_to_gray1_pixels(
            luma, profile.logical_width, profile.logical_height, dither=dither
        )
        pixels = _orient_pixels_to_storage(pixels, profile)
        return pack_gray1(pixels, profile.storage_width, profile.storage_height)
    if profile.storage_pixel_format == PixelFormat.GRAY2_PACKED:
        pixels = _luma_to_gray2_pixels(
            luma, profile.logical_width, profile.logical_height, dither=dither
        )
        pixels = _orient_pixels_to_storage(pixels, profile)
        return pack_gray2(pixels, profile.storage_width, profile.storage_height)
    raise ValueError(
        f"unsupported profile pixel format: {profile.storage_pixel_format}"
    )


def packed_to_image(
    data: bytes, pixel_format: PixelFormat, width: int, height: int
) -> Image.Image:
    image = Image.new("L", (width, height))
    if pixel_format == PixelFormat.GRAY1_PACKED:
        pixels = unpack_gray1(data, width, height)
        image.putdata([gray1_to_luma(v) for v in pixels])
    elif pixel_format == PixelFormat.GRAY2_PACKED:
        pixels = unpack_gray2(data, width, height)
        image.putdata([gray2_to_luma(v) for v in pixels])
    else:
        raise ValueError(f"unsupported page pixel format: {pixel_format}")
    return image


def storage_image_to_logical(
    image: Image.Image,
    *,
    logical_width: int,
    logical_height: int,
    logical_to_physical_rotation: int,
) -> Image.Image:
    rotation = logical_to_physical_rotation % 360
    if rotation == 0:
        logical = image
    elif rotation == 90:
        logical = image.rotate(270, expand=True)
    elif rotation == 180:
        logical = image.rotate(180, expand=True)
    elif rotation == 270:
        logical = image.rotate(90, expand=True)
    else:
        raise ValueError(
            f"unsupported logical-to-physical rotation: {logical_to_physical_rotation}"
        )
    if logical.size != (logical_width, logical_height):
        raise ValueError("logical image dimensions do not match display profile")
    return logical


def packed_to_png(
    data: bytes, pixel_format: PixelFormat, width: int, height: int, output: Path
) -> None:
    image = packed_to_image(data, pixel_format, width, height)
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


def _luma_to_gray1_pixels(
    luma: bytes, width: int, height: int, *, dither: bool
) -> list[int]:
    if not dither:
        return [_luma_to_gray1(v) for v in luma]
    return _floyd_steinberg(luma, width, height, [0, 255], [0, 1])


def _luma_to_gray2_pixels(
    luma: bytes, width: int, height: int, *, dither: bool
) -> list[int]:
    if not dither:
        return [_luma_to_gray2(v) for v in luma]
    return _floyd_steinberg(luma, width, height, [0, 85, 170, 255], [0, 1, 2, 3])


def _orient_pixels_to_storage(pixels: list[int], profile: DisplayProfile) -> list[int]:
    rotation = profile.logical_to_physical_rotation % 360
    if rotation == 0:
        if (
            profile.storage_width != profile.logical_width
            or profile.storage_height != profile.logical_height
        ):
            raise ValueError(
                "storage dimensions do not match logical dimensions for zero-rotation profile"
            )
        return pixels
    if rotation == 90:
        return _rotate_pixels_270_clockwise(
            pixels, profile.logical_width, profile.logical_height
        )
    if rotation == 180:
        return _rotate_pixels_180(pixels, profile.logical_width, profile.logical_height)
    if rotation == 270:
        return _rotate_pixels_90_clockwise(
            pixels, profile.logical_width, profile.logical_height
        )
    raise ValueError(
        f"unsupported logical-to-physical rotation: {profile.logical_to_physical_rotation}"
    )


def _rotate_pixels_90_clockwise(
    pixels: list[int], width: int, height: int
) -> list[int]:
    out = [0] * (width * height)
    storage_width = height
    for y in range(height):
        for x in range(width):
            out[x * storage_width + (height - 1 - y)] = pixels[y * width + x]
    return out


def _rotate_pixels_180(pixels: list[int], width: int, height: int) -> list[int]:
    out = [0] * (width * height)
    for y in range(height):
        for x in range(width):
            out[(height - 1 - y) * width + (width - 1 - x)] = pixels[y * width + x]
    return out


def _rotate_pixels_270_clockwise(
    pixels: list[int], width: int, height: int
) -> list[int]:
    out = [0] * (width * height)
    storage_width = height
    for y in range(height):
        for x in range(width):
            out[(width - 1 - x) * storage_width + y] = pixels[y * width + x]
    return out


def _floyd_steinberg(
    luma: bytes, width: int, height: int, levels: list[int], values: list[int]
) -> list[int]:
    if len(luma) != width * height:
        raise ValueError("luma byte length does not match image dimensions")

    work = [float(v) for v in luma]
    pixels = [0] * len(work)
    for y in range(height):
        for x in range(width):
            index = y * width + x
            old = _clamp_luma(work[index])
            level_index = _nearest_level_index(old, levels)
            new = levels[level_index]
            pixels[index] = values[level_index]
            error = old - new
            if x + 1 < width:
                work[index + 1] += error * 7 / 16
            if y + 1 < height:
                if x > 0:
                    work[index + width - 1] += error * 3 / 16
                work[index + width] += error * 5 / 16
                if x + 1 < width:
                    work[index + width + 1] += error * 1 / 16
    return pixels


def _nearest_level_index(value: float, levels: list[int]) -> int:
    return min(range(len(levels)), key=lambda index: abs(value - levels[index]))


def _clamp_luma(value: float) -> float:
    return min(255.0, max(0.0, value))


def png_to_gray2_packed(path: Path, profile: DisplayProfile) -> bytes:
    return png_to_packed(path, profile)


def image_bytes_to_gray2_packed(data: bytes, profile: DisplayProfile) -> bytes:
    return image_bytes_to_packed(data, profile)


def pil_image_to_gray2_packed(image: Image.Image, profile: DisplayProfile) -> bytes:
    return pil_image_to_packed(image, profile)


def gray2_packed_to_png(data: bytes, width: int, height: int, output: Path) -> None:
    packed_to_png(data, PixelFormat.GRAY2_PACKED, width, height, output)
