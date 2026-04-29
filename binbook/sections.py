from __future__ import annotations

from dataclasses import dataclass
import struct

from .constants import CompressionMethod, SectionId
from .profiles.base import DisplayProfile
from .structs import StringRef

DISPLAY_PROFILE_SIZE = 120
LAYOUT_PROFILE_SIZE = 100
READER_REQUIREMENTS_SIZE = 76

SECTION_STRING_REF_OFFSETS: dict[SectionId, tuple[int, ...]] = {
    SectionId.DISPLAY_PROFILE: (0, 8, 16),
    SectionId.SOURCE_IDENTITY: (60, 68),
    SectionId.BOOK_METADATA: (0, 8, 16, 24, 32, 40),
    SectionId.RENDITION_IDENTITY: (256, 264),
    SectionId.FONT_POLICY: (36, 44, 52),
    SectionId.TYPOGRAPHY_POLICY: (36,),
}

_DISPLAY_PROFILE = struct.Struct("<HHHHBhBIIHHHHBHB")
_LAYOUT_PROFILE = struct.Struct("<HHHHHHHHHHHHBB2sHHI32s32s")
_READER_REQUIREMENTS = struct.Struct("<QQIHHIHHII36s")


@dataclass(frozen=True)
class DisplayProfileSection:
    profile: StringRef
    family: StringRef
    model: StringRef
    logical_width: int
    logical_height: int
    physical_width: int
    physical_height: int
    logical_orientation: int
    logical_to_physical_rotation: int
    scan_order_hint: int
    supported_storage_pixel_format_flags: int
    required_storage_pixel_format_flags: int
    default_storage_pixel_format: int
    reserved_pixel_format: int
    native_grayscale_levels: int
    required_grayscale_levels: int
    framebuffer_bits_per_pixel: int
    waveform_hint: int
    dither_mode: int

    @classmethod
    def from_profile(
        cls,
        profile: DisplayProfile,
        *,
        profile_ref: StringRef | None = None,
        family: StringRef | None = None,
        model: StringRef | None = None,
    ) -> "DisplayProfileSection":
        return cls(
            profile=profile_ref or StringRef(),
            family=family or StringRef(),
            model=model or StringRef(),
            logical_width=profile.logical_width,
            logical_height=profile.logical_height,
            physical_width=profile.physical_width,
            physical_height=profile.physical_height,
            logical_orientation=profile.logical_orientation,
            logical_to_physical_rotation=profile.logical_to_physical_rotation,
            scan_order_hint=profile.scan_order_hint,
            supported_storage_pixel_format_flags=profile.supported_storage_pixel_format_flags,
            required_storage_pixel_format_flags=profile.supported_storage_pixel_format_flags,
            default_storage_pixel_format=profile.storage_pixel_format,
            reserved_pixel_format=0,
            native_grayscale_levels=profile.grayscale_levels,
            required_grayscale_levels=profile.grayscale_levels,
            framebuffer_bits_per_pixel=profile.framebuffer_bits_per_pixel,
            waveform_hint=1,
            dither_mode=0,
        )

    def pack(self) -> bytes:
        return b"".join(
            [
                self.profile.pack(),
                self.family.pack(),
                self.model.pack(),
                _DISPLAY_PROFILE.pack(
                    self.logical_width,
                    self.logical_height,
                    self.physical_width,
                    self.physical_height,
                    self.logical_orientation,
                    self.logical_to_physical_rotation,
                    self.scan_order_hint,
                    self.supported_storage_pixel_format_flags,
                    self.required_storage_pixel_format_flags,
                    self.default_storage_pixel_format,
                    self.reserved_pixel_format,
                    self.native_grayscale_levels,
                    self.required_grayscale_levels,
                    self.framebuffer_bits_per_pixel,
                    self.waveform_hint,
                    self.dither_mode,
                ),
                bytes(32),
                bytes(32),
            ]
        )

    @classmethod
    def unpack(cls, data: bytes) -> "DisplayProfileSection":
        if len(data) < DISPLAY_PROFILE_SIZE:
            raise ValueError("DISPLAY_PROFILE section is too short")
        values = _DISPLAY_PROFILE.unpack_from(data, 24)
        return cls(
            profile=StringRef.unpack(data, 0),
            family=StringRef.unpack(data, 8),
            model=StringRef.unpack(data, 16),
            logical_width=values[0],
            logical_height=values[1],
            physical_width=values[2],
            physical_height=values[3],
            logical_orientation=values[4],
            logical_to_physical_rotation=values[5],
            scan_order_hint=values[6],
            supported_storage_pixel_format_flags=values[7],
            required_storage_pixel_format_flags=values[8],
            default_storage_pixel_format=values[9],
            reserved_pixel_format=values[10],
            native_grayscale_levels=values[11],
            required_grayscale_levels=values[12],
            framebuffer_bits_per_pixel=values[13],
            waveform_hint=values[14],
            dither_mode=values[15],
        )


