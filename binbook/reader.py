from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

from .checksums import crc32
from .constants import (
    REQUIRED_SECTIONS,
    CompressionMethod,
    PageKind,
    PixelFormat,
    PixelFormatFlag,
    SectionId,
)
from .images import packed_to_png
from .rle import decode_packbits
from .sections import (
    SECTION_STRING_REF_OFFSETS,
    DisplayProfileSection,
    LayoutProfileSection,
    ReaderRequirementsSection,
)
from .structs import (
    HEADER_SIZE,
    CHAPTER_INDEX_ENTRY_SIZE,
    NAV_INDEX_ENTRY_SIZE,
    PAGE_CHUNK_INDEX_ENTRY_SIZE,
    PAGE_INDEX_ENTRY_SIZE,
    PAGE_TRANSITION_INDEX_ENTRY_SIZE,
    SECTION_ENTRY_SIZE,
    BinBookHeader,
    ChapterIndexEntry,
    NavIndexEntry,
    PageChunkIndexEntry,
    PageIndexEntry,
    PageTransitionIndexEntry,
    SectionEntry,
    StringRef,
)

SUPPORTED_READER_FEATURES = (1 << 0) | (1 << 2) | (1 << 3) | (1 << 4)
SUPPORTED_STORAGE_PIXEL_FORMATS = int(
    PixelFormatFlag.GRAY1_PACKED | PixelFormatFlag.GRAY2_PACKED
)
SUPPORTED_COMPRESSION_METHOD_FLAGS = 1 << int(CompressionMethod.RLE_PACKBITS)


