from __future__ import annotations

from ..constants import PixelFormat, PixelFormatFlag
from .base import DisplayProfile


PROFILE = DisplayProfile(
    name="xteink-x4-portrait",
    logical_width=480,
    logical_height=800,
    physical_width=800,
    physical_height=480,
    default_storage_pixel_format=PixelFormat.GRAY2_PACKED,
    storage_pixel_format=PixelFormat.GRAY2_PACKED,
    storage_pixel_format_flag=PixelFormatFlag.GRAY2_PACKED,
    supported_storage_pixel_format_flags=int(PixelFormatFlag.GRAY1_PACKED | PixelFormatFlag.GRAY2_PACKED),
    logical_orientation=1,
    logical_to_physical_rotation=90,
    scan_order_hint=1,
    grayscale_levels=4,
    framebuffer_bits_per_pixel=2,
)
