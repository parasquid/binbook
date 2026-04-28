from __future__ import annotations

from dataclasses import dataclass
from html.parser import HTMLParser
import hashlib
from pathlib import Path
import posixpath
import xml.etree.ElementTree as ET
import zipfile


CONTAINER_PATH = "META-INF/container.xml"
CONTAINER_NS = {"container": "urn:oasis:names:tc:opendocument:xmlns:container"}
OPF_NS = {"opf": "http://www.idpf.org/2007/opf", "dc": "http://purl.org/dc/elements/1.1/"}


@dataclass(frozen=True)
class EpubMetadata:
    title: str
    author: str
    language: str
    package_identifier: str


@dataclass(frozen=True)
class ManifestItem:
    item_id: str
    href: str
    media_type: str
    full_path: str
    properties: str = ""


@dataclass(frozen=True)
class SpineItem:
    index: int
    idref: str
    href: str
    media_type: str
    full_path: str
    html: str


@dataclass(frozen=True)
class RoughPage:
    source_spine_index: int
    href: str
    text: str


@dataclass(frozen=True)
class NavPoint:
    title: str
    href: str
    full_path: str


@dataclass(frozen=True)
class EpubBook:
    path: Path
    file_size: int
    md5: bytes
    sha256: bytes
    metadata: EpubMetadata
    manifest: dict[str, ManifestItem]
    spine: list[SpineItem]
    nav_points: list[NavPoint]

    def rough_page_sequence(self) -> list[RoughPage]:
        return [
            RoughPage(source_spine_index=item.index, href=item.href, text=extract_text(item.html))
            for item in self.spine
        ]


def read_epub(path: Path | str) -> EpubBook:
    epub_path = Path(path)
    data = epub_path.read_bytes()
    with zipfile.ZipFile(epub_path) as archive:
        opf_path = _rootfile_path(archive)
        opf = ET.fromstring(archive.read(opf_path))
        opf_dir = posixpath.dirname(opf_path)
        metadata = _metadata(opf)
        manifest = _manifest(opf, opf_dir)
        spine = _spine(archive, opf, manifest)
        nav_points = _nav_points(archive, manifest)
    return EpubBook(
        path=epub_path,
        file_size=len(data),
        md5=hashlib.md5(data).digest(),
        sha256=hashlib.sha256(data).digest(),
        metadata=metadata,
        manifest=manifest,
        spine=spine,
        nav_points=nav_points,
    )


def extract_text(html: str) -> str:
    parser = _TextExtractor()
    parser.feed(html)
    return " ".join(" ".join(parser.parts).split())


def _rootfile_path(archive: zipfile.ZipFile) -> str:
    try:
        container = ET.fromstring(archive.read(CONTAINER_PATH))
    except KeyError as exc:
        raise ValueError("EPUB is missing META-INF/container.xml") from exc
    rootfile = container.find(".//container:rootfile", CONTAINER_NS)
    if rootfile is None:
        raise ValueError("EPUB container does not declare a rootfile")
    full_path = rootfile.attrib.get("full-path", "")
    if not full_path:
        raise ValueError("EPUB rootfile is missing full-path")
    return full_path


def _metadata(opf: ET.Element) -> EpubMetadata:
    unique_id = opf.attrib.get("unique-identifier", "")
    package_identifier = ""
    if unique_id:
        identifier = opf.find(f".//dc:identifier[@id='{unique_id}']", OPF_NS)
        if identifier is not None and identifier.text:
            package_identifier = identifier.text.strip()
    if not package_identifier:
        package_identifier = _text(opf.find(".//dc:identifier", OPF_NS))
    return EpubMetadata(
        title=_text(opf.find(".//dc:title", OPF_NS)),
        author=_text(opf.find(".//dc:creator", OPF_NS)),
        language=_text(opf.find(".//dc:language", OPF_NS)),
        package_identifier=package_identifier,
    )


def _manifest(opf: ET.Element, opf_dir: str) -> dict[str, ManifestItem]:
    items: dict[str, ManifestItem] = {}
    for element in opf.findall(".//opf:manifest/opf:item", OPF_NS):
        item_id = element.attrib.get("id", "")
        href = element.attrib.get("href", "")
        media_type = element.attrib.get("media-type", "")
        if not item_id or not href:
            continue
        full_path = posixpath.normpath(posixpath.join(opf_dir, href)) if opf_dir else href
        items[item_id] = ManifestItem(item_id, href, media_type, full_path, element.attrib.get("properties", ""))
    return items


def _spine(archive: zipfile.ZipFile, opf: ET.Element, manifest: dict[str, ManifestItem]) -> list[SpineItem]:
    spine: list[SpineItem] = []
    for index, itemref in enumerate(opf.findall(".//opf:spine/opf:itemref", OPF_NS)):
        idref = itemref.attrib.get("idref", "")
        item = manifest.get(idref)
        if item is None:
            raise ValueError(f"spine item references missing manifest item: {idref}")
        html = archive.read(item.full_path).decode("utf-8", errors="replace")
        spine.append(
            SpineItem(
                index=index,
                idref=idref,
                href=item.href,
                media_type=item.media_type,
                full_path=item.full_path,
                html=html,
            )
        )
    return spine


def _nav_points(archive: zipfile.ZipFile, manifest: dict[str, ManifestItem]) -> list[NavPoint]:
    nav_item = next((item for item in manifest.values() if "nav" in item.properties.split()), None)
    if nav_item is None:
        return []
    html = archive.read(nav_item.full_path).decode("utf-8", errors="replace")
    parser = _NavExtractor(posixpath.dirname(nav_item.full_path))
    parser.feed(html)
    return parser.points


def _text(element: ET.Element | None) -> str:
    if element is None or element.text is None:
        return ""
    return element.text.strip()


class _TextExtractor(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.parts: list[str] = []

    def handle_data(self, data: str) -> None:
        stripped = data.strip()
        if stripped:
            self.parts.append(stripped)


class _NavExtractor(HTMLParser):
    def __init__(self, nav_dir: str) -> None:
        super().__init__()
        self.nav_dir = nav_dir
        self.points: list[NavPoint] = []
        self._href: str | None = None
        self._parts: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag.lower() == "a":
            attrs_dict = dict(attrs)
            href = attrs_dict.get("href")
            if href:
                self._href = href.split("#", 1)[0]
                self._parts = []

    def handle_data(self, data: str) -> None:
        if self._href is not None and data.strip():
            self._parts.append(data.strip())

    def handle_endtag(self, tag: str) -> None:
        if tag.lower() == "a" and self._href is not None:
            title = " ".join(" ".join(self._parts).split())
            full_path = posixpath.normpath(posixpath.join(self.nav_dir, self._href))
            if title:
                self.points.append(NavPoint(title, self._href, full_path))
            self._href = None
            self._parts = []
