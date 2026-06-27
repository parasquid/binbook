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


def gray1_to_luma(value: int) -> int:
    if value not in (0, 1):
        raise ValueError("GRAY1 pixel values must be 0 or 1")
    return 255 if value else 0


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


X4_PHYSICAL_WIDTH = 800
X4_PHYSICAL_HEIGHT = 480
X4_ROW_BYTES = 100
X4_CHUNK_ROWS = 16
X4_CHUNK_BYTES = X4_ROW_BYTES * X4_CHUNK_ROWS
X4_CHUNKS_PER_PLANE = X4_PHYSICAL_HEIGHT // X4_CHUNK_ROWS


def x4_logical_to_physical(logical_x: int, logical_y: int) -> tuple[int, int]:
    return 799 - logical_y, logical_x


def _clear_native_bit(row: bytearray, physical_x: int) -> None:
    ram_x = X4_PHYSICAL_WIDTH - 1 - physical_x
    row[ram_x // 8] &= ~(0x80 >> (ram_x % 8)) & 0xFF


def gray2_packed_to_x4_native_planes(
    data: bytes, storage_width: int, storage_height: int
) -> tuple[bytes, bytes, bytes]:
    if storage_width != X4_PHYSICAL_WIDTH or storage_height != X4_PHYSICAL_HEIGHT:
        raise ValueError(
            f"xteink-x4-portrait native planes require {X4_PHYSICAL_WIDTH}x{X4_PHYSICAL_HEIGHT} "
            f"storage data, got {storage_width}x{storage_height}"
        )
    pixels = unpack_gray2(data, storage_width, storage_height)
    msb_rows = [bytearray([0xFF] * X4_ROW_BYTES) for _ in range(X4_PHYSICAL_HEIGHT)]
    lsb_rows = [bytearray([0xFF] * X4_ROW_BYTES) for _ in range(X4_PHYSICAL_HEIGHT)]
    bw_rows = [bytearray([0xFF] * X4_ROW_BYTES) for _ in range(X4_PHYSICAL_HEIGHT)]

    for storage_y in range(storage_height):
        for storage_x in range(storage_width):
            gray = pixels[storage_y * storage_width + storage_x]
            if gray in (0, 1):
                _clear_native_bit(msb_rows[storage_y], storage_x)
            if gray in (0, 2):
                _clear_native_bit(lsb_rows[storage_y], storage_x)
            if gray < 2:
                _clear_native_bit(bw_rows[storage_y], storage_x)

    return (
        b"".join(msb_rows),
        b"".join(lsb_rows),
        b"".join(bw_rows),
    )


def split_x4_plane_chunks(plane: bytes) -> list[bytes]:
    expected = X4_ROW_BYTES * X4_PHYSICAL_HEIGHT
    if len(plane) != expected:
        raise ValueError(f"expected {expected} bytes, got {len(plane)}")
    return [
        plane[i * X4_CHUNK_BYTES : (i + 1) * X4_CHUNK_BYTES]
        for i in range(X4_CHUNKS_PER_PLANE)
    ]
