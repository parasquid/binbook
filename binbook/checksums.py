from __future__ import annotations

import binascii


def crc32(data: bytes) -> int:
    return binascii.crc32(data) & 0xFFFFFFFF
