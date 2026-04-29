from __future__ import annotations

from dataclasses import dataclass, replace
import hashlib
import json
from types import MappingProxyType
from typing import Mapping
from pathlib import Path


FONT_ASSET_DIR = Path(__file__).resolve().parent / "assets" / "fonts"
FONT_KERNING_DIR = Path(__file__).resolve().parent / "font_kerning"
PairKerningTable = Mapping[tuple[str, str], int]


@dataclass(frozen=True)
class FontInfo:
    family: str
    display_name: str
    path: Path
    stable_path: str
    default_character_spacing_milli_em: int = 0
    pair_kerning_milli_em: PairKerningTable = MappingProxyType({})

    @property
    def sha256(self) -> bytes:
        return hashlib.sha256(self.path.read_bytes()).digest()


def load_pair_kerning_table(path: Path) -> PairKerningTable:
    if not path.exists():
        return MappingProxyType({})
    try:
        payload = json.loads(path.read_text())
    except json.JSONDecodeError as exc:
        raise ValueError(f"invalid kerning JSON in {path}") from exc
    if not isinstance(payload, dict):
        raise ValueError(f"kerning JSON must be an object: {path}")

    table: dict[tuple[str, str], int] = {}
    for pair, value in payload.items():
        if not isinstance(pair, str) or len(pair) != 2:
            raise ValueError(f"kerning pair keys must be two-character strings: {path}")
        if not isinstance(value, int):
            raise ValueError(f"kerning pair values must be integers: {path}")
        table[(pair[0], pair[1])] = value
    return MappingProxyType(table)


FONTS = {
    "literata": FontInfo(
        family="literata",
        display_name="Literata",
        path=FONT_ASSET_DIR / "Literata" / "Literata.ttf",
        stable_path="binbook/assets/fonts/Literata/Literata.ttf",
    ),
    "opendyslexic": FontInfo(
        family="opendyslexic",
        display_name="OpenDyslexic",
        path=FONT_ASSET_DIR / "OpenDyslexic" / "OpenDyslexic-Regular.otf",
        stable_path="binbook/assets/fonts/OpenDyslexic/OpenDyslexic-Regular.otf",
        default_character_spacing_milli_em=-160,
        pair_kerning_milli_em=load_pair_kerning_table(FONT_KERNING_DIR / "opendyslexic.json"),
    ),
    "sans-serif": FontInfo(
        family="sans-serif",
        display_name="OpenDyslexic",
        path=FONT_ASSET_DIR / "OpenDyslexic" / "OpenDyslexic-Regular.otf",
        stable_path="binbook/assets/fonts/OpenDyslexic/OpenDyslexic-Regular.otf",
        default_character_spacing_milli_em=-160,
    ),
}


def get_font(family: str) -> FontInfo:
    key = family.lower()
    try:
        font_info = FONTS[key]
    except KeyError as exc:
        raise ValueError(f"unsupported font family: {family}") from exc
    return replace(
        font_info,
        pair_kerning_milli_em=load_pair_kerning_table(FONT_KERNING_DIR / f"{font_info.family}.json"),
    )


def available_font_families() -> list[str]:
    return sorted(FONTS)
