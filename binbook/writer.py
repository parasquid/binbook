from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import struct

from .checksums import crc32
from .constants import (
    PAGE_DATA_ALIGNMENT,
    UINT32_MAX,
    CompressionMethod,
    PageKind,
    PixelFormat,
    PixelFormatFlag,
    SectionId,
    SourceType,
)
from .fonts import FontInfo, get_font
from .images import png_to_gray2_packed
from .profiles import DisplayProfile, get_profile
from .rle import encode_packbits
from .strings import StringTableBuilder
from .structs import (
    HEADER_SIZE,
    NAV_INDEX_ENTRY_SIZE,
    PAGE_INDEX_ENTRY_SIZE,
    SECTION_ENTRY_SIZE,
    BinBookHeader,
    PageIndexEntry,
    SectionEntry,
    StringRef,
)


@dataclass(frozen=True)
class EncodedPage:
    compressed: bytes
    uncompressed_size: int
    page_crc32: int
    page_kind: int = PageKind.IMAGE
    source_spine_index: int = UINT32_MAX
    chapter_nav_index: int = UINT32_MAX


@dataclass(frozen=True)
class BookInfo:
    title: str = ""
    author: str = ""
    language: str = ""
    package_identifier: str = ""


@dataclass(frozen=True)
class SourceInfo:
    source_type: int = SourceType.UNKNOWN
    filename: str = ""
    file_size: int = 0
    md5: bytes = bytes(16)
    sha256: bytes = bytes(32)
    package_identifier: str = ""


@dataclass(frozen=True)
class NavEntry:
    title: str
    target_page_number: int
    source_spine_index: int = UINT32_MAX
    nav_type: int = 3
    level: int = 0


def encode_png_folder(input_dir: Path, output: Path, profile_name: str = "xteink-x4-portrait") -> None:
    profile = get_profile(profile_name)
    pngs = sorted(p for p in input_dir.iterdir() if p.suffix.lower() == ".png")
    if not pngs:
        raise ValueError("input folder contains no PNG files")

    pages: list[EncodedPage] = []
    for path in pngs:
        packed = png_to_gray2_packed(path, profile)
        compressed = encode_packbits(packed)
        pages.append(EncodedPage(compressed, len(packed), crc32(compressed)))

    output.write_bytes(build_binbook(pages, profile, source_name=input_dir.name))


