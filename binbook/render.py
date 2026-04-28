from __future__ import annotations

from dataclasses import dataclass
from html.parser import HTMLParser
from pathlib import Path
import posixpath
import zipfile

from PIL import Image, ImageDraw, ImageFont

from .checksums import crc32
from .constants import PageKind, SourceType, UINT32_MAX
from .epub import EpubBook, read_epub
from .images import image_bytes_to_gray2_packed, pil_image_to_gray2_packed
from .profiles import DisplayProfile, get_profile
from .rle import encode_packbits
from .writer import BookInfo, EncodedPage, NavEntry, SourceInfo, build_binbook


@dataclass(frozen=True)
class FlowItem:
    kind: str
    value: str
    source_spine_index: int
    source_full_path: str


def encode_epub(input_epub: Path, output: Path, profile_name: str = "xteink-x4-portrait") -> None:
    profile = get_profile(profile_name)
    book = read_epub(input_epub)
    pages, spine_first_page = _compile_pages(book, profile)
    nav_entries = _compile_nav_entries(book, spine_first_page)
    output.write_bytes(
        build_binbook(
            pages,
            profile,
            source_name=input_epub.name,
            book_info=BookInfo(
                title=book.metadata.title,
                author=book.metadata.author,
                language=book.metadata.language,
                package_identifier=book.metadata.package_identifier,
            ),
            source_info=SourceInfo(
                source_type=SourceType.EPUB,
                filename=input_epub.name,
                file_size=book.file_size,
                md5=book.md5,
                sha256=book.sha256,
                package_identifier=book.metadata.package_identifier,
            ),
            nav_entries=nav_entries,
        )
    )


def _compile_pages(book: EpubBook, profile: DisplayProfile) -> tuple[list[EncodedPage], dict[int, int]]:
    pages: list[EncodedPage] = []
    spine_first_page: dict[int, int] = {}
    with zipfile.ZipFile(book.path) as archive:
        for item in book.spine:
            spine_first_page.setdefault(item.index, len(pages))
            for flow in _flow_items(item.html, item.index, item.full_path):
                if flow.kind == "text" and flow.value:
                    pages.extend(_text_pages(flow.value, profile, item.index))
                elif flow.kind == "image":
                    image_path = _resolve_image_path(flow.source_full_path, flow.value)
                    packed = image_bytes_to_gray2_packed(archive.read(image_path), profile)
                    pages.append(_encoded_page(packed, PageKind.IMAGE, item.index))
    if not pages:
        pages.append(_encoded_page(_render_text_to_packed("(empty book)", profile), PageKind.TEXT, UINT32_MAX))
    return pages, spine_first_page


def _compile_nav_entries(book: EpubBook, spine_first_page: dict[int, int]) -> list[NavEntry]:
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


def _text_pages(text: str, profile: DisplayProfile, spine_index: int) -> list[EncodedPage]:
    chunks = [text[i : i + 1800] for i in range(0, len(text), 1800)] or [text]
    return [_encoded_page(_render_text_to_packed(chunk, profile), PageKind.TEXT, spine_index) for chunk in chunks]


def _render_text_to_packed(text: str, profile: DisplayProfile) -> bytes:
    image = Image.new("L", (profile.logical_width, profile.logical_height), 255)
    draw = ImageDraw.Draw(image)
    font = _font(24)
    x = 24
    y = 24
    right = profile.logical_width - 24
    line_height = 32
    for paragraph in text.splitlines() or [text]:
        for line in _wrap_text_to_width(paragraph, draw, font, right - x) or [""]:
            if y + line_height > profile.logical_height - 24:
                break
            draw.text((x, y), line, fill=0, font=font)
            y += line_height
        y += 8
    return pil_image_to_gray2_packed(image, profile)


def _wrap_text_to_width(text: str, draw: ImageDraw.ImageDraw, font: ImageFont.ImageFont, max_width: int) -> list[str]:
    words = text.split()
    lines: list[str] = []
    current = ""
    for word in words:
        candidates = _split_word_to_width(word, draw, font, max_width)
        for candidate in candidates:
            proposed = candidate if not current else f"{current} {candidate}"
            if _text_width(draw, proposed, font) <= max_width:
                current = proposed
            else:
                if current:
                    lines.append(current)
                current = candidate
    if current:
        lines.append(current)
    return lines


def _split_word_to_width(word: str, draw: ImageDraw.ImageDraw, font: ImageFont.ImageFont, max_width: int) -> list[str]:
    if _text_width(draw, word, font) <= max_width:
        return [word]
    parts: list[str] = []
    current = ""
    for char in word:
        proposed = current + char
        if current and _text_width(draw, proposed, font) > max_width:
            parts.append(current)
            current = char
        else:
            current = proposed
    if current:
        parts.append(current)
    return parts


def _text_width(draw: ImageDraw.ImageDraw, text: str, font: ImageFont.ImageFont) -> int:
    bbox = draw.textbbox((0, 0), text, font=font)
    return bbox[2] - bbox[0]


def _font(size: int) -> ImageFont.ImageFont:
    for path in [
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/System/Library/Fonts/Supplemental/Helvetica.ttf",
        "/Library/Fonts/Arial.ttf",
    ]:
        try:
            return ImageFont.truetype(path, size)
        except OSError:
            pass
    return ImageFont.load_default()


def _encoded_page(packed: bytes, kind: int, spine_index: int) -> EncodedPage:
    compressed = encode_packbits(packed)
    return EncodedPage(
        compressed=compressed,
        uncompressed_size=len(packed),
        page_crc32=crc32(compressed),
        page_kind=kind,
        source_spine_index=spine_index,
    )


def _flow_items(html: str, spine_index: int, source_full_path: str) -> list[FlowItem]:
    parser = _FlowParser(spine_index, source_full_path)
    parser.feed(html)
    parser.close()
    return parser.items


def _resolve_image_path(source_full_path: str, src: str) -> str:
    return posixpath.normpath(posixpath.join(posixpath.dirname(source_full_path), src.split("#", 1)[0]))


class _FlowParser(HTMLParser):
    def __init__(self, spine_index: int, source_full_path: str) -> None:
        super().__init__()
        self.spine_index = spine_index
        self.source_full_path = source_full_path
        self.items: list[FlowItem] = []
        self._text_parts: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag.lower() == "img":
            self._flush_text()
            attrs_dict = dict(attrs)
            src = attrs_dict.get("src")
            if src:
                self.items.append(FlowItem("image", src, self.spine_index, self.source_full_path))

    def handle_data(self, data: str) -> None:
        stripped = data.strip()
        if stripped:
            self._text_parts.append(stripped)

    def handle_endtag(self, tag: str) -> None:
        return None

    def close(self) -> None:
        self._flush_text()
        super().close()

    def _flush_text(self) -> None:
        if self._text_parts:
            text = " ".join(" ".join(self._text_parts).split())
            self.items.append(FlowItem("text", text, self.spine_index, self.source_full_path))
            self._text_parts = []
