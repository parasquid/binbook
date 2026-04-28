from __future__ import annotations

from math import ceil


def _expect_pixel_count(pixels: list[int], width: int, height: int) -> None:
    if len(pixels) != width * height:
        raise ValueError(f"expected {width * height} pixels, got {len(pixels)}")


def pack_gray2(pixels: list[int], width: int, height: int) -> bytes:
    _expect_pixel_count(pixels, width, height)
    out = bytearray()
    index = 0
    for _y in range(height):
        for _x_byte in range(ceil(width / 4)):
            byte = 0
            for slot in range(4):
                x = _x_byte * 4 + slot
                value = pixels[index] if x < width else 0
                if value < 0 or value > 3:
                    raise ValueError("GRAY2 pixel values must be in range 0..3")
                byte |= value << (6 - slot * 2)
                if x < width:
                    index += 1
            out.append(byte)
    return bytes(out)


def unpack_gray2(data: bytes, width: int, height: int) -> list[int]:
    row_bytes = ceil(width / 4)
    expected = row_bytes * height
    if len(data) != expected:
        raise ValueError(f"expected {expected} bytes, got {len(data)}")
    pixels: list[int] = []
    for y in range(height):
        row = data[y * row_bytes : (y + 1) * row_bytes]
        for x in range(width):
            byte = row[x // 4]
            shift = 6 - (x % 4) * 2
            pixels.append((byte >> shift) & 0b11)
    return pixels


def pack_gray1(pixels: list[int], width: int, height: int) -> bytes:
    _expect_pixel_count(pixels, width, height)
    out = bytearray()
    index = 0
    for _y in range(height):
        for x_byte in range(ceil(width / 8)):
            byte = 0
            for slot in range(8):
                x = x_byte * 8 + slot
                value = pixels[index] if x < width else 0
                if value not in (0, 1):
                    raise ValueError("GRAY1 pixel values must be 0 or 1")
                byte |= value << (7 - slot)
                if x < width:
                    index += 1
            out.append(byte)
    return bytes(out)


def unpack_gray1(data: bytes, width: int, height: int) -> list[int]:
    row_bytes = ceil(width / 8)
    if len(data) != row_bytes * height:
        raise ValueError("invalid GRAY1 byte length")
    pixels: list[int] = []
    for y in range(height):
        row = data[y * row_bytes : (y + 1) * row_bytes]
        for x in range(width):
            pixels.append((row[x // 8] >> (7 - (x % 8))) & 1)
    return pixels


def pack_gray4(pixels: list[int], width: int, height: int) -> bytes:
    _expect_pixel_count(pixels, width, height)
    out = bytearray()
    index = 0
    for _y in range(height):
        for x_byte in range(ceil(width / 2)):
            byte = 0
            for slot in range(2):
                x = x_byte * 2 + slot
                value = pixels[index] if x < width else 0
                if value < 0 or value > 15:
                    raise ValueError("GRAY4 pixel values must be in range 0..15")
                byte |= value << (4 - slot * 4)
                if x < width:
                    index += 1
            out.append(byte)
    return bytes(out)


def unpack_gray4(data: bytes, width: int, height: int) -> list[int]:
    row_bytes = ceil(width / 2)
    if len(data) != row_bytes * height:
        raise ValueError("invalid GRAY4 byte length")
    pixels: list[int] = []
    for y in range(height):
        row = data[y * row_bytes : (y + 1) * row_bytes]
        for x in range(width):
            shift = 4 if x % 2 == 0 else 0
            pixels.append((row[x // 2] >> shift) & 0xF)
    return pixels


def xteink_xth_value(gray2_value: int) -> int:
    return [3, 1, 2, 0][gray2_value]


def gray2_to_luma(value: int) -> int:
    return [0, 85, 170, 255][value]