def build_binbook(
    pages: list[EncodedPage],
    profile: DisplayProfile,
    source_name: str = "png-folder",
    *,
    book_info: BookInfo | None = None,
    source_info: SourceInfo | None = None,
    nav_entries: list[NavEntry] | None = None,
    font_info: FontInfo | None = None,
) -> bytes:
    book_info = book_info or BookInfo(title=source_name)
    source_info = source_info or SourceInfo(filename=source_name)
    nav_entries = nav_entries or []
    font_info = font_info or get_font("literata")
    strings = StringTableBuilder()
    refs = {
        "profile": strings.add(profile.name),
        "family": strings.add("xteink"),
        "model": strings.add("x4"),
        "title": strings.add(book_info.title or source_name),
        "author": strings.add(book_info.author),
        "language": strings.add(book_info.language),
        "package_identifier": strings.add(book_info.package_identifier or source_info.package_identifier),
        "filename": strings.add(source_info.filename or source_name),
        "compiler": strings.add("binbook-poc"),
        "version": strings.add("0.1.0"),
        "renderer": strings.add("Pillow"),
        "font_name": strings.add(font_info.display_name),
        "font_path": strings.add(font_info.stable_path),
    }
    nav_title_refs = [strings.add(entry.title) for entry in nav_entries]

    sections: list[tuple[SectionId, bytes, int, int]] = [
        (SectionId.STRING_TABLE, b"", 0, 0),
        (SectionId.DISPLAY_PROFILE, _display_profile(profile, refs), 0, 0),
        (SectionId.LAYOUT_PROFILE, _layout_profile(profile), 0, 0),
        (SectionId.READER_REQUIREMENTS, _reader_requirements(profile), 0, 0),
        (SectionId.SOURCE_IDENTITY, _source_identity(source_info, refs["filename"], refs["package_identifier"]), 0, 0),
        (SectionId.BOOK_METADATA, _book_metadata(refs["title"], refs["author"], refs["language"]), 0, 0),
        (SectionId.RENDITION_IDENTITY, _rendition_identity(refs["compiler"], refs["version"]), 0, 0),
        (SectionId.FONT_POLICY, _font_policy(font_info, refs["font_name"], refs["font_path"], refs["renderer"]), 0, 0),
        (SectionId.TYPOGRAPHY_POLICY, _typography_policy(), 0, 0),
        (SectionId.IMAGE_POLICY, _image_policy(), 0, 0),
        (SectionId.COMPRESSION_POLICY, _compression_policy(), 0, 0),
        (SectionId.CHROME_POLICY, _chrome_policy(), 0, 0),
    ]

    page_index = _page_index(pages, profile)
    nav_index = _nav_index(nav_entries, nav_title_refs)
    sections.extend(
        [
            (SectionId.PAGE_INDEX, page_index, PAGE_INDEX_ENTRY_SIZE, len(pages)),
            (SectionId.NAV_INDEX, nav_index, NAV_INDEX_ENTRY_SIZE, len(nav_entries)),
        ]
    )

    string_table = strings.to_bytes()
    sections[0] = (SectionId.STRING_TABLE, string_table, 0, 0)

    section_count = len(sections) + 1
    section_table_length = section_count * SECTION_ENTRY_SIZE
    cursor = HEADER_SIZE + section_table_length
    section_entries: list[SectionEntry] = []
    data_parts: list[bytes] = []
    for section_id, data, entry_size, record_count in sections:
        section_entries.append(SectionEntry(section_id, cursor, len(data), entry_size, record_count, crc32(data) if data else 0))
        data_parts.append(data)
        cursor += len(data)

    page_data_offset = _align_up(cursor, PAGE_DATA_ALIGNMENT)
    page_data = b"".join(page.compressed for page in pages)
    section_entries.append(SectionEntry(SectionId.PAGE_DATA, page_data_offset, len(page_data), 0, 0, crc32(page_data)))

    header = BinBookHeader(
        file_size=page_data_offset + len(page_data),
        section_table_offset=HEADER_SIZE,
        section_table_length=section_table_length,
        section_count=section_count,
        page_data_offset=page_data_offset,
        page_data_length=len(page_data),
    )
    section_table = b"".join(entry.pack() for entry in section_entries)
    metadata = b"".join(data_parts)
    padding = bytes(page_data_offset - (HEADER_SIZE + len(section_table) + len(metadata)))
    return header.pack() + section_table + metadata + padding + page_data


def _page_index(pages: list[EncodedPage], profile: DisplayProfile) -> bytes:
    out = bytearray()
    relative = 0
    total = len(pages)
    for index, page in enumerate(pages):
        start = int(index * 1_000_000 / total)
        end = int((index + 1) * 1_000_000 / total)
        out.extend(
            PageIndexEntry(
                page_number=index,
                page_kind=page.page_kind,
                pixel_format=PixelFormat.GRAY2_PACKED,
                compression_method=CompressionMethod.RLE_PACKBITS,
                relative_blob_offset=relative,
                compressed_size=len(page.compressed),
                uncompressed_size=page.uncompressed_size,
                page_crc32=page.page_crc32,
                stored_width=profile.logical_width,
                stored_height=profile.logical_height,
                source_spine_index=page.source_spine_index,
                chapter_nav_index=page.chapter_nav_index,
                progress_start_ppm=start,
                progress_end_ppm=end,
            ).pack()
        )
        relative += len(page.compressed)
    return bytes(out)


def _nav_index(entries: list[NavEntry], title_refs: list[StringRef]) -> bytes:
    out = bytearray()
    for index, entry in enumerate(entries):
        from .structs import NavIndexEntry

        out.extend(
            NavIndexEntry(
                nav_index=index,
                nav_type=entry.nav_type,
                level=entry.level,
                title=title_refs[index],
                target_page_number=entry.target_page_number,
                source_spine_index=entry.source_spine_index,
            ).pack()
        )
    return bytes(out)


