from __future__ import annotations

import hashlib
import struct


def sha256_digest(data: bytes) -> bytes:
    return hashlib.sha256(data).digest()


def hash_with_zeroed_range(data: bytes, start: int, length: int = 32) -> bytes:
    mutable = bytearray(data)
    mutable[start : start + length] = bytes(length)
    return sha256_digest(bytes(mutable))


def rendition_hash(
    source_sha256: bytes,
    display_profile_hash: bytes,
    layout_profile_hash: bytes,
    font_policy_hash: bytes,
    typography_policy_hash: bytes,
    image_policy_hash: bytes,
    compression_policy_hash: bytes,
    chrome_policy_hash: bytes,
    compiler_version: str,
) -> bytes:
    version = compiler_version.encode("utf-8")
    return sha256_digest(
        b"".join(
            [
                source_sha256,
                display_profile_hash,
                layout_profile_hash,
                font_policy_hash,
                typography_policy_hash,
                image_policy_hash,
                compression_policy_hash,
                chrome_policy_hash,
                struct.pack("<I", len(version)),
                version,
            ]
        )
    )