@dataclass
class BinBookReader:
    path: Path
    data: bytes
    header: BinBookHeader
    sections: dict[SectionId, SectionEntry]
    pages: list[PageIndexEntry]
    chapters: list[ChapterIndexEntry]
    page_chunks: list[PageChunkIndexEntry] = field(default_factory=list)
    page_transitions: list[PageTransitionIndexEntry] = field(default_factory=list)

    @classmethod
    def open(cls, path: Path | str, *, validate: bool = True) -> "BinBookReader":
        path = Path(path)
        data = path.read_bytes()
        header = BinBookHeader.unpack(data[:HEADER_SIZE])
        sections = _read_sections(data, header)
        pages = _read_pages(data, sections)
        chapters = _read_chapters(data, sections)
        page_chunks = _read_page_chunks(data, sections)
        page_transitions = _read_page_transitions(data, sections)
        reader = cls(
            path, data, header, sections, pages, chapters, page_chunks, page_transitions
        )
        if validate:
            reader.validate()
        return reader

    def validate(self) -> None:
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
            raise ValueError(
                f"missing required sections: {sorted(int(s) for s in missing)}"
            )
        metadata_end = max(
            section.offset + section.length
            for section_id, section in self.sections.items()
            if section_id != SectionId.PAGE_DATA
        )
        if self.header.page_data_offset < metadata_end:
            raise ValueError("page_data_offset is before end of metadata")
        page_data = self.sections[SectionId.PAGE_DATA]
        if (
            page_data.offset != self.header.page_data_offset
            or page_data.length != self.header.page_data_length
        ):
            raise ValueError("PAGE_DATA section does not match header")
        for section in self.sections.values():
            if section.offset + section.length > len(self.data):
                raise ValueError(f"section {section.section_id} is outside file bounds")
            if (
                section.crc32
                and crc32(self.data[section.offset : section.offset + section.length])
                != section.crc32
            ):
                name = SectionId(section.section_id).name
                raise ValueError(f"section {name} CRC32 mismatch")
        self._validate_reader_requirements()
        self._validate_display_and_layout_profiles()
        self._validate_string_refs()
        required_storage_formats = ReaderRequirementsSection.unpack(
            self._section_data(SectionId.READER_REQUIREMENTS)
        ).required_storage_pixel_format_flags
        previous_progress = 0
        for page in self.pages:
            if page.page_kind == PageKind.MIXED_RESERVED:
                raise ValueError("MIXED_RESERVED pages are unsupported")
            if page.pixel_format == PixelFormat.GRAY1_PACKED:
                page_format_flag = int(PixelFormatFlag.GRAY1_PACKED)
            elif page.pixel_format == PixelFormat.GRAY2_PACKED:
                page_format_flag = int(PixelFormatFlag.GRAY2_PACKED)
            else:
                raise ValueError(f"unsupported page pixel format: {page.pixel_format}")
            if not required_storage_formats & page_format_flag:
                raise ValueError("page pixel format does not match reader requirements")
            if page.compression_method != CompressionMethod.RLE_PACKBITS:
                raise ValueError("unsupported page compression method")
            for slot in range(4):
                if page.plane_dir.bitmap & (1 << slot):
                    blob_end = page.plane_dir.offsets[slot] + page.plane_dir.sizes[slot]
                    if blob_end > self.header.page_data_length:
                        raise ValueError("page blob is outside PAGE_DATA")
            if (
                page.progress_start_ppm > page.progress_end_ppm
                or page.progress_end_ppm > 1_000_000
            ):
                raise ValueError("invalid page progress range")
            if page.progress_start_ppm < previous_progress:
                raise ValueError("page progress is not monotonic")
            previous_progress = page.progress_end_ppm
        chapter_section = self.sections[SectionId.CHAPTER_INDEX]
        if chapter_section.entry_size != CHAPTER_INDEX_ENTRY_SIZE:
            raise ValueError("unsupported chapter index entry size")
        if chapter_section.record_count != len(self.chapters):
            raise ValueError("chapter index count mismatch")
        for expected, chapter in enumerate(self.chapters):
            if chapter.chapter_index != expected:
                raise ValueError("chapter index is not contiguous")
            if chapter.target_page_number >= len(self.pages):
                raise ValueError("chapter target page is out of range")
            if chapter.nav_type not in (3, 4):
                raise ValueError("chapter index contains non-selectable nav type")
        chunk_section = self.sections.get(SectionId.PAGE_CHUNK_INDEX)
        if chunk_section is not None:
            if chunk_section.entry_size != PAGE_CHUNK_INDEX_ENTRY_SIZE:
                raise ValueError("unsupported page chunk index entry size")
            if chunk_section.record_count != len(self.page_chunks):
                raise ValueError("page chunk index count mismatch")
            for chunk in self.page_chunks:
                chunk_end = chunk.page_data_offset + chunk.compressed_size
                if chunk_end > self.header.page_data_length:
                    raise ValueError("page chunk is outside PAGE_DATA")
        transition_section = self.sections.get(SectionId.PAGE_TRANSITION_INDEX)
        if transition_section is not None:
            if transition_section.entry_size != PAGE_TRANSITION_INDEX_ENTRY_SIZE:
                raise ValueError("unsupported page transition index entry size")
            if transition_section.record_count != len(self.page_transitions):
                raise ValueError("page transition index count mismatch")

    def _validate_reader_requirements(self) -> None:
        data = self._section_data(SectionId.READER_REQUIREMENTS)
        requirements = ReaderRequirementsSection.unpack(data)
        unsupported_features = (
            requirements.required_features & ~SUPPORTED_READER_FEATURES
        )
        if unsupported_features:
            raise ValueError(
                f"unsupported required reader features: 0x{unsupported_features:x}"
            )
        if (
            not requirements.required_storage_pixel_format_flags
            & SUPPORTED_STORAGE_PIXEL_FORMATS
        ):
            raise ValueError("unsupported required storage pixel formats")
        if requirements.required_grayscale_levels not in (0, 2, 4):
            raise ValueError("unsupported required output grayscale levels")
        if (
            not requirements.required_compression_method_flags
            & SUPPORTED_COMPRESSION_METHOD_FLAGS
        ):
            raise ValueError("unsupported required compression methods")

    def _validate_display_and_layout_profiles(self) -> None:
        errors = self.profile_validation_errors()
        if errors:
            raise ValueError(errors[0])

    def profile_validation_errors(self) -> list[str]:
        errors: list[str] = []
        try:
            display = DisplayProfileSection.unpack(
                self._section_data(SectionId.DISPLAY_PROFILE)
            )
            layout = LayoutProfileSection.unpack(
                self._section_data(SectionId.LAYOUT_PROFILE)
            )
        except ValueError as exc:
            return [str(exc)]
        if display.logical_width == 0 or display.logical_height == 0:
            errors.append("display profile logical dimensions must be non-zero")
        if display.supported_storage_pixel_format_flags == 0:
            errors.append(
                "display profile must advertise at least one storage pixel format"
            )
        if display.native_grayscale_levels < 2:
            errors.append("display profile must use at least 2 grayscale levels")
        if (layout.full_width, layout.full_height) != (
            display.logical_width,
            display.logical_height,
        ):
            errors.append(
                "LayoutProfile full page dimensions do not match DisplayProfile"
            )
        expected_x = layout.margin_left
        expected_y = layout.margin_top + layout.header_height
        expected_width = layout.full_width - layout.margin_left - layout.margin_right
        expected_height = (
            layout.full_height
            - layout.margin_top
            - layout.margin_bottom
            - layout.header_height
            - layout.footer_height
        )
        if (
            layout.content_x,
            layout.content_y,
            layout.content_width,
            layout.content_height,
        ) != (
            expected_x,
            expected_y,
            expected_width,
            expected_height,
        ):
            errors.append("LayoutProfile content box is inconsistent with margins")
        return errors

    def _validate_string_refs(self) -> None:
        table = self._section_data(SectionId.STRING_TABLE)
        for section_id, offsets in SECTION_STRING_REF_OFFSETS.items():
            section = self.sections[section_id]
            data = self._section_data(section_id)
            record_count = section.record_count if section.entry_size else 1
            stride = section.entry_size if section.entry_size else 0
            for record_index in range(record_count):
                base = record_index * stride
                for offset in offsets:
                    absolute = base + offset
                    if absolute + 8 > len(data):
                        raise ValueError(
                            f"{section_id.name} StringRef field is outside section"
                        )
                    ref = StringRef.unpack(data, absolute)
                    if ref.length == 0:
                        continue
                    if ref.offset + ref.length > len(table):
                        raise ValueError("StringRef is outside the string table")
                    table[ref.offset : ref.offset + ref.length].decode("utf-8")

    def _section_data(self, section_id: SectionId) -> bytes:
        section = self.sections[section_id]
        return self.data[section.offset : section.offset + section.length]

    def decode_page_bytes(self, page_number: int) -> tuple[bytes, PageIndexEntry]:
        page = self.pages[page_number]
        pd = page.plane_dir
        if pd.bitmap == 0x07 and page.pixel_format == PixelFormat.GRAY2_PACKED:
            return self._decode_x4_native_page(page)
        parts = []
        for slot in range(4):
            if not (pd.bitmap & (1 << slot)):
                continue
            absolute = self.header.page_data_offset + pd.offsets[slot]
            compressed = self.data[absolute : absolute + pd.sizes[slot]]
            method = (
                pd.compression[slot]
                if (page.page_flags & 1)
                else page.compression_method
            )
            parts.append(decode_packbits(compressed))
        unpacked = b"".join(parts)
        return unpacked, page

    def _decode_x4_native_page(
        self, page: PageIndexEntry
    ) -> tuple[bytes, PageIndexEntry]:
        from .pixels import (
            X4_PHYSICAL_HEIGHT,
            X4_PHYSICAL_WIDTH,
            X4_ROW_BYTES,
            pack_gray2,
        )

        pd = page.plane_dir
        planes_data: list[bytes] = []
        for slot in range(3):
            if not (pd.bitmap & (1 << slot)):
                planes_data.append(b"\xff" * (X4_ROW_BYTES * X4_PHYSICAL_HEIGHT))
                continue
            absolute = self.header.page_data_offset + pd.offsets[slot]
            compressed = self.data[absolute : absolute + pd.sizes[slot]]
            planes_data.append(decode_packbits(compressed))
        msb_rows = planes_data[0]
        lsb_rows = planes_data[1]
        logical_width = 480
        logical_height = 800
        pixels = [3] * (logical_width * logical_height)
        for physical_y in range(X4_PHYSICAL_HEIGHT):
            for physical_x in range(X4_PHYSICAL_WIDTH):
                ram_x = X4_PHYSICAL_WIDTH - 1 - physical_x
                byte_idx = ram_x // 8
                bit_mask = 0x80 >> (ram_x % 8)
                msb_set = (
                    msb_rows[physical_y * X4_ROW_BYTES + byte_idx] & bit_mask
                ) != 0
                lsb_set = (
                    lsb_rows[physical_y * X4_ROW_BYTES + byte_idx] & bit_mask
                ) != 0
                if msb_set and lsb_set:
                    gray = 3
                elif msb_set and not lsb_set:
                    gray = 2
                elif not msb_set and lsb_set:
                    gray = 1
                else:
                    gray = 0
                logical_x = physical_y
                logical_y = X4_PHYSICAL_WIDTH - 1 - physical_x
                if 0 <= logical_x < logical_width and 0 <= logical_y < logical_height:
                    pixels[logical_y * logical_width + logical_x] = gray
        packed = pack_gray2(pixels, logical_width, logical_height)
        return packed, page

    def decode_page_to_png(self, page_number: int, output: Path | str) -> None:
        unpacked, page = self.decode_page_bytes(page_number)
        packed_to_png(
            unpacked,
            PixelFormat(page.pixel_format),
            page.stored_width,
            page.stored_height,
            Path(output),
        )


