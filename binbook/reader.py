from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from .checksums import crc32
from .constants import (
    REQUIRED_SECTIONS,
    VERSION_MAJOR,
    CompressionMethod,
    PageKind,
    PixelFormat,
    SectionId,
)
from .images import gray2_packed_to_png
from .rle import decode_packbits
from .structs import (
    HEADER_SIZE,
    NAV_INDEX_ENTRY_SIZE,
    PAGE_INDEX_ENTRY_SIZE,
    SECTION_ENTRY_SIZE,
    BinBookHeader,
    PageIndexEntry,
    SectionEntry,
)


@dataclass
class BinBookReader:
    path: Path
    data: bytes
    header: BinBookHeader
    sections: dict[SectionId, SectionEntry]
    pages: list[PageIndexEntry]

    @classmethod
    def open(cls, path: Path | str) -> "BinBookReader":
        book_path = Path(path)
        data = book_path.read_bytes()
        header = BinBookHeader.unpack(data[:HEADER_SIZE])
        sections = _read_sections(data, header)
        pages = _read_pages(data, sections)
        reader = cls(book_path, data, header, sections, pages)
        reader.validate()
        return reader

    def validate(self) -> None:
        if self.header.version_major != VERSION_MAJOR:
            raise ValueError("unsupported BinBook major version")
        if self.header.section_table_entry_size != SECTION_ENTRY_SIZE:
            raise ValueError("unsupported section entry size")
        if self.header.page_index_entry_size != PAGE_INDEX_ENTRY_SIZE:
            raise ValueError("unsupported page index entry size")
        if self.header.nav_index_entry_size != NAV_INDEX_ENTRY_SIZE:
            raise ValueError("unsupported nav index entry size")
        if self.header.file_size and len(self.data) < self.header.file_size:
            raise ValueError("file is smaller than header.file_size")
        missing = REQUIRED_SECTIONS.difference(self.sections)
        if missing:
            raise ValueError(f"missing required sections: {sorted(int(s) for s in missing)}")
        page_data = self.sections[SectionId.PAGE_DATA]
        if page_data.offset != self.header.page_data_offset or page_data.length != self.header.page_data_length:
            raise ValueError("PAGE_DATA section does not match header")
        for section in self.sections.values():
            if section.offset + section.length > len(self.data):
                raise ValueError(f"section {section.section_id} is outside file bounds")
        used: list[tuple[int, int]] = []
        previous_progress = 0
        for page in self.pages:
            if page.page_kind == PageKind.MIXED_RESERVED:
                raise ValueError("MIXED_RESERVED pages are unsupported in v0.1")
            if page.pixel_format != PixelFormat.GRAY2_PACKED:
                raise ValueError("xteink-x4-portrait requires GRAY2_PACKED pages")
            if page.compression_method != CompressionMethod.RLE_PACKBITS:
                raise ValueError("unsupported page compression method")
            start = page.relative_blob_offset
            end = start + page.compressed_size
            if end > self.header.page_data_length:
                raise ValueError("page blob is outside PAGE_DATA")
            if page.progress_start_ppm > page.progress_end_ppm or page.progress_end_ppm > 1_000_000:
                raise ValueError("invalid page progress range")
            if page.progress_start_ppm < previous_progress:
                raise ValueError("page progress is not monotonic")
            previous_progress = page.progress_end_ppm
            for other_start, other_end in used:
                if start < other_end and end > other_start:
                    raise ValueError("page blobs overlap")
            used.append((start, end))

    def decode_page_bytes(self, page_number: int) -> tuple[bytes, PageIndexEntry]:
        page = self.pages[page_number]
        absolute = self.header.page_data_offset + page.relative_blob_offset
        compressed = self.data[absolute : absolute + page.compressed_size]
        if page.page_crc32 and crc32(compressed) != page.page_crc32:
            raise ValueError("page CRC32 mismatch")
        unpacked = decode_packbits(compressed)
        if len(unpacked) != page.uncompressed_size:
            raise ValueError("decompressed page size mismatch")
        return unpacked, page

    def decode_page_to_png(self, page_number: int, output: Path | str) -> None:
        unpacked, page = self.decode_page_bytes(page_number)
        gray2_packed_to_png(unpacked, page.stored_width, page.stored_height, Path(output))


def _read_sections(data: bytes, header: BinBookHeader) -> dict[SectionId, SectionEntry]:
    start = header.section_table_offset
    end = start + header.section_table_length
    if end > len(data):
        raise ValueError("section table is outside file bounds")
    sections: dict[SectionId, SectionEntry] = {}
    for index in range(header.section_count):
        entry = SectionEntry.unpack(data, start + index * header.section_table_entry_size)
        try:
            sections[SectionId(entry.section_id)] = entry
        except ValueError:
            continue
    return sections


def _read_pages(data: bytes, sections: dict[SectionId, SectionEntry]) -> list[PageIndexEntry]:
    section = sections.get(SectionId.PAGE_INDEX)
    if section is None:
        return []
    return [
        PageIndexEntry.unpack(data, section.offset + index * section.entry_size)
        for index in range(section.record_count)
    ]
