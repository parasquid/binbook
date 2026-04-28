from __future__ import annotations

from dataclasses import dataclass

from .constants import PixelFormat, PixelFormatFlag


@dataclass(frozen=True)
class DisplayProfile:
    name: str
    logical_width: int
    logical_height: int
    physical_width: int
    physical_height: int
    storage_pixel_format: PixelFormat
    storage_pixel_format_flag: PixelFormatFlag


XTEINK_X4_PORTRAIT = DisplayProfile(
    name="xteink-x4-portrait",
    logical_width=480,
    logical_height=800,
    physical_width=800,
    physical_height=480,
    storage_pixel_format=PixelFormat.GRAY2_PACKED,
    storage_pixel_format_flag=PixelFormatFlag.GRAY2_PACKED,
)


def get_profile(name: str) -> DisplayProfile:
    if name != XTEINK_X4_PORTRAIT.name:
        raise ValueError(f"unsupported profile: {name}")
    return XTEINK_X4_PORTRAIT
