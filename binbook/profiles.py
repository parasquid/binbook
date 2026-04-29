from __future__ import annotations

from dataclasses import dataclass, replace

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
    supported_storage_pixel_format_flags: int
    logical_orientation: int
    logical_to_physical_rotation: int
    scan_order_hint: int
    grayscale_levels: int
    framebuffer_bits_per_pixel: int


XTEINK_X4_PORTRAIT = DisplayProfile(
    name="xteink-x4-portrait",
    logical_width=480,
    logical_height=800,
    physical_width=800,
    physical_height=480,
    storage_pixel_format=PixelFormat.GRAY2_PACKED,
    storage_pixel_format_flag=PixelFormatFlag.GRAY2_PACKED,
    supported_storage_pixel_format_flags=int(PixelFormatFlag.GRAY1_PACKED | PixelFormatFlag.GRAY2_PACKED),
    logical_orientation=1,
    logical_to_physical_rotation=90,
    scan_order_hint=1,
    grayscale_levels=4,
    framebuffer_bits_per_pixel=2,
)


def get_profile(name: str, storage_pixel_format: str | PixelFormat | None = None) -> DisplayProfile:
    if name != XTEINK_X4_PORTRAIT.name:
        raise ValueError(f"unsupported profile: {name}")
    if storage_pixel_format is None:
        return XTEINK_X4_PORTRAIT
    pixel_format = _parse_storage_pixel_format(storage_pixel_format)
    if pixel_format == PixelFormat.GRAY2_PACKED:
        return XTEINK_X4_PORTRAIT
    if pixel_format == PixelFormat.GRAY1_PACKED:
        return replace(
            XTEINK_X4_PORTRAIT,
            storage_pixel_format=PixelFormat.GRAY1_PACKED,
            storage_pixel_format_flag=PixelFormatFlag.GRAY1_PACKED,
            grayscale_levels=2,
            framebuffer_bits_per_pixel=1,
        )
    raise ValueError(f"unsupported storage pixel format for {name}: {pixel_format.name}")


def _parse_storage_pixel_format(value: str | PixelFormat) -> PixelFormat:
    if isinstance(value, PixelFormat):
        return value
    normalized = value.lower().replace("-", "").replace("_", "")
    if normalized in {"gray1", "gray1packed", "1"}:
        return PixelFormat.GRAY1_PACKED
    if normalized in {"gray2", "gray2packed", "2"}:
        return PixelFormat.GRAY2_PACKED
    raise ValueError(f"unsupported storage pixel format: {value}")
