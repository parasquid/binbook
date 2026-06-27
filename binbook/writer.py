from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import struct

from .checksums import crc32
from .constants import (
    UINT32_MAX,
    CompressionMethod,
    DitherMethod,
    PageKind,
    PixelFormat,
    PixelFormatFlag,
    SectionId,
    SourceType,
)
from .fonts import FontInfo, get_font
from .images import png_to_packed
from .profiles import DisplayProfile, get_profile
from .rle import encode_packbits
from .sections import (
    DisplayProfileSection,
    LayoutProfileSection,
    ReaderRequirementsSection,
)
from .strings import StringTableBuilder
from .structs import (
    CHAPTER_INDEX_ENTRY_SIZE,
    HEADER_SIZE,
    NAV_INDEX_ENTRY_SIZE,
    PAGE_CHUNK_INDEX_ENTRY_SIZE,
    PAGE_INDEX_ENTRY_SIZE,
    PAGE_TRANSITION_INDEX_ENTRY_SIZE,
    SECTION_ENTRY_SIZE,
    BinBookHeader,
    ChapterIndexEntry,
    PageChunkIndexEntry,
    PageIndexEntry,
    PageTransitionIndexEntry,
    PlaneDir,
    SectionEntry,
    StringRef,
)


@dataclass(frozen=True)
class EncodedPlane:
    slot: int
    chunks: tuple[bytes, ...]
    uncompressed_size: int

    @property
    def compressed(self) -> bytes:
        return b"".join(self.chunks)


@dataclass(frozen=True)
class EncodedPage:
    compressed: bytes
    uncompressed_size: int
    page_crc32: int
    page_kind: int = PageKind.IMAGE
    source_spine_index: int = UINT32_MAX
    chapter_nav_index: int = UINT32_MAX
    planes: tuple[EncodedPlane, ...] = ()


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


def encode_png_folder(
    input_dir: Path,
    output: Path,
    profile_name: str = "xteink-x4-portrait",
    storage_pixel_format: str | PixelFormat | None = None,
    *,
    dither: bool = True,
) -> None:
    profile = get_profile(profile_name).resolve(storage_pixel_format)
    pngs = sorted(p for p in input_dir.iterdir() if p.suffix.lower() == ".png")
    if not pngs:
        raise ValueError("input folder contains no PNG files")

    pages: list[EncodedPage] = []
    for path in pngs:
        packed = png_to_packed(path, profile, dither=dither)
        compressed = encode_packbits(packed)
        pages.append(EncodedPage(compressed, len(packed), crc32(compressed)))

    dither_method = DitherMethod.FLOYD_STEINBERG if dither else DitherMethod.NONE
    output.write_bytes(
        build_binbook(
            pages, profile, source_name=input_dir.name, dither_method=dither_method
        )
    )


