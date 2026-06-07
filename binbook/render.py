from __future__ import annotations

from pathlib import Path

from .constants import DitherMethod, SourceType
from .epub import read_epub
from .fonts import get_font
from .page_compiler import compile_nav_entries, compile_pages
from .profiles import get_profile
from .writer import BookInfo, SourceInfo, build_binbook


def encode_epub(
    input_epub: Path,
    output: Path,
    profile_name: str = "xteink-x4-portrait",
    font_family: str = "literata",
    storage_pixel_format: str | None = None,
    *,
    dither: bool = True,
) -> None:
    profile = get_profile(profile_name).resolve(storage_pixel_format)
    font = get_font(font_family)
    book = read_epub(input_epub)
    pages, spine_first_page = compile_pages(book, profile, font, dither=dither)
    nav_entries = compile_nav_entries(book, spine_first_page)
    dither_method = DitherMethod.FLOYD_STEINBERG if dither else DitherMethod.NONE
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
            font_info=font,
            character_spacing_milli_em=font.default_character_spacing_milli_em,
            dither_method=dither_method,
        )
    )
