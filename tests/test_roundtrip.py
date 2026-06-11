from pathlib import Path
import struct

from PIL import Image

from binbook.cli import main
from binbook.constants import MAGIC, PixelFormat, SectionId
from binbook.pixels import unpack_gray2
from binbook.reader import BinBookReader


def test_png_folder_can_encode_inspect_and_decode(tmp_path: Path, capsys):
    pages = tmp_path / "pages"
    pages.mkdir()
    Image.new("L", (480, 800), 0).save(pages / "001.png")
    Image.new("L", (480, 800), 255).save(pages / "002.png")
    book = tmp_path / "test.binbook"
    decoded = tmp_path / "decoded.png"

    assert main(["encode-png-folder", str(pages), "-o", str(book)]) == 0
    raw = book.read_bytes()
    assert raw[:8] == MAGIC

    reader = BinBookReader.open(book)
    assert reader.header.page_data_offset < 4096
    assert reader.header.page_data_offset % 16 == 0
    assert len(reader.pages) == 2
    assert len(reader.chapters) == 0
    assert {page.pixel_format for page in reader.pages} == {PixelFormat.GRAY2_PACKED}
    assert {page.uncompressed_size for page in reader.pages} == {96_000}

    assert main(["inspect", str(book), "--validate"]) == 0
    assert "Validation: OK" in capsys.readouterr().out

    assert main(["decode", str(book), "--page", "0", "-o", str(decoded)]) == 0
    image = Image.open(decoded)
    assert image.size == (800, 480)
    assert image.getpixel((0, 0)) == 0


def test_png_folder_stores_portrait_profile_pages_in_native_physical_orientation(tmp_path: Path):
    pages = tmp_path / "pages"
    pages.mkdir()
    image = Image.new("L", (480, 800), 255)
    image.putpixel((10, 20), 0)
    image.save(pages / "001.png")
    book = tmp_path / "native.binbook"

    assert main(["encode-png-folder", str(pages), "-o", str(book), "--no-dither"]) == 0

    reader = BinBookReader.open(book)
    page = reader.pages[0]
    assert (page.stored_width, page.stored_height) == (800, 480)

    packed, page = reader.decode_page_bytes(0)
    native = unpack_gray2(packed, page.stored_width, page.stored_height)
    assert native[10 * 800 + (800 - 1 - 20)] == 0


def test_png_folder_can_encode_x4_gray1_when_requested(tmp_path: Path):
    pages = tmp_path / "pages"
    pages.mkdir()
    Image.new("L", (480, 800), 255).save(pages / "001.png")
    book = tmp_path / "gray1.binbook"

    assert main(["encode-png-folder", str(pages), "-o", str(book), "--pixel-format", "gray1"]) == 0

    reader = BinBookReader.open(book)
    assert {page.pixel_format for page in reader.pages} == {PixelFormat.GRAY1_PACKED}
    assert {page.uncompressed_size for page in reader.pages} == {48_000}


def test_png_folder_records_floyd_steinberg_dither_by_default(tmp_path: Path):
    pages = tmp_path / "pages"
    pages.mkdir()
    Image.new("L", (480, 800), 127).save(pages / "001.png")
    book = tmp_path / "dithered.binbook"

    assert main(["encode-png-folder", str(pages), "-o", str(book)]) == 0

    reader = BinBookReader.open(book)
    image_policy = reader._section_data(SectionId.IMAGE_POLICY)
    assert struct.unpack_from("<H", image_policy, 8)[0] == 1


def test_png_folder_no_dither_records_none_and_uses_threshold_quantization(tmp_path: Path):
    pages = tmp_path / "pages"
    pages.mkdir()
    Image.new("L", (480, 800), 127).save(pages / "001.png")
    book = tmp_path / "threshold.binbook"
    decoded = tmp_path / "decoded.png"

    assert main(["encode-png-folder", str(pages), "-o", str(book), "--no-dither"]) == 0

    reader = BinBookReader.open(book)
    image_policy = reader._section_data(SectionId.IMAGE_POLICY)
    assert struct.unpack_from("<H", image_policy, 8)[0] == 0

    assert main(["decode", str(book), "--page", "0", "-o", str(decoded)]) == 0
    image = Image.open(decoded)
    assert image.getpixel((0, 0)) == 85