def build_binbook(
    pages: list[EncodedPage],
    profile: DisplayProfile,
    source_name: str = "png-folder",
    *,
    book_info: BookInfo | None = None,
    source_info: SourceInfo | None = None,
    nav_entries: list[NavEntry] | None = None,
    font_info: FontInfo | None = None,
    character_spacing_milli_em: int = 0,
    dither_method: int = DitherMethod.FLOYD_STEINBERG,
) -> bytes:
    book_info = book_info or BookInfo(title=source_name)
    source_info = source_info or SourceInfo(filename=source_name)
    nav_entries = nav_entries or []
    font_info = font_info or get_font("literata")
    strings = StringTableBuilder()
    refs = {
        "profile": strings.add(profile.name),
        "family": strings.add(profile.family),
        "model": strings.add(profile.model),
        "title": strings.add(book_info.title or source_name),
        "author": strings.add(book_info.author),
        "language": strings.add(book_info.language),
        "package_identifier": strings.add(
            book_info.package_identifier or source_info.package_identifier
        ),
        "filename": strings.add(source_info.filename or source_name),
        "compiler": strings.add("binbook-poc"),
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
        (
            SectionId.SOURCE_IDENTITY,
            _source_identity(source_info, refs["filename"], refs["package_identifier"]),
            0,
            0,
        ),
        (
            SectionId.BOOK_METADATA,
            _book_metadata(refs["title"], refs["author"], refs["language"]),
            0,
            0,
        ),
        (SectionId.RENDITION_IDENTITY, _rendition_identity(refs["compiler"]), 0, 0),
        (
            SectionId.FONT_POLICY,
            _font_policy(
                font_info, refs["font_name"], refs["font_path"], refs["renderer"]
            ),
            0,
            0,
        ),
        (
            SectionId.TYPOGRAPHY_POLICY,
            _typography_policy(character_spacing_milli_em),
            0,
            0,
        ),
        (SectionId.IMAGE_POLICY, _image_policy(profile, dither_method), 0, 0),
        (SectionId.COMPRESSION_POLICY, _compression_policy(), 0, 0),
        (SectionId.CHROME_POLICY, _chrome_policy(), 0, 0),
    ]

    page_index = _page_index(pages, profile)
    nav_index = _nav_index(nav_entries, nav_title_refs)
    chapter_index = _chapter_index(nav_entries, nav_title_refs)
    chunk_index = _chunk_index(pages)
    transition_index = _transition_index(pages)
    sections.extend(
        [
            (SectionId.PAGE_INDEX, page_index, PAGE_INDEX_ENTRY_SIZE, len(pages)),
            (SectionId.NAV_INDEX, nav_index, NAV_INDEX_ENTRY_SIZE, len(nav_entries)),
            (
                SectionId.CHAPTER_INDEX,
                chapter_index,
                CHAPTER_INDEX_ENTRY_SIZE,
                len(_chapter_entries(nav_entries)),
            ),
            (
                SectionId.PAGE_CHUNK_INDEX,
                chunk_index,
                PAGE_CHUNK_INDEX_ENTRY_SIZE,
                len(chunk_index) // PAGE_CHUNK_INDEX_ENTRY_SIZE,
            ),
            (
                SectionId.PAGE_TRANSITION_INDEX,
                transition_index,
                PAGE_TRANSITION_INDEX_ENTRY_SIZE,
                len(transition_index) // PAGE_TRANSITION_INDEX_ENTRY_SIZE
                if transition_index
                else 0,
            ),
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
        section_entries.append(
            SectionEntry(
                section_id,
                cursor,
                len(data),
                entry_size,
                record_count,
                crc32(data) if data else 0,
            )
        )
        data_parts.append(data)
        cursor += len(data)

    page_data_offset = _align_up(cursor, profile.page_data_alignment)
    page_data = b"".join(page.compressed for page in pages)
    section_entries.append(
        SectionEntry(
            SectionId.PAGE_DATA,
            page_data_offset,
            len(page_data),
            0,
            0,
            crc32(page_data),
        )
    )

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
    padding = bytes(
        page_data_offset - (HEADER_SIZE + len(section_table) + len(metadata))
    )
    return header.pack() + section_table + metadata + padding + page_data


def _page_index(pages: list[EncodedPage], profile: DisplayProfile) -> bytes:
    out = bytearray()
    total = len(pages)
    blob_offset = 0
    for index, page in enumerate(pages):
        start = int(index * 1_000_000 / total)
        end = int((index + 1) * 1_000_000 / total)
        if page.planes:
            bitmap = 0
            compression = [0, 0, 0, 0]
            offsets = [0, 0, 0, 0]
            sizes = [0, 0, 0, 0]
            for plane in page.planes:
                bitmap |= 1 << plane.slot
                compression[plane.slot] = CompressionMethod.RLE_PACKBITS
                offsets[plane.slot] = blob_offset
                sizes[plane.slot] = len(plane.compressed)
                blob_offset += len(plane.compressed)
            plane_dir = PlaneDir(
                bitmap=bitmap,
                compression=compression,
                offsets=offsets,
                sizes=sizes,
            )
        else:
            plane_dir = PlaneDir(
                bitmap=0x01,
                compression=[CompressionMethod.RLE_PACKBITS, 0, 0, 0],
                offsets=[blob_offset, 0, 0, 0],
                sizes=[len(page.compressed), 0, 0, 0],
            )
            blob_offset += len(page.compressed)
        out.extend(
            PageIndexEntry(
                page_number=index,
                page_kind=page.page_kind,
                pixel_format=profile.storage_pixel_format,
                compression_method=CompressionMethod.RLE_PACKBITS,
                page_crc32=page.page_crc32,
                stored_width=profile.storage_width,
                stored_height=profile.storage_height,
                plane_dir=plane_dir,
                source_spine_index=page.source_spine_index,
                chapter_nav_index=page.chapter_nav_index,
                progress_start_ppm=start,
                progress_end_ppm=end,
            ).pack()
        )
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


def _chapter_entries(entries: list[NavEntry]) -> list[tuple[int, NavEntry]]:
    return [
        (nav_index, entry)
        for nav_index, entry in enumerate(entries)
        if entry.nav_type in (3, 4)
    ]


def _chapter_index(entries: list[NavEntry], title_refs: list[StringRef]) -> bytes:
    out = bytearray()
    for chapter_index, (nav_index, entry) in enumerate(_chapter_entries(entries)):
        out.extend(
            ChapterIndexEntry(
                chapter_index=chapter_index,
                nav_index=nav_index,
                title=title_refs[nav_index],
                target_page_number=entry.target_page_number,
                level=entry.level,
                nav_type=entry.nav_type,
                source_spine_index=entry.source_spine_index,
            ).pack()
        )
    return bytes(out)


def _chunk_index(pages: list[EncodedPage]) -> bytes:
    from .pixels import X4_CHUNK_ROWS

    out = bytearray()
    global_offset = 0
    for page_index, page in enumerate(pages):
        if not page.planes:
            continue
        page_start = global_offset
        for plane in page.planes:
            chunk_offset = page_start
            for chunk_index, chunk_data in enumerate(plane.chunks):
                out.extend(
                    PageChunkIndexEntry(
                        page_number=page_index,
                        plane_slot=plane.slot,
                        chunk_index=chunk_index,
                        row_start=chunk_index * X4_CHUNK_ROWS,
                        row_count=X4_CHUNK_ROWS,
                        page_data_offset=chunk_offset,
                        compressed_size=len(chunk_data),
                        uncompressed_size=1600,
                    ).pack()
                )
                chunk_offset += len(chunk_data)
            page_start = chunk_offset
        global_offset = page_start
    return bytes(out)


def _transition_index(pages: list[EncodedPage]) -> bytes:
    if len(pages) < 2:
        return b""
    out = bytearray()
    for i in range(len(pages) - 1):
        for from_page, to_page in [(i, i + 1), (i + 1, i)]:
            mask, first, count = _compare_bw_chunks(pages[from_page], pages[to_page])
            out.extend(
                PageTransitionIndexEntry(
                    from_page_number=from_page,
                    to_page_number=to_page,
                    changed_chunk_mask=mask,
                    first_changed_chunk=first,
                    changed_chunk_count=count,
                ).pack()
            )
    return bytes(out)


def _compare_bw_chunks(
    from_page: EncodedPage, to_page: EncodedPage
) -> tuple[int, int, int]:
    from .rle import decode_packbits
    from .pixels import X4_CHUNK_ROWS, X4_ROW_BYTES

    from_plane = next((p for p in from_page.planes if p.slot == 2), None)
    to_plane = next((p for p in to_page.planes if p.slot == 2), None)
    if from_plane is None or to_plane is None:
        return 0, 0, 0
    mask = 0
    changed_chunks: list[int] = []
    for chunk_idx in range(len(from_plane.chunks)):
        from_data = decode_packbits(from_plane.chunks[chunk_idx])
        to_data = decode_packbits(to_plane.chunks[chunk_idx])
        if from_data != to_data:
            mask |= 1 << chunk_idx
            changed_chunks.append(chunk_idx)
    if not changed_chunks:
        return 0, 0, 0
    first = changed_chunks[0]
    last = changed_chunks[-1]
    return mask, first, last - first + 1


def _display_profile(profile: DisplayProfile, refs: dict[str, StringRef]) -> bytes:
    return DisplayProfileSection.from_profile(
        profile,
        profile_ref=refs["profile"],
        family=refs["family"],
        model=refs["model"],
    ).pack()


def _layout_profile(profile: DisplayProfile) -> bytes:
    return LayoutProfileSection.from_profile(profile).pack()


def _reader_requirements(profile: DisplayProfile) -> bytes:
    return ReaderRequirementsSection.from_profile(profile).pack()


def _source_identity(
    source: SourceInfo, filename: StringRef, package_identifier: StringRef
) -> bytes:
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


def _rendition_identity(compiler_name: StringRef) -> bytes:
    return b"".join(
        [
            bytes(32 * 8),
            compiler_name.pack(),
            StringRef().pack(),
            struct.pack("<Q", 0),
            bytes(32),
        ]
    )


def _font_policy(
    font_info: FontInfo, font_name: StringRef, font_path: StringRef, renderer: StringRef
) -> bytes:
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


def _typography_policy(character_spacing_milli_em: int = 0) -> bytes:
    return b"".join(
        [
            struct.pack(
                "<HHHHIIHHiiBBBB",
                24,
                18,
                0,
                400,
                1000,
                1250,
                0,
                8,
                character_spacing_milli_em,
                0,
                1,
                1,
                1,
                0,
            ),
            StringRef().pack(),
            struct.pack("<I", 0),
            bytes(32),
            bytes(32),
        ]
    )


def _image_policy(
    profile: DisplayProfile, dither_method: int = DitherMethod.FLOYD_STEINBERG
) -> bytes:
    white_value = profile.grayscale_levels - 1
    return struct.pack(
        "<HHHHHHHHHHHHI32s32s",
        1,
        profile.storage_pixel_format,
        1,
        1,
        dither_method,
        1,
        white_value,
        1000,
        0,
        0,
        0,
        0,
        0,
        bytes(32),
        bytes(32),
    )


def _compression_policy() -> bytes:
    return struct.pack(
        "<HIHHI32s32s",
        CompressionMethod.RLE_PACKBITS,
        1 << CompressionMethod.RLE_PACKBITS,
        1,
        0,
        0,
        bytes(32),
        bytes(32),
    )


def _chrome_policy() -> bytes:
    return struct.pack("<4sHHI32s32s", bytes(4), 0, 0, 0, bytes(32), bytes(32))


def _align_up(value: int, alignment: int) -> int:
    return ((value + alignment - 1) // alignment) * alignment