def _display_profile(profile: DisplayProfile, refs: dict[str, StringRef]) -> bytes:
    return b"".join(
        [
            refs["profile"].pack(),
            refs["family"].pack(),
            refs["model"].pack(),
            struct.pack(
                "<HHHHBhBIIHHHHBHB",
                profile.logical_width,
                profile.logical_height,
                profile.physical_width,
                profile.physical_height,
                1,
                0,
                1,
                PixelFormatFlag.GRAY2_PACKED,
                PixelFormatFlag.GRAY2_PACKED,
                PixelFormat.GRAY2_PACKED,
                0,
                4,
                4,
                2,
                3,
                0,
            ),
            bytes(32),
            bytes(32),
        ]
    )


def _layout_profile(profile: DisplayProfile) -> bytes:
    return struct.pack(
        "<HHHHHHHHHHHHBB2sHHI32s32s",
        profile.logical_width,
        profile.logical_height,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        profile.logical_width,
        profile.logical_height,
        1,
        1,
        bytes(2),
        3,
        0,
        0,
        bytes(32),
        bytes(32),
    )


def _reader_requirements(profile: DisplayProfile) -> bytes:
    feature_flags = (1 << 0) | (1 << 3)
    required_features = (1 << 0) | (1 << 2) | (1 << 3) | (1 << 4)
    return struct.pack(
        "<QQIHHIHHII36s",
        feature_flags,
        required_features,
        profile.storage_pixel_format_flag,
        4,
        1,
        1 << CompressionMethod.RLE_PACKBITS,
        profile.logical_width,
        profile.logical_height,
        96000,
        96000 * 2,
        bytes(36),
    )


def _source_identity(source: SourceInfo, filename: StringRef, package_identifier: StringRef) -> bytes:
    return b"".join(
        [
            struct.pack("<HHQ", source.source_type, 0, source.file_size),
            source.md5[:16].ljust(16, b"\0"),
            source.sha256[:32].ljust(32, b"\0"),
            filename.pack(),
            package_identifier.pack(),
            bytes(32),
        ]
    )


def _book_metadata(title: StringRef, author: StringRef, language: StringRef) -> bytes:
    return b"".join(
        [
            title.pack(),
            StringRef().pack(),
            author.pack(),
            StringRef().pack(),
            language.pack(),
            StringRef().pack(),
            struct.pack("<II", 0, 0),
            bytes(32),
        ]
    )


def _rendition_identity(compiler_name: StringRef, compiler_version: StringRef) -> bytes:
    return b"".join([bytes(32 * 8), compiler_name.pack(), compiler_version.pack(), struct.pack("<Q", 0), bytes(32)])


def _font_policy(font_info: FontInfo, font_name: StringRef, font_path: StringRef, renderer: StringRef) -> bytes:
    font_mode_force = 2
    force_custom_font = 1 << 0
    return b"".join(
        [
            struct.pack("<HH", font_mode_force, force_custom_font),
            font_info.sha256,
            font_name.pack(),
            font_path.pack(),
            renderer.pack(),
            bytes(32),
            bytes(32),
        ]
    )


def _typography_policy() -> bytes:
    return b"".join(
        [
            struct.pack("<HHHHIIHHiiBBBB", 24, 18, 0, 400, 1000, 1250, 0, 8, 0, 0, 1, 1, 1, 0),
            StringRef().pack(),
            struct.pack("<I", 0),
            bytes(32),
            bytes(32),
        ]
    )


def _image_policy() -> bytes:
    return struct.pack("<HHHHHHHHHHHHI32s32s", 1, PixelFormat.GRAY2_PACKED, 1, 1, 1, 3, 3, 1000, 0, 0, 0, 0, 0, bytes(32), bytes(32))


def _compression_policy() -> bytes:
    return struct.pack("<HIHHI32s32s", CompressionMethod.RLE_PACKBITS, 1 << CompressionMethod.RLE_PACKBITS, 1, 0, 0, bytes(32), bytes(32))


def _chrome_policy() -> bytes:
    return struct.pack("<4sHHI32s32s", bytes(4), 0, 0, 0, bytes(32), bytes(32))


def _align_up(value: int, alignment: int) -> int:
    return ((value + alignment - 1) // alignment) * alignment
