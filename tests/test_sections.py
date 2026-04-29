from __future__ import annotations

from binbook.constants import PixelFormatFlag, SectionId
from binbook.profiles import XTEINK_X4_PORTRAIT
from binbook.sections import (
    SECTION_STRING_REF_OFFSETS,
    DisplayProfileSection,
    LayoutProfileSection,
    ReaderRequirementsSection,
)
from binbook.structs import StringRef


def test_profile_sections_roundtrip_named_fields():
    display = DisplayProfileSection.from_profile(
        XTEINK_X4_PORTRAIT,
        profile_ref=StringRef(1, 2),
        family=StringRef(3, 4),
        model=StringRef(5, 6),
    )
    parsed_display = DisplayProfileSection.unpack(display.pack())

    assert parsed_display.profile == StringRef(1, 2)
    assert parsed_display.family == StringRef(3, 4)
    assert parsed_display.model == StringRef(5, 6)
    assert parsed_display.logical_width == 480
    assert parsed_display.logical_height == 800
    assert parsed_display.physical_width == 800
    assert parsed_display.physical_height == 480
    assert parsed_display.required_storage_pixel_format_flags == int(
        PixelFormatFlag.GRAY1_PACKED | PixelFormatFlag.GRAY2_PACKED
    )
    assert parsed_display.native_grayscale_levels == 4

    layout = LayoutProfileSection.from_profile(XTEINK_X4_PORTRAIT)
    parsed_layout = LayoutProfileSection.unpack(layout.pack())

    assert parsed_layout.full_width == 480
    assert parsed_layout.full_height == 800
    assert parsed_layout.content_width == 480
    assert parsed_layout.content_height == 800

    requirements = ReaderRequirementsSection.from_profile(XTEINK_X4_PORTRAIT)
    parsed_requirements = ReaderRequirementsSection.unpack(requirements.pack())

    assert parsed_requirements.required_storage_pixel_format_flags == int(
        PixelFormatFlag.GRAY2_PACKED
    )
    assert parsed_requirements.required_grayscale_levels == 4
    assert parsed_requirements.max_uncompressed_page_bytes == 96_000
    assert parsed_requirements.recommended_working_buffer_bytes == 192_000


def test_section_string_ref_offsets_are_named_by_section():
    assert SECTION_STRING_REF_OFFSETS[SectionId.DISPLAY_PROFILE] == (0, 8, 16)
    assert SECTION_STRING_REF_OFFSETS[SectionId.FONT_POLICY] == (36, 44, 52)
