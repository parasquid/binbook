from __future__ import annotations

from pathlib import Path
import subprocess
import tomllib
import zipfile

import pytest

from binbook.cli import main
from binbook.constants import SectionId
from binbook.reader import BinBookReader
from binbook.viewer import render_page_image


def test_support_entrypoint_and_help_expose_only_support_commands(
    capsys: pytest.CaptureFixture[str],
) -> None:
    scripts = tomllib.loads(Path("pyproject.toml").read_text())["project"]["scripts"]
    assert scripts == {"binbook-support": "binbook.cli:main"}
    with pytest.raises(SystemExit) as caught:
        main(["--help"])
    assert caught.value.code == 0
    help_text = capsys.readouterr().out
    assert "view" in help_text
    assert "kerning-proof" in help_text
    for removed in ("encode", "encode-png-folder", "decode", "inspect"):
        assert removed not in help_text


def test_viewer_renders_rust_generated_book_with_font_resource_index(
    tmp_path: Path,
) -> None:
    source = tmp_path / "book.epub"
    output = tmp_path / "page.binbook"
    with zipfile.ZipFile(source, "w") as archive:
        archive.writestr(
            "mimetype", "application/epub+zip", compress_type=zipfile.ZIP_STORED
        )
        archive.writestr(
            "META-INF/container.xml",
            '<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" '
            'version="1.0"><rootfiles><rootfile full-path="OPS/package.opf" '
            'media-type="application/oebps-package+xml"/></rootfiles></container>',
        )
        archive.writestr(
            "OPS/package.opf",
            '<package xmlns="http://www.idpf.org/2007/opf" version="3.0" '
            'unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/">'
            '<dc:identifier id="id">support</dc:identifier><dc:title>Support</dc:title>'
            '<dc:language>en</dc:language></metadata><manifest><item id="chapter" '
            'href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest>'
            '<spine><itemref idref="chapter"/></spine></package>',
        )
        archive.writestr(
            "OPS/chapter.xhtml", "<html><body><p>Viewer proof.</p></body></html>"
        )
    subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "-p",
            "binbook",
            "--",
            "encode",
            str(source),
            "-o",
            str(output),
        ],
        check=True,
    )
    reader = BinBookReader.open(output)
    assert reader.sections[SectionId.FONT_RESOURCE_INDEX].record_count == 1
    assert len(reader.fonts) == 1
    image = render_page_image(reader, 0, show_chrome=False)
    assert image.size == (480, 800)