def _read_sections(data: bytes, header: BinBookHeader) -> dict[SectionId, SectionEntry]:
    start = header.section_table_offset
    end = start + header.section_table_length
    if end > len(data):
        raise ValueError("section table is outside file bounds")
    sections: dict[SectionId, SectionEntry] = {}
    for index in range(header.section_count):
        entry = SectionEntry.unpack(
            data, start + index * header.section_table_entry_size
        )
        try:
            sections[SectionId(entry.section_id)] = entry
        except ValueError:
            continue
    return sections


def _read_pages(
    data: bytes, sections: dict[SectionId, SectionEntry]
) -> list[PageIndexEntry]:
    section = sections.get(SectionId.PAGE_INDEX)
    if section is None:
        return []
    return [
        PageIndexEntry.unpack(data, section.offset + index * section.entry_size)
        for index in range(section.record_count)
    ]


def _read_chapters(
    data: bytes, sections: dict[SectionId, SectionEntry]
) -> list[ChapterIndexEntry]:
    section = sections.get(SectionId.CHAPTER_INDEX)
    if section is None:
        return []
    return [
        ChapterIndexEntry.unpack(data, section.offset + index * section.entry_size)
        for index in range(section.record_count)
    ]


def _read_page_chunks(
    data: bytes, sections: dict[SectionId, SectionEntry]
) -> list[PageChunkIndexEntry]:
    section = sections.get(SectionId.PAGE_CHUNK_INDEX)
    if section is None:
        return []
    return [
        PageChunkIndexEntry.unpack(data, section.offset + index * section.entry_size)
        for index in range(section.record_count)
    ]


def _read_page_transitions(
    data: bytes, sections: dict[SectionId, SectionEntry]
) -> list[PageTransitionIndexEntry]:
    section = sections.get(SectionId.PAGE_TRANSITION_INDEX)
    if section is None:
        return []
    return [
        PageTransitionIndexEntry.unpack(
            data, section.offset + index * section.entry_size
        )
        for index in range(section.record_count)
    ]
