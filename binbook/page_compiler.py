from __future__ import annotations

import zipfile

from .checksums import crc32
from .constants import PageKind, UINT32_MAX
from .epub import EpubBook
from .epub_flow import flow_items, resolve_image_path
from .fonts import FontInfo
from .images import image_bytes_to_packed
from .pixels import gray2_packed_to_x4_native_planes, split_x4_plane_chunks
from .profiles import DisplayProfile
from .rle import encode_packbits
from .text_rendering import render_text_to_packed
from .writer import EncodedPage, EncodedPlane, NavEntry


def compile_pages(
    book: EpubBook,
    profile: DisplayProfile,
    font: FontInfo,
    *,
    dither: bool = True,
) -> tuple[list[EncodedPage], dict[int, int]]:
    pages: list[EncodedPage] = []
    spine_first_page: dict[int, int] = {}
    with zipfile.ZipFile(book.path) as archive:
        for item in book.spine:
            spine_first_page.setdefault(item.index, len(pages))
            for flow in flow_items(item.html, item.index, item.full_path):
                if flow.kind == "text" and flow.value:
                    pages.extend(text_pages(flow.value, profile, item.index, font))
                elif flow.kind == "image":
                    image_path = resolve_image_path(flow.source_full_path, flow.value)
                    packed = image_bytes_to_packed(
                        archive.read(image_path), profile, dither=dither
                    )
                    pages.append(encoded_page(packed, PageKind.IMAGE, item.index))
    if not pages:
        pages.append(
            encoded_page(
                render_text_to_packed("(empty book)", profile, font),
                PageKind.TEXT,
                UINT32_MAX,
            )
        )
    return pages, spine_first_page


def compile_nav_entries(
    book: EpubBook, spine_first_page: dict[int, int]
) -> list[NavEntry]:
    by_full_path = {item.full_path: item for item in book.spine}
    entries: list[NavEntry] = []
    for point in book.nav_points:
        spine = by_full_path.get(point.full_path)
        if spine is None:
            continue
        entries.append(
            NavEntry(
                title=point.title,
                target_page_number=spine_first_page.get(spine.index, 0),
                source_spine_index=spine.index,
            )
        )
    return entries


def text_pages(
    text: str, profile: DisplayProfile, spine_index: int, font: FontInfo
) -> list[EncodedPage]:
    chunks = [text[i : i + 1800] for i in range(0, len(text), 1800)] or [text]
    return [
        encoded_page(
            render_text_to_packed(chunk, profile, font), PageKind.TEXT, spine_index
        )
        for chunk in chunks
    ]


def encoded_page(packed: bytes, kind: int, spine_index: int) -> EncodedPage:
    overlay_msb, overlay_lsb, fast_base = gray2_packed_to_x4_native_planes(
        packed, 800, 480
    )
    planes: list[EncodedPlane] = []
    crc_parts: list[bytes] = []
    for slot, plane in enumerate((overlay_msb, overlay_lsb, fast_base)):
        chunks = tuple(encode_packbits(chunk) for chunk in split_x4_plane_chunks(plane))
        encoded = EncodedPlane(slot=slot, chunks=chunks, uncompressed_size=len(plane))
        planes.append(encoded)
        crc_parts.append(encoded.compressed)
    return EncodedPage(
        compressed=b"".join(crc_parts),
        uncompressed_size=sum(p.uncompressed_size for p in planes),
        page_crc32=crc32(b"".join(crc_parts)),
        page_kind=kind,
        source_spine_index=spine_index,
        planes=tuple(planes),
    )
