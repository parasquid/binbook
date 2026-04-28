from __future__ import annotations

from pathlib import Path

from PIL import Image

from binbook.cli import main
from binbook.reader import BinBookReader
from binbook.viewer import ViewerState, image_to_surface, render_page_image
from binbook.writer import encode_png_folder


def test_viewer_state_navigates_and_clamps_page_numbers():
    state = ViewerState(page_count=3)

    assert state.current_page == 0
    assert state.next_page() == 1
    assert state.next_page() == 2
    assert state.next_page() == 2
    assert state.previous_page() == 1
    assert state.jump_to_page(99) == 2
    assert state.jump_to_page(-5) == 0


def test_render_page_image_returns_display_sized_image_with_optional_overlay(tmp_path: Path):
    book_path = _sample_book(tmp_path)
    reader = BinBookReader.open(book_path)

    image = render_page_image(reader, 0, show_chrome=True, debug_content_box=True)

    assert image.size == (480, 800)
    assert image.mode == "RGB"
    assert image.getbbox() is not None


def test_image_to_surface_preserves_dimensions(tmp_path: Path):
    book_path = _sample_book(tmp_path)
    reader = BinBookReader.open(book_path)
    image = render_page_image(reader, 0)

    surface = image_to_surface(image)

    assert surface.get_size() == (480, 800)


def test_cli_exposes_view_command(capsys):
    try:
        main(["view", "--help"])
    except SystemExit as exc:
        assert exc.code == 0

    out = capsys.readouterr().out
    assert "simulate a BinBook file" in out
    assert "--debug-content-box" in out


def _sample_book(tmp_path: Path) -> Path:
    pages = tmp_path / "pages"
    pages.mkdir()
    Image.new("L", (480, 800), 0).save(pages / "001.png")
    book = tmp_path / "viewer.binbook"
    encode_png_folder(pages, book)
    return book
