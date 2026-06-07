from __future__ import annotations

from dataclasses import dataclass
from html.parser import HTMLParser
import posixpath


@dataclass(frozen=True)
class FlowItem:
    kind: str
    value: str
    source_spine_index: int
    source_full_path: str


def flow_items(html: str, spine_index: int, source_full_path: str) -> list[FlowItem]:
    parser = _FlowParser(spine_index, source_full_path)
    parser.feed(html)
    parser.close()
    return parser.items


def resolve_image_path(source_full_path: str, src: str) -> str:
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
