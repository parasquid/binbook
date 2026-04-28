from __future__ import annotations

from dataclasses import dataclass
import struct

from .constants import MAGIC, VERSION_MAJOR, VERSION_MINOR, SectionId

HEADER_SIZE = 256
SECTION_ENTRY_SIZE = 40
PAGE_INDEX_ENTRY_SIZE = 76
NAV_INDEX_ENTRY_SIZE = 48

_HEADER = struct.Struct("<8sHHHHQQIHHHHQQII")
_STRING_REF = struct.Struct("<II")
_SECTION = struct.Struct("<HHQQIII8s")
_PAGE_INDEX = struct.Struct("<IHHHHI QIII HHHH II II 16s")
_NAV_INDEX = struct.Struct("<IHH II II IIIIII")


@dataclass(frozen=True)
class StringRef:
    offset: int = 0
    length: int = 0

    def pack(self) -> bytes:
        return _STRING_REF.pack(self.offset, self.length)

    @classmethod
    def unpack(cls, data: bytes, offset: int = 0) -> "StringRef":
        return cls(*_STRING_REF.unpack_from(data, offset))


@dataclass(frozen=True)
class BinBookHeader:
    file_size: int = 0
    section_table_offset: int = HEADER_SIZE
    section_table_length: int = 0
    section_table_entry_size: int = SECTION_ENTRY_SIZE
    section_count: int = 0
    page_index_entry_size: int = PAGE_INDEX_ENTRY_SIZE
    nav_index_entry_size: int = NAV_INDEX_ENTRY_SIZE
    page_data_offset: int = 0
    page_data_length: int = 0
    file_crc32: int = 0
    header_crc32: int = 0
    version_major: int = VERSION_MAJOR
    version_minor: int = VERSION_MINOR
    header_flags: int = 0
    header_size: int = HEADER_SIZE

    def pack(self) -> bytes:
        data = _HEADER.pack(
            MAGIC,
            self.version_major,
            self.version_minor,
            self.header_size,
            self.header_flags,
            self.file_size,
            self.section_table_offset,
            self.section_table_length,
            self.section_table_entry_size,
            self.section_count,
            self.page_index_entry_size,
            self.nav_index_entry_size,
            self.page_data_offset,
            self.page_data_length,
            self.file_crc32,
            self.header_crc32,
        )
        return data + bytes(HEADER_SIZE - len(data))

    @classmethod
    def unpack(cls, data: bytes) -> "BinBookHeader":
        if len(data) < HEADER_SIZE:
            raise ValueError("header is shorter than 256 bytes")
        (
            magic,
            version_major,
            version_minor,
            header_size,
            header_flags,
            file_size,
            section_table_offset,
            section_table_length,
            section_table_entry_size,
            section_count,
            page_index_entry_size,
            nav_index_entry_size,
            page_data_offset,
            page_data_length,
            file_crc32,
            header_crc32,
        ) = _HEADER.unpack_from(data)
        if magic != MAGIC:
            raise ValueError("invalid BinBook magic")
        return cls(
            file_size=file_size,
            section_table_offset=section_table_offset,
            section_table_length=section_table_length,
            section_table_entry_size=section_table_entry_size,
            section_count=section_count,
            page_index_entry_size=page_index_entry_size,
            nav_index_entry_size=nav_index_entry_size,
            page_data_offset=page_data_offset,
            page_data_length=page_data_length,
            file_crc32=file_crc32,
            header_crc32=header_crc32,
            version_major=version_major,
            version_minor=version_minor,
            header_flags=header_flags,
            header_size=header_size,
        )


@dataclass(frozen=True)
class SectionEntry:
    section_id: int
    offset: int
    length: int
    entry_size: int = 0
    record_count: int = 0
    crc32: int = 0
    section_flags: int = 0

    def pack(self) -> bytes:
        return _SECTION.pack(
            int(self.section_id),
            self.section_flags,
            self.offset,
            self.length,
            self.entry_size,
            self.record_count,
            self.crc32,
            bytes(8),
        )

    @classmethod
    def unpack(cls, data: bytes, offset: int = 0) -> "SectionEntry":
        section_id, flags, section_offset, length, entry_size, record_count, crc, _ = _SECTION.unpack_from(data, offset)
        return cls(section_id, section_offset, length, entry_size, record_count, crc, flags)

    @property
    def id(self) -> SectionId:
        return SectionId(self.section_id)


