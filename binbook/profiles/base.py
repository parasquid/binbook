from __future__ import annotations

from dataclasses import dataclass

from ..constants import PixelFormat, PixelFormatFlag


@dataclass(frozen=True)
class DisplayProfile:
    name: str
    logical_width: int
    logical_height: int
    physical_width: int
    physical_height: int
    default_storage_pixel_format: PixelFormat
    storage_pixel_format: PixelFormat
    storage_pixel_format_flag: PixelFormatFlag
    supported_storage_pixel_format_flags: int
    logical_orientation: int
    logical_to_physical_rotation: int
    scan_order_hint: int
    grayscale_levels: int
    framebuffer_bits_per_pixel: int

    def resolve(self, storage_pixel_format: str | PixelFormat | None = None) -> "DisplayProfile":
        if storage_pixel_format is None:
            return self._resolve_pixel_format(self.default_storage_pixel_format)
        return self._resolve_pixel_format(_parse_storage_pixel_format(storage_pixel_format))

    def _resolve_pixel_format(self, pixel_format: PixelFormat) -> "DisplayProfile":
        pixel_flags_by_format = {
            PixelFormat.GRAY1_PACKED: PixelFormatFlag.GRAY1_PACKED,
            PixelFormat.GRAY2_PACKED: PixelFormatFlag.GRAY2_PACKED,
        }
        pixel_flag = pixel_flags_by_format.get(pixel_format)
        if pixel_flag is None:
            raise ValueError(f"unsupported storage pixel format for {self.name}: {pixel_format.name}")
        if not self.supported_storage_pixel_format_flags & int(pixel_flag):
            raise ValueError(f"unsupported storage pixel format for {self.name}: {pixel_format.name}")
        grayscale_levels = 2 if pixel_format == PixelFormat.GRAY1_PACKED else 4
        framebuffer_bits_per_pixel = 1 if pixel_format == PixelFormat.GRAY1_PACKED else 2
        return DisplayProfile(
            name=self.name,
            logical_width=self.logical_width,
            logical_height=self.logical_height,
            physical_width=self.physical_width,
            physical_height=self.physical_height,
            default_storage_pixel_format=self.default_storage_pixel_format,
            storage_pixel_format=pixel_format,
            storage_pixel_format_flag=pixel_flag,
            supported_storage_pixel_format_flags=self.supported_storage_pixel_format_flags,
            logical_orientation=self.logical_orientation,
            logical_to_physical_rotation=self.logical_to_physical_rotation,
            scan_order_hint=self.scan_order_hint,
            grayscale_levels=grayscale_levels,
            framebuffer_bits_per_pixel=framebuffer_bits_per_pixel,
        )


def parse_storage_pixel_format(value: str | PixelFormat) -> PixelFormat:
    if isinstance(value, PixelFormat):
        return value
    normalized = value.lower().replace("-", "").replace("_", "")
    if normalized in {"gray1", "gray1packed", "1"}:
        return PixelFormat.GRAY1_PACKED
    if normalized in {"gray2", "gray2packed", "2"}:
        return PixelFormat.GRAY2_PACKED
    raise ValueError(f"unsupported storage pixel format: {value}")


# Backwards-compatible alias for internal helper naming.
_parse_storage_pixel_format = parse_storage_pixel_format
