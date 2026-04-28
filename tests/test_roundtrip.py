from pathlib import Path

from PIL import Image

from binbook.cli import main
from binbook.constants import MAGIC
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
    assert reader.header.version_major == 0
    assert reader.header.version_minor == 1
    assert reader.header.page_data_offset % 65536 == 0
    assert len(reader.pages) == 2

    assert main(["inspect", str(book), "--validate"]) == 0
    assert "Validation: OK" in capsys.readouterr().out

    assert main(["decode", str(book), "--page", "0", "-o", str(decoded)]) == 0
    image = Image.open(decoded)
    assert image.size == (480, 800)
    assert image.getpixel((0, 0)) == 0