@dataclass(frozen=True)
class PageIndexEntry:
    page_number: int
    page_kind: int
    pixel_format: int
    compression_method: int
    relative_blob_offset: int
    compressed_size: int
    uncompressed_size: int
    page_crc32: int
    stored_width: int
    stored_height: int
    placement_x: int = 0
    placement_y: int = 0
    update_hint: int = 0
    page_flags: int = 0
    source_spine_index: int = 0xFFFFFFFF
    chapter_nav_index: int = 0xFFFFFFFF
    progress_start_ppm: int = 0
    progress_end_ppm: int = 0

    def pack(self) -> bytes:
        return _PAGE_INDEX.pack(
            self.page_number,
            self.page_kind,
            self.pixel_format,
            self.compression_method,
            self.update_hint,
            self.page_flags,
            self.relative_blob_offset,
            self.compressed_size,
            self.uncompressed_size,
            self.page_crc32,
            self.stored_width,
            self.stored_height,
            self.placement_x,
            self.placement_y,
            self.source_spine_index,
            self.chapter_nav_index,
            self.progress_start_ppm,
            self.progress_end_ppm,
            bytes(16),
        )

    @classmethod
    def unpack(cls, data: bytes, offset: int = 0) -> "PageIndexEntry":
        values = _PAGE_INDEX.unpack_from(data, offset)
        return cls(
            page_number=values[0],
            page_kind=values[1],
            pixel_format=values[2],
            compression_method=values[3],
            update_hint=values[4],
            page_flags=values[5],
            relative_blob_offset=values[6],
            compressed_size=values[7],
            uncompressed_size=values[8],
            page_crc32=values[9],
            stored_width=values[10],
            stored_height=values[11],
            placement_x=values[12],
            placement_y=values[13],
            source_spine_index=values[14],
            chapter_nav_index=values[15],
            progress_start_ppm=values[16],
            progress_end_ppm=values[17],
        )


@dataclass(frozen=True)
class NavIndexEntry:
    nav_index: int
    nav_type: int
    title: StringRef
    target_page_number: int
    level: int = 0
    source_href: StringRef = StringRef()
    source_spine_index: int = 0xFFFFFFFF
    parent_nav_index: int = 0xFFFFFFFF
    first_child_nav_index: int = 0xFFFFFFFF
    next_sibling_nav_index: int = 0xFFFFFFFF
    nav_flags: int = 0

    def pack(self) -> bytes:
        return _NAV_INDEX.pack(
            self.nav_index,
            self.nav_type,
            self.level,
            self.title.offset,
            self.title.length,
            self.source_href.offset,
            self.source_href.length,
            self.source_spine_index,
            self.target_page_number,
            self.parent_nav_index,
            self.first_child_nav_index,
            self.next_sibling_nav_index,
            self.nav_flags,
        )

    @classmethod
    def unpack(cls, data: bytes, offset: int = 0) -> "NavIndexEntry":
        values = _NAV_INDEX.unpack_from(data, offset)
        return cls(
            nav_index=values[0],
            nav_type=values[1],
            level=values[2],
            title=StringRef(values[3], values[4]),
            source_href=StringRef(values[5], values[6]),
            source_spine_index=values[7],
            target_page_number=values[8],
            parent_nav_index=values[9],
            first_child_nav_index=values[10],
            next_sibling_nav_index=values[11],
            nav_flags=values[12],
        )


assert _HEADER.size == 68
assert _STRING_REF.size == 8
assert _SECTION.size == SECTION_ENTRY_SIZE
assert _PAGE_INDEX.size == PAGE_INDEX_ENTRY_SIZE
assert _NAV_INDEX.size == NAV_INDEX_ENTRY_SIZE
