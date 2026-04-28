from __future__ import annotations

from dataclasses import dataclass
import hashlib
from pathlib import Path


FONT_ASSET_DIR = Path(__file__).resolve().parent / "assets" / "fonts"


@dataclass(frozen=True)
class FontInfo:
    family: str
    display_name: str
    path: Path
    stable_path: str

    @property
    def sha256(self) -> bytes:
        return hashlib.sha256(self.path.read_bytes()).digest()


FONTS = {
    "literata": FontInfo(
        family="literata",
        display_name="Literata",
        path=FONT_ASSET_DIR / "Literata" / "Literata.ttf",
        stable_path="binbook/assets/fonts/Literata/Literata.ttf",
    ),
}


def get_font(family: str) -> FontInfo:
    key = family.lower()
    try:
        return FONTS[key]
    except KeyError as exc:
        raise ValueError(f"unsupported font family: {family}") from exc


def available_font_families() -> list[str]:
    return sorted(FONTS)
