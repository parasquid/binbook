from __future__ import annotations

from dataclasses import dataclass, replace

from ..constants import PixelFormat, PixelFormatFlag


@dataclass(frozen=True)
class PixelFormatDescriptor:
    pixel_format: PixelFormat
    flag: PixelFormatFlag
    grayscale_levels: int
    framebuffer_bits_per_pixel: int


PIXEL_FORMATS: dict[PixelFormat, PixelFormatDescriptor] = {
    PixelFormat.GRAY1_PACKED: PixelFormatDescriptor(
        pixel_format=PixelFormat.GRAY1_PACKED,
        flag=PixelFormatFlag.GRAY1_PACKED,
        grayscale_levels=2,
        framebuffer_bits_per_pixel=1,
    ),
    PixelFormat.GRAY2_PACKED: PixelFormatDescriptor(
        pixel_format=PixelFormat.GRAY2_PACKED,
        flag=PixelFormatFlag.GRAY2_PACKED,
        grayscale_levels=4,
        framebuffer_bits_per_pixel=2,
    ),
    PixelFormat.GRAY4_PACKED: PixelFormatDescriptor(
        pixel_format=PixelFormat.GRAY4_PACKED,
        flag=PixelFormatFlag.GRAY4_PACKED,
        grayscale_levels=16,
        framebuffer_bits_per_pixel=4,
    ),
}


@dataclass(frozen=True)
class DisplayProfile:
    name: str
    family: str
    model: str
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
        descriptor = PIXEL_FORMATS.get(pixel_format)
        if descriptor is None:
            raise ValueError(f"unsupported storage pixel format for {self.name}: {pixel_format.name}")
        if not self.supported_storage_pixel_format_flags & int(descriptor.flag):
            raise ValueError(f"unsupported storage pixel format for {self.name}: {pixel_format.name}")
        return replace(
            self,
            storage_pixel_format=pixel_format,
            storage_pixel_format_flag=descriptor.flag,
            grayscale_levels=descriptor.grayscale_levels,
            framebuffer_bits_per_pixel=descriptor.framebuffer_bits_per_pixel,
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