@dataclass(frozen=True)
class LayoutProfileSection:
    full_width: int
    full_height: int
    header_height: int
    footer_height: int
    margin_top: int
    margin_right: int
    margin_bottom: int
    margin_left: int
    content_x: int
    content_y: int
    content_width: int
    content_height: int
    content_alignment: int
    page_layout_mode: int
    line_spacing_milli_em: int
    paragraph_spacing_milli_em: int
    layout_flags: int

    @classmethod
    def from_profile(cls, profile: DisplayProfile) -> "LayoutProfileSection":
        return cls(
            full_width=profile.logical_width,
            full_height=profile.logical_height,
            header_height=0,
            footer_height=0,
            margin_top=0,
            margin_right=0,
            margin_bottom=0,
            margin_left=0,
            content_x=0,
            content_y=0,
            content_width=profile.logical_width,
            content_height=profile.logical_height,
            content_alignment=1,
            page_layout_mode=1,
            line_spacing_milli_em=3,
            paragraph_spacing_milli_em=0,
            layout_flags=0,
        )

    def pack(self) -> bytes:
        return _LAYOUT_PROFILE.pack(
            self.full_width,
            self.full_height,
            self.header_height,
            self.footer_height,
            self.margin_top,
            self.margin_right,
            self.margin_bottom,
            self.margin_left,
            self.content_x,
            self.content_y,
            self.content_width,
            self.content_height,
            self.content_alignment,
            self.page_layout_mode,
            bytes(2),
            self.line_spacing_milli_em,
            self.paragraph_spacing_milli_em,
            self.layout_flags,
            bytes(32),
            bytes(32),
        )

    @classmethod
    def unpack(cls, data: bytes) -> "LayoutProfileSection":
        if len(data) < LAYOUT_PROFILE_SIZE:
            raise ValueError("LAYOUT_PROFILE section is too short")
        values = _LAYOUT_PROFILE.unpack_from(data)
        return cls(
            full_width=values[0],
            full_height=values[1],
            header_height=values[2],
            footer_height=values[3],
            margin_top=values[4],
            margin_right=values[5],
            margin_bottom=values[6],
            margin_left=values[7],
            content_x=values[8],
            content_y=values[9],
            content_width=values[10],
            content_height=values[11],
            content_alignment=values[12],
            page_layout_mode=values[13],
            line_spacing_milli_em=values[15],
            paragraph_spacing_milli_em=values[16],
            layout_flags=values[17],
        )


@dataclass(frozen=True)
class ReaderRequirementsSection:
    feature_flags: int
    required_features: int
    required_storage_pixel_format_flags: int
    required_grayscale_levels: int
    required_minimum_minor_version: int
    required_compression_method_flags: int
    max_page_width: int
    max_page_height: int
    max_uncompressed_page_bytes: int
    recommended_working_buffer_bytes: int

    @classmethod
    def from_profile(cls, profile: DisplayProfile) -> "ReaderRequirementsSection":
        page_bytes = (profile.logical_width * profile.logical_height * profile.framebuffer_bits_per_pixel + 7) // 8
        return cls(
            feature_flags=(1 << 0) | (1 << 3),
            required_features=(1 << 0) | (1 << 2) | (1 << 3) | (1 << 4),
            required_storage_pixel_format_flags=int(profile.storage_pixel_format_flag),
            required_grayscale_levels=profile.grayscale_levels,
            required_minimum_minor_version=1,
            required_compression_method_flags=1 << CompressionMethod.RLE_PACKBITS,
            max_page_width=profile.logical_width,
            max_page_height=profile.logical_height,
            max_uncompressed_page_bytes=page_bytes,
            recommended_working_buffer_bytes=page_bytes * 2,
        )

    def pack(self) -> bytes:
        return _READER_REQUIREMENTS.pack(
            self.feature_flags,
            self.required_features,
            self.required_storage_pixel_format_flags,
            self.required_grayscale_levels,
            self.required_minimum_minor_version,
            self.required_compression_method_flags,
            self.max_page_width,
            self.max_page_height,
            self.max_uncompressed_page_bytes,
            self.recommended_working_buffer_bytes,
            bytes(36),
        )

    @classmethod
    def unpack(cls, data: bytes) -> "ReaderRequirementsSection":
        if len(data) < READER_REQUIREMENTS_SIZE:
            raise ValueError("READER_REQUIREMENTS section is too short")
        values = _READER_REQUIREMENTS.unpack_from(data)
        return cls(
            feature_flags=values[0],
            required_features=values[1],
            required_storage_pixel_format_flags=values[2],
            required_grayscale_levels=values[3],
            required_minimum_minor_version=values[4],
            required_compression_method_flags=values[5],
            max_page_width=values[6],
            max_page_height=values[7],
            max_uncompressed_page_bytes=values[8],
            recommended_working_buffer_bytes=values[9],
        )


assert _DISPLAY_PROFILE.size == 32
assert _LAYOUT_PROFILE.size == LAYOUT_PROFILE_SIZE
assert _READER_REQUIREMENTS.size == READER_REQUIREMENTS_SIZE
