from __future__ import annotations

from enum import IntEnum

MAGIC = b"BINBOOK\0"
VERSION_MAJOR = 0
VERSION_MINOR = 1
PAGE_DATA_ALIGNMENT = 65536
UINT32_MAX = 0xFFFFFFFF


class SectionId(IntEnum):
    INVALID = 0
    STRING_TABLE = 1
    DISPLAY_PROFILE = 10
    LAYOUT_PROFILE = 11
    READER_REQUIREMENTS = 12
    SOURCE_IDENTITY = 20
    BOOK_METADATA = 21
    RENDITION_IDENTITY = 22
    FONT_POLICY = 30
    TYPOGRAPHY_POLICY = 31
    IMAGE_POLICY = 32
    COMPRESSION_POLICY = 33
    CHROME_POLICY = 34
    PAGE_INDEX = 40
    NAV_INDEX = 41
    PAGE_LABELS_RESERVED = 42
    PAGE_DATA = 50


REQUIRED_SECTIONS = {
    SectionId.STRING_TABLE,
    SectionId.DISPLAY_PROFILE,
    SectionId.LAYOUT_PROFILE,
    SectionId.READER_REQUIREMENTS,
    SectionId.SOURCE_IDENTITY,
    SectionId.BOOK_METADATA,
    SectionId.RENDITION_IDENTITY,
    SectionId.FONT_POLICY,
    SectionId.TYPOGRAPHY_POLICY,
    SectionId.IMAGE_POLICY,
    SectionId.COMPRESSION_POLICY,
    SectionId.CHROME_POLICY,
    SectionId.PAGE_INDEX,
    SectionId.NAV_INDEX,
    SectionId.PAGE_DATA,
}


class PixelFormat(IntEnum):
    GRAY1_PACKED = 1
    GRAY2_PACKED = 2
    GRAY4_PACKED = 4


class PixelFormatFlag(IntEnum):
    GRAY1_PACKED = 1 << 0
    GRAY2_PACKED = 1 << 1
    GRAY4_PACKED = 1 << 2


class CompressionMethod(IntEnum):
    NONE = 0
    RLE_PACKBITS = 1


class PageKind(IntEnum):
    TEXT = 1
    IMAGE = 2
    MIXED_RESERVED = 3


class SourceType(IntEnum):
    UNKNOWN = 0
    EPUB = 1
