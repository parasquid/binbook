# BinBook POC Specification

> Historical note: this document describes the original proof-of-concept direction and is no longer authoritative for BinBook 0.1. Use [`../../BINBOOK_FORMAT_SPEC.md`](../../BINBOOK_FORMAT_SPEC.md) for the current 0.1 candidate format.

## 1. Purpose

BinBook is a universal compiled raster-book container for low-RAM e-ink and embedded display devices.

The format stores pre-rendered page content as compressed grayscale page blobs.

The format does not store reflowable text, CSS, font instructions for runtime rendering, or EPUB layout instructions for the reader.

The intended first proof of concept is:

    EPUB
      → Python compiler
      → .binbook
      → Python decoder/viewer on desktop

The first target profile is:

    xteink-x4-portrait

The first implementation is a Python POC for both encoding and decoding.

## 2. Core Design Principles

### 2.1 Compiler does heavy work

The compiler is responsible for:

    - reading EPUB
    - extracting metadata
    - processing the EPUB spine in reading order
    - mapping EPUB TOC/chapter entries to rendered pages
    - handling fonts
    - applying typography policy
    - paginating text
    - rasterizing text
    - extracting images from the flow
    - creating separate image pages
    - resizing/rotating images to fit
    - dithering images
    - packing grayscale pixels
    - compressing page blobs
    - writing fixed binary indexes and metadata
    - writing the UTF-8 string table

### 2.2 Reader/firmware does light work

The reader/firmware is responsible for:

    - validating the file
    - validating the target display profile
    - validating format version and required features
    - reading the fixed binary section table
    - reading the fixed binary page index
    - reading the fixed binary navigation index
    - reading strings through StringRef records
    - caching compressed page blobs
    - decoding selected pages
    - converting pixels to display-native format if needed
    - drawing firmware UI/chrome
    - displaying the page

### 2.3 No reflow on device

BinBook is intentionally non-reflowable.

Changing any of these requires recompilation:

    - font
    - font size
    - font weight
    - font scale / compile-time zoom
    - minimum font size
    - margins
    - line spacing
    - character spacing
    - word spacing
    - paragraph spacing
    - screen size
    - orientation
    - grayscale policy
    - image fitting policy
    - chrome policy

### 2.4 Page blobs contain content only

Page blobs store book content pixels only.

The firmware/reader renders:

    - page number
    - current chapter title
    - progress
    - status bar
    - battery indicator
    - menus
    - selection/debug overlays

Therefore, BinBook includes layout metadata and ChromePolicy telling the reader how content and optional chrome should be placed.

### 2.5 No dynamic metadata format in v0.1

BinBook v0.1 is an all-binary container.

Required runtime metadata must not be encoded as:

    - JSON
    - CBOR
    - Protocol Buffers
    - MessagePack
    - YAML
    - TOML
    - dynamically parsed object maps

Variable-length text is stored in the STRING_TABLE section as UTF-8 and referenced by fixed binary StringRef records.

The inspect tool may output JSON for humans, but the .binbook file itself must not use JSON sections.

## 3. First POC Scope

### 3.1 Encoder

The encoder is a Python CLI that takes an EPUB and outputs a .binbook.

Example:

    binbook encode input.epub \
      --profile xteink-x4-portrait \
      --font path/to/font.ttf \
      --font-mode force \
      --font-size 24 \
      --font-weight 400 \
      --min-font-size 18 \
      --font-scale 1.10 \
      --line-height 1.25 \
      --char-spacing 0.02 \
      --paragraph-spacing-after 8 \
      --dither floyd-steinberg \
      --dither-strength 1.00 \
      --output output.binbook

### 3.2 Developer/debug encoder

The developer/debug encoder takes a folder of already-rendered PNG pages and outputs a .binbook.

Example:

    binbook encode-png-folder ./pages \
      --profile xteink-x4-portrait \
      --pixel-format gray2 \
      --output test.binbook

This command is not the main user-facing workflow.

It exists to validate the binary container, pixel packing, compression, page index, decoder, and viewer without involving EPUB parsing/rendering.

### 3.3 Decoder

The decoder reads a .binbook and exports a selected page to PNG.

Example:

    binbook decode output.binbook --page 1 --output page001.png

### 3.4 Desktop simulation viewer

The viewer is a Python desktop simulation viewer for .binbook files.

Example:

    binbook view output.binbook

The viewer should simulate how the book will look on the target device.

### 3.5 Inspector

The inspector prints structural and metadata information.

Example:

    binbook inspect output.binbook --validate

The inspector may print human-readable text or JSON to stdout, but this does not change the .binbook format.

## 4. First Device Profile: Xteink X4 Portrait

The POC should define a profile named:

    xteink-x4-portrait

The logical reading orientation should be portrait.

Use logical page dimensions:

    logical_width_px  = 480
    logical_height_px = 800

The physical panel may be 800 × 480, but BinBook page blobs should be stored in logical reading orientation for the POC.

For xteink-x4-portrait v0.1:

    - allowed page pixel formats: GRAY2_PACKED only
    - text pages: GRAY2_PACKED
    - image pages: GRAY2_PACKED
    - intended output grayscale levels: 4
    - required storage pixel formats: GRAY2_PACKED
    - GRAY4_PACKED must not be emitted for this profile in v0.1

Source images are converted to grayscale, resized/rotated to fit, dithered/quantized to 4 grayscale levels, then packed as GRAY2_PACKED.

## 5. Xteink XTC/XTCH Compatibility Notes

Xteink native formats distinguish:

    XTC / XTG:
      1-bit monochrome pages

    XTCH / XTH:
      2-bit / 4-level grayscale pages

BinBook does not use XTG/XTH as its internal page blob format.

BinBook stores canonical logical pages in row-major order.

For xteink-x4-portrait, BinBook emits GRAY2_PACKED pages only.

The X4 firmware/display backend is responsible for converting BinBook GRAY2 row-major pixels to the display controller's native update order and LUT mapping.

Canonical BinBook GRAY2:

    0 = black
    1 = dark gray
    2 = light gray
    3 = white

Xteink XTH LUT mapping:

    0 = white
    1 = dark gray
    2 = light gray
    3 = black

Conversion from BinBook canonical to Xteink XTH pixel value:

    BinBook black      0 → XTH 3
    BinBook dark gray  1 → XTH 1
    BinBook light gray 2 → XTH 2
    BinBook white      3 → XTH 0

This conversion belongs in the X4 display backend, not in the BinBook page blob storage.

## 6. File Layout

Use a single .binbook file.

No required sidecar files.

High-level layout:

    .binbook
    ├── fixed header
    ├── section table
    ├── metadata/index region
    │   ├── string table
    │   ├── display profile
    │   ├── layout profile
    │   ├── reader requirements
    │   ├── source identity
    │   ├── book metadata
    │   ├── rendition identity
    │   ├── font policy
    │   ├── typography policy
    │   ├── image policy
    │   ├── compression policy
    │   ├── chrome policy
    │   ├── navigation index
    │   └── page index
    ├── optional zero padding/alignment
    └── page data region
        ├── page blob 1
        ├── page blob 2
        ├── page blob 3
        └── ...

The section table is authoritative.

Readers must not depend on physical order except that the header starts at byte 0.

Recommended physical order:

    1. Header
    2. Section table
    3. String table
    4. Display profile
    5. Layout profile
    6. Reader requirements
    7. Source identity
    8. Book metadata
    9. Rendition identity
    10. Font policy
    11. Typography policy
    12. Image policy
    13. Compression policy
    14. Chrome policy
    15. Navigation index
    16. Page index
    17. Zero padding/alignment
    18. Page data

## 7. Section IDs

Use these section IDs:

    0  = INVALID

    1  = STRING_TABLE

    10 = DISPLAY_PROFILE
    11 = LAYOUT_PROFILE
    12 = READER_REQUIREMENTS

    20 = SOURCE_IDENTITY
    21 = BOOK_METADATA
    22 = RENDITION_IDENTITY

    30 = FONT_POLICY
    31 = TYPOGRAPHY_POLICY
    32 = IMAGE_POLICY
    33 = COMPRESSION_POLICY
    34 = CHROME_POLICY

    40 = PAGE_INDEX
    41 = NAV_INDEX
    42 = PAGE_LABELS_RESERVED

    50 = PAGE_DATA

    60 = PROGRESS_MAP_RESERVED
    61 = SEARCH_INDEX_RESERVED
    62 = PAGE_BLOCK_INDEX_RESERVED
    63 = DEBUG_INFO_RESERVED

Required sections for v0.1:

    - STRING_TABLE
    - DISPLAY_PROFILE
    - LAYOUT_PROFILE
    - READER_REQUIREMENTS
    - SOURCE_IDENTITY
    - BOOK_METADATA
    - RENDITION_IDENTITY
    - FONT_POLICY
    - TYPOGRAPHY_POLICY
    - IMAGE_POLICY
    - COMPRESSION_POLICY
    - CHROME_POLICY
    - PAGE_INDEX
    - NAV_INDEX
    - PAGE_DATA

## 8. Endianness and Primitive Types

All multi-byte integer fields are little-endian.

Primitive types:

    u8   = unsigned 8-bit integer
    i16  = signed 16-bit integer
    u16  = unsigned 16-bit integer
    i32  = signed 32-bit integer
    u32  = unsigned 32-bit integer
    u64  = unsigned 64-bit integer

Hashes are raw byte arrays.

Reserved bytes must be written as zero.

## 9. Padding and Reserved Bytes

All padding and reserved bytes written by the compiler MUST be zero-filled.

Readers MUST ignore padding and reserved bytes.

Readers MUST NOT depend on padding contents for parsing.

Strict validators MAY warn if reserved bytes are non-zero, but normal readers MUST NOT reject a file solely because reserved bytes are non-zero unless the field has been defined as required-zero in the active format version.

No magic values or sigils are used for padding.

The only required v0.1 magic value is:

    magic[8] = "BINBOOK\0"

## 10. Versioning

BinBook must be explicitly versioned.

For the POC:

    version_major = 0
    version_minor = 1

Rules:

    - readers MUST reject files where version_major != supported_major_version
    - readers SHOULD NOT reject a file solely because version_minor is greater than the reader's supported minor version
    - readers MAY reject a same-major newer-minor file if it uses required features, required entry sizes, required sections, or required semantics that the reader does not support
    - unknown optional sections may be ignored
    - header_size allows the header to grow
    - section_table_entry_size allows section records to grow
    - page_index_entry_size allows page index records to grow
    - nav_index_entry_size allows nav index records to grow

## 11. Header

Use a fixed-size 256-byte header.

Header layout, little-endian:

    magic[8]                    = "BINBOOK\0"
    version_major: u16           = 0
    version_minor: u16           = 1
    header_size: u16             = 256
    header_flags: u16

    file_size: u64

    section_table_offset: u64
    section_table_length: u32
    section_table_entry_size: u16
    section_count: u16

    page_index_entry_size: u16
    nav_index_entry_size: u16

    page_data_offset: u64
    page_data_length: u64

    file_crc32: u32              = 0 if unused
    header_crc32: u32            = 0 if unused

    reserved[...]                = zero-filled to 256 bytes

header_flags:

    Reserved for future use in v0.1.
    Writers MUST set header_flags = 0.
    Readers MUST ignore non-zero header_flags in v0.1.
    Strict validators MAY warn on non-zero header_flags.

file_size:

    Exact logical size of the .binbook file in bytes.
    If file_size is 0, file size validation is skipped (consistent with CRC32 convention).
    If file_size is nonzero, readers SHOULD validate that the actual file size is at least file_size.
    If the actual file is smaller than a nonzero file_size, the file is invalid.
    Extra bytes beyond file_size, if any, SHOULD be ignored.

## 12. v0.1 Entry Sizes

Expected v0.1 entry sizes:

    SectionEntry: 40 bytes
    PageIndexEntry: 76 bytes
    NavIndexEntry: 48 bytes

Header fields should therefore be:

    section_table_entry_size = 40
    page_index_entry_size = 76
    nav_index_entry_size = 48

Readers MUST reject files where these entry-size values do not match the v0.1 expectations unless they explicitly support that alternate same-major layout.

## 13. SectionEntry

SectionEntry layout, little-endian:

    section_id: u16
    section_flags: u16
    offset: u64
    length: u64
    entry_size: u32
    record_count: u32
    crc32: u32
    reserved[8]

This is 40 bytes per entry.

Field sizes: 2+2+8+8+4+4+4+8 = 40 bytes.

section_flags:

    Reserved for future use in v0.1.
    Writers MUST set section_flags = 0.
    Readers MUST ignore non-zero section_flags in v0.1.
    Strict validators MAY warn on non-zero section_flags.

Rules:

    - offset and length are absolute file offsets/lengths
    - entry_size is 0 if the section is not record-based
    - record_count is 0 if the section is not record-based
    - crc32 may be 0 if unused
    - PAGE_DATA section offset must equal header.page_data_offset
    - PAGE_DATA section length must equal header.page_data_length

## 14. Page Data Offset and Alignment

The header must encode:

    page_data_offset
    page_data_length

The compiler should align page_data_offset to a useful boundary.

POC default:

    64 KiB alignment

Algorithm:

    page_data_offset = align_up(end_of_metadata, 65536)

The firmware/decoder must not hardcode 64 KiB.

It must read page_data_offset from the header.

All alignment padding must be zero-filled.

## 15. CRC32

All CRC32 fields use IEEE 802.3 / PKZIP CRC32.

Parameters:

    Polynomial: 0xEDB88320
    Initial value: 0xFFFFFFFF
    Final XOR: 0xFFFFFFFF

CRC fields:

    file_crc32
    header_crc32
    SectionEntry.crc32
    PageIndexEntry.page_crc32

Coverage:

    file_crc32:
      Entire file from byte 0 through header.file_size bytes, with file_crc32 set to 0 during computation.

    header_crc32:
      Entire 256-byte header, with header_crc32 set to 0 during computation.

    SectionEntry.crc32:
      Section data from offset to offset + length.

    PageIndexEntry.page_crc32:
      Entire compressed page blob for that page entry.

If a CRC32 field is 0, the checksum is considered not computed and validation should be skipped.

## 16. Hashes

All *_hash[32] fields are SHA-256 digests.

Hashes are stored as 32 raw bytes, not hex strings.

Each profile or policy hash is computed over that section's exact binary data as stored in the file, with that section's own hash field zeroed during computation.

rendition_hash is computed over a canonical byte sequence containing:

    - source EPUB SHA-256, 32 bytes
    - display_profile_hash, 32 bytes
    - layout_profile_hash, 32 bytes
    - font_policy_hash, 32 bytes
    - typography_policy_hash, 32 bytes
    - image_policy_hash, 32 bytes
    - compression_policy_hash, 32 bytes
    - chrome_policy_hash, 32 bytes
    - compiler version string as UTF-8 bytes, length-prefixed as u32 length + bytes

## 17. StringRef

Use StringRef for all variable-length strings.

StringRef layout:

    offset: u32
    length: u32

Both fields are relative to the start of the STRING_TABLE section.

Rules:

    - strings are UTF-8
    - strings are not null-terminated
    - for length > 0, offset + length must be within STRING_TABLE length
    - for length == 0, the offset field is ignored
    - writers SHOULD set offset = 0 when length == 0
    - readers MUST NOT validate offset bounds when length == 0
    - decoder must validate UTF-8 or safely replace invalid strings during inspection/viewing
    - firmware may choose to display only valid UTF-8 strings and ignore invalid strings

Low-RAM access:

    Firmware is not required to load the entire STRING_TABLE into RAM.
    Since StringRef stores offsets within the STRING_TABLE section,
    firmware may seek directly to section.offset + StringRef.offset and read only the required string bytes.

## 18. ReaderRequirements Section

ReaderRequirements layout:

    feature_flags: u64
    required_reader_features: u64

    required_storage_pixel_formats: u32
    required_output_grayscale_levels: u16
    fallback_output_policy: u16

    required_compression_methods: u32

    max_stored_page_width_px: u16
    max_stored_page_height_px: u16
    max_uncompressed_page_size: u32
    max_compressed_page_size: u32

    reserved[36]

Output grayscale levels:

    0  = unknown
    2  = black/white, 1-bit
    4  = 4-level grayscale, 2-bit
    16 = 16-level grayscale, 4-bit

fallback_output_policy:

    0 = unknown
    1 = reject_if_output_levels_insufficient
    2 = allow_downquantize
    3 = allow_dither_to_1bit

Feature flag examples:

    bit 0  = has_page_crc32
    bit 1  = has_file_crc32
    bit 2  = has_rendition_hash
    bit 3  = has_nav_index
    bit 4  = has_source_identity
    bit 5  = has_book_metadata
    bit 6  = has_font_policy
    bit 7  = has_typography_policy
    bit 8  = has_image_policy
    bit 9  = has_compression_policy
    bit 10 = has_chrome_policy

Required reader feature examples:

    bit 0 = requires_gray2_decode
    bit 1 = requires_gray4_decode
    bit 2 = requires_rle_packbits
    bit 3 = requires_string_table
    bit 4 = requires_logical_orientation

These bits are the canonical source for reader capability requirements.
No separate u8 fields duplicate them.

For xteink-x4-portrait:

    required_storage_pixel_formats = GRAY2_PACKED
    required_output_grayscale_levels = 4
    fallback_output_policy = reject_if_output_levels_insufficient or allow_dither_to_1bit

## 19. DisplayProfile Section

DisplayProfile layout:

    profile_id: StringRef
    device_family: StringRef
    device_model: StringRef

    logical_width_px: u16
    logical_height_px: u16
    physical_width_px: u16
    physical_height_px: u16

    orientation: u8
    rotation_from_physical: i16
    scan_order_hint: u8

    supported_storage_pixel_formats: u32
    native_output_pixel_formats: u32

    native_output_format_hint: u16
    display_controller_hint: u16

    native_grayscale_levels: u16
    panel_grayscale_levels: u16
    framebuffer_bits_per_pixel: u8
    output_gray_mapping: u16
    reserved_u8: u8

    display_profile_hash[32]
    reserved[32]

orientation:

    0 = unknown
    1 = portrait
    2 = landscape

scan_order_hint:

    0 = unknown
    1 = row_major_left_to_right_top_to_bottom
    2 = row_major_right_to_left_top_to_bottom
    3 = column_major_reserved

Pixel format bit flags:

    bit 0 = GRAY1_PACKED
    bit 1 = GRAY2_PACKED
    bit 2 = GRAY4_PACKED

output_gray_mapping:

    0 = unknown
    1 = linear_black_to_white
    2 = linear_white_to_black
    3 = xteink_xth_lut_0_white_1_dark_2_light_3_black

For xteink-x4-portrait POC:

    logical_width_px  = 480
    logical_height_px = 800
    physical_width_px = 800
    physical_height_px = 480
    orientation = portrait
    supported_storage_pixel_formats = GRAY2_PACKED
    native_output_pixel_formats = GRAY2_PACKED
    native_grayscale_levels = 4
    panel_grayscale_levels = 4
    framebuffer_bits_per_pixel = 2 for intended grayscale output
    output_gray_mapping = xteink_xth_lut_0_white_1_dark_2_light_3_black

## 20. LayoutProfile Section

LayoutProfile layout:

    full_page_width_px: u16
    full_page_height_px: u16

    reserved_header_height_px: u16
    reserved_footer_height_px: u16

    margin_top_px: u16
    margin_right_px: u16
    margin_bottom_px: u16
    margin_left_px: u16

    content_x_px: u16
    content_y_px: u16
    content_width_px: u16
    content_height_px: u16

    reading_direction: u8
    page_turn_direction: u8
    reserved_u8[2]

    page_background_gray: u16
    reserved_u16: u16

    layout_flags: u32
    layout_profile_hash[32]
    reserved[32]

full_page_width_px / full_page_height_px:

    Total dimensions of a rendered page blob, in pixels.
    These MUST equal DisplayProfile.logical_width_px and logical_height_px.
    If they differ, the file is invalid.

content_x_px / content_y_px / content_width_px / content_height_px:

    The content box within the page, derived from margins:

        content_x_px     = margin_left_px
        content_y_px     = margin_top_px + reserved_header_height_px
        content_width_px = full_page_width_px - margin_left_px - margin_right_px
        content_height_px = full_page_height_px
                            - margin_top_px - margin_bottom_px
                            - reserved_header_height_px - reserved_footer_height_px

    Writers MUST store these derived values explicitly so readers can use them
    without recomputing.
    Strict validators SHOULD verify the content box fields are consistent with
    the margin fields.

reading_direction:

    0 = unknown
    1 = left_to_right
    2 = right_to_left
    3 = top_to_bottom_reserved

page_turn_direction:

    0 = unknown
    1 = next_is_right_or_down
    2 = next_is_left_or_up

## 21. SourceIdentity Section

SourceIdentity layout:

    source_type: u16
    source_flags: u16

    original_file_size: u64
    original_md5[16]
    original_sha256[32]

    original_filename: StringRef
    source_package_identifier: StringRef

    reserved[32]

source_type:

    0 = unknown
    1 = EPUB

## 22. BookMetadata Section

BookMetadata layout:

    title: StringRef
    subtitle: StringRef
    author: StringRef
    publisher: StringRef
    language: StringRef
    series_name: StringRef

    series_index_milli: u32
    metadata_flags: u32

    reserved[32]

series_index_milli:

    1000 = book 1
    1500 = book 1.5
    0 = unused

## 23. RenditionIdentity Section

RenditionIdentity layout:

    rendition_hash[32]
    display_profile_hash[32]
    layout_profile_hash[32]
    font_policy_hash[32]
    typography_policy_hash[32]
    image_policy_hash[32]
    compression_policy_hash[32]
    chrome_policy_hash[32]

    compiler_name: StringRef
    compiler_version: StringRef

    created_unix_time: u64
    reserved[32]

## 24. FontPolicy Section

FontPolicy layout:

    font_mode: u16
    font_flags: u16

    font_sha256[32]

    font_name: StringRef
    font_original_path: StringRef
    renderer_name: StringRef

    font_policy_hash[32]
    reserved[32]

font_mode:

    0 = unknown
    1 = default
    2 = force

font_flags:

    bit 0 = force_custom_font
    bit 1 = preserve_epub_font_family_when_possible
    bit 2 = allow_synthetic_bold
    bit 3 = allow_synthetic_italic
    bit 4 = preserve_epub_font_weight_when_possible
    bit 5 = preserve_epub_font_style_when_possible

## 25. TypographyPolicy Section

TypographyPolicy layout:

    base_font_size_px: u16
    minimum_font_size_px: u16
    maximum_font_size_px: u16
    font_weight: u16

    font_scale_milli: u32
    line_height_milli: u32

    paragraph_spacing_before_px: u16
    paragraph_spacing_after_px: u16

    character_spacing_milli_em: i32
    word_spacing_milli_em: i32

    text_align: u8
    hyphenation_policy: u8
    widow_orphan_policy: u8
    reserved_u8: u8

    hyphenation_language: StringRef

    typography_flags: u32

    typography_policy_hash[32]
    reserved[32]

font_weight:

    CSS-like numeric weight.
    400 = normal
    700 = bold

font_scale_milli:

    1000 = 100%
    1100 = 110%
    1250 = 125%

line_height_milli:

    1000 = 1.0
    1250 = 1.25
    1500 = 1.5

character_spacing_milli_em:

    signed thousandths of current font size
    0 = normal
    50 = +0.05em
    100 = +0.10em
    -50 = -0.05em

word_spacing_milli_em:

    signed thousandths of current font size

text_align:

    0 = unknown
    1 = left
    2 = center
    3 = right
    4 = justify
    5 = preserve

hyphenation_policy:

    0 = unknown
    1 = disabled
    2 = enabled
    3 = preserve_epub_setting

widow_orphan_policy:

    0 = unknown
    1 = disabled
    2 = basic
    3 = preserve_epub_setting

typography_flags:

    bit 0 = enforce_minimum_font_size
    bit 1 = enforce_maximum_font_size
    bit 2 = force_line_height
    bit 3 = preserve_epub_line_height_when_possible
    bit 4 = force_character_spacing
    bit 5 = preserve_epub_character_spacing_when_possible
    bit 6 = force_word_spacing
    bit 7 = preserve_epub_word_spacing_when_possible
    bit 8 = force_text_align
    bit 9 = preserve_epub_text_align_when_possible
    bit 10 = hyphenation_enabled
    bit 11 = widow_orphan_control_enabled

## 26. ImagePolicy Section

ImagePolicy layout:

    image_page_mode: u16
    image_pixel_format: u16
    image_fit_mode: u16
    image_rotation_policy: u16

    image_dither_method: u16
    image_background_gray: u16
    alpha_background_gray: u16
    dithering_strength_milli: u16

    skip_tiny_images_below_px: u16
    max_source_image_width_px: u16
    max_source_image_height_px: u16
    reserved_u16: u16

    image_flags: u32

    image_policy_hash[32]
    reserved[32]

image_page_mode:

    1 = separate_page

image_fit_mode:

    1 = fit_inside_no_crop
    2 = crop_to_fill_reserved
    3 = split_long_image_reserved

image_rotation_policy:

    0 = never
    1 = rotate_if_better_fit

image_dither_method:

    0 = none
    1 = floyd_steinberg
    2 = ordered_bayer_reserved

dithering_strength_milli:

    1000 = normal strength

For xteink-x4-portrait:

    image_pixel_format = GRAY2_PACKED
    image pages are dithered/quantized to 4 grayscale levels

## 27. CompressionPolicy Section

CompressionPolicy layout:

    default_compression_method: u16
    allowed_compression_methods: u32

    block_model: u16
    strip_height_px: u16

    compression_flags: u32

    compression_policy_hash[32]
    reserved[32]

block_model:

    1 = whole_page_blob
    2 = strip_blobs_reserved

## 28. ChromePolicy Section

ChromePolicy layout:

    reserved[4]

    progress_bar_mode: u16
    progress_bar_position: u16

    chrome_flags: u32

    chrome_policy_hash[32]
    reserved[32]

progress_bar_mode:

    0 = none
    1 = book_progress
    2 = chapter_progress
    3 = book_and_chapter_reserved

progress_bar_position:

    0 = unknown
    1 = top
    2 = bottom
    3 = left_reserved
    4 = right_reserved

chrome_flags:

    bit 0 = dark_mode_negative
    bit 1 = show_page_numbers
    bit 2 = show_page_number_x_of_y
    bit 3 = show_percent
    bit 4 = show_chapter_marks
    bit 5 = show_chapter_title

chrome_flags notes:

    dark_mode_negative:
      Runtime UI hint only. Page blobs are not pre-inverted.
      The firmware/viewer applies negative rendering to chrome and optionally
      to page pixels at display time. Page blobs remain canonical (light-mode)
      in PAGE_DATA regardless of this flag.

    show_page_numbers:
      Whether to show any page number indicator at all.

    show_page_number_x_of_y:
      Format page numbers as "X of Y" rather than just "X".
      Only relevant if show_page_numbers is set.

ChromePolicy is a default/rendering hint for firmware/viewer chrome.

Chrome pixels are not baked into page blobs.

## 29. PageIndexEntry

PAGE_INDEX is a fixed-size binary record array.

PageIndexEntry layout:

    page_number: u32
    page_kind: u16
    pixel_format: u16

    compression_method: u16
    update_hint: u16
    page_flags: u32

    relative_blob_offset: u64
    compressed_size: u32
    uncompressed_size: u32
    page_crc32: u32

    stored_width: u16
    stored_height: u16
    placement_x: u16
    placement_y: u16

    source_spine_index: u32
    chapter_nav_index: u32

    progress_start_ppm: u32
    progress_end_ppm: u32

    reserved[16]

page_number:

    0-based index. The first page is page_number = 0.

page_kind:

    1 = TEXT
    2 = IMAGE
    3 = MIXED_RESERVED

Readers MUST reject pages with page_kind = MIXED_RESERVED (3) in v0.1.

pixel_format:

    1 = GRAY1_PACKED
    2 = GRAY2_PACKED
    4 = GRAY4_PACKED

For xteink-x4-portrait:

    pixel_format MUST be GRAY2_PACKED for all pages.

compression_method:

    0 = NONE
    1 = RLE_PACKBITS
    2 = HEATSHRINK_RESERVED
    3 = LZ4_BLOCK_RESERVED
    4 = ZLIB_DEBUG_RESERVED

update_hint:

    0 = default
    1 = full_refresh_recommended
    2 = partial_refresh_ok

source_spine_index:

    0-based index of the EPUB spine item this page was rendered from.
    UINT32_MAX (0xFFFFFFFF) means no spine item (e.g., pages from encode-png-folder).

chapter_nav_index:

    0-based index into NAV_INDEX of the chapter this page belongs to.
    UINT32_MAX (0xFFFFFFFF) means no chapter assignment.

The absolute blob location is:

    absolute_offset = header.page_data_offset + page.relative_blob_offset

## 30. NavIndexEntry

NAV_INDEX is a fixed-size binary record array.

NavIndexEntry layout:

    nav_index: u32
    nav_type: u16
    level: u16

    title: StringRef
    source_href: StringRef

    source_spine_index: u32
    rendered_page_number: u32

    parent_nav_index: u32
    first_child_nav_index: u32
    next_sibling_nav_index: u32

    nav_flags: u32

This is 48 bytes per entry.

Field sizes: 4+2+2+8+8+4+4+4+4+4+4 = 48 bytes.

nav_index:

    0-based.
    The first navigation entry has nav_index = 0.

rendered_page_number:

    0-based index into the page array.
    The first page is rendered_page_number = 0.

nav_type:

    1 = COVER
    2 = TOC
    3 = CHAPTER
    4 = SECTION
    5 = LANDMARK

UINT32_MAX means no parent/child/sibling.

## 31. Page Data Section

PAGE_DATA is raw concatenated compressed page blobs.

For v0.1, page blobs do not have page-local headers.

## 32. Pixel Packing

### Pixel Storage Order

Page blobs are stored in row-major order:

    - bytes are arranged sequentially by row
    - the top row is stored first
    - the bottom row is stored last
    - within each row, pixels are arranged left-to-right
    - the logical origin (0,0) is the top-left of the content area

Firmware with a different native scan order must convert during display.

### Row Padding

Each row is padded to the nearest whole byte boundary.

    GRAY1_PACKED: row occupies ceil(width / 8) bytes
    GRAY2_PACKED: row occupies ceil(width / 4) bytes
    GRAY4_PACKED: row occupies ceil(width / 2) bytes

Unused bits in the last byte of a row MUST be zero-filled by the writer.

Readers MUST ignore unused bits in the last byte of each row.

For xteink-x4-portrait (width = 480):

    GRAY2_PACKED: 480 / 4 = 120 bytes per row (exact, no padding needed)

### GRAY1_PACKED

GRAY1_PACKED:

    8 pixels per byte
    leftmost pixel in highest bit

    pixel 0 → bit 7
    pixel 1 → bit 6
    pixel 2 → bit 5
    pixel 3 → bit 4
    pixel 4 → bit 3
    pixel 5 → bit 2
    pixel 6 → bit 1
    pixel 7 → bit 0

Canonical BinBook values:

    0 = black
    1 = white

GRAY1_PACKED is part of the universal format for future 1-bit profiles.
It is not emitted for xteink-x4-portrait v0.1.

### GRAY2_PACKED

GRAY2_PACKED:

    4 pixels per byte
    leftmost pixel in highest bits

    pixel 0 → bits 7..6
    pixel 1 → bits 5..4
    pixel 2 → bits 3..2
    pixel 3 → bits 1..0

Canonical BinBook values:

    0 = black
    1 = dark gray
    2 = light gray
    3 = white

### GRAY4_PACKED

GRAY4_PACKED:

    2 pixels per byte
    leftmost pixel in high nibble
    next pixel in low nibble

Canonical BinBook values:

    0  = black
    15 = white

GRAY4_PACKED is part of the universal format but is not emitted for xteink-x4-portrait v0.1.

## 33. Xteink XTH Output Conversion

For xteink-x4-portrait, the firmware/display backend converts canonical BinBook GRAY2 to Xteink XTH grayscale values.

Conversion table:

    BinBook black      0 → XTH 3
    BinBook dark gray  1 → XTH 1
    BinBook light gray 2 → XTH 2
    BinBook white      3 → XTH 0

The conversion is not applied inside PAGE_DATA.

PAGE_DATA remains canonical BinBook row-major GRAY2.

## 34. Compression

RLE_PACKBITS:

    control byte 0..127:
      literal run of control + 1 bytes

    control byte 128..255:
      repeated-byte run of (control & 127) + 1 bytes
      followed by one byte to repeat

Note: BinBook RLE_PACKBITS differs from standard Apple PackBits at control byte
0x80. Standard PackBits treats 0x80 as a no-op; BinBook treats it as a 1-byte
repeat run ((0x80 & 0x7F) + 1 = 1). Do not use an off-the-shelf PackBits
library without verifying it matches this behaviour.

## 35. EPUB Handling

Use spine-aware processing.

The compiler should:

    1. Open EPUB.
    2. Extract metadata.
    3. Compute MD5 and SHA-256 of the original EPUB file.
    4. Iterate spine items in reading order.
    5. Load each spine item HTML.
    6. Identify images in reading order.
    7. Remove images from inline text flow.
    8. Render text into pages.
    9. Insert each extracted image as its own page at the correct reading-order position.
    10. Build navigation entries from EPUB TOC where practical.
    11. Map navigation entries to rendered page numbers where practical.

## 36. Text Rendering

For POC, a practical approach is acceptable:

    - Extract readable text from spine HTML.
    - Use Pillow ImageDraw with the chosen TTF.
    - Word-wrap text into the content box.
    - Paginate based on font metrics and line height.
    - Apply typography policy.

## 37. Image Rendering

For xteink-x4-portrait:

    1. Load image with Pillow.
    2. Convert to grayscale.
    3. Decide whether rotation improves fit into the content box.
    4. Rotate if beneficial.
    5. Resize to fit inside content box.
    6. Do not crop.
    7. Flatten alpha to alpha_background_gray.
    8. Center on image_background_gray.
    9. Dither/quantize to 4 grayscale levels.
    10. Pack as GRAY2_PACKED.
    11. Store as IMAGE page.

Do not emit GRAY4_PACKED for xteink-x4-portrait v0.1.

## 38. Decoder Behavior

The decoder should:

    1. Read header.
    2. Validate magic.
    3. Validate version.
    4. Read section table.
    5. Validate required sections.
    6. Read STRING_TABLE.
    7. Read DISPLAY_PROFILE.
    8. Read LAYOUT_PROFILE.
    9. Read READER_REQUIREMENTS.
    10. Read SOURCE_IDENTITY.
    11. Read BOOK_METADATA.
    12. Read RENDITION_IDENTITY.
    13. Read FONT_POLICY.
    14. Read TYPOGRAPHY_POLICY.
    15. Read IMAGE_POLICY.
    16. Read COMPRESSION_POLICY.
    17. Read CHROME_POLICY.
    18. Read PAGE_INDEX.
    19. Read NAV_INDEX.
    20. Validate page blob bounds.
    21. Validate page blob non-overlap.
    22. Validate progress monotonicity.
    23. Read selected page blob.
    24. Validate page_crc32 if nonzero.
    25. Decompress RLE.
    26. Unpack GRAY2 or GRAY4.
    27. Create full logical canvas.
    28. Place content at placement_x / placement_y.
    29. Optionally draw debug chrome.
    30. Save PNG or display in GUI.

## 39. Validation Rules

The decoder/firmware should validate:

    - magic matches BinBook
    - version_major is supported
    - version_minor is acceptable for this reader
    - required_reader_features are supported
    - file size is valid
    - section table is within file bounds
    - section_table_entry_size is supported
    - page_index_entry_size is supported
    - nav_index_entry_size is supported
    - all required sections are present
    - all section offsets and lengths are within file bounds
    - page_data_offset is within file bounds
    - page_data_offset >= end of metadata
    - PAGE_DATA section matches header page data offset
    - every page blob is within page_data_length
    - no two page blobs overlap
    - StringRef offsets and lengths are within STRING_TABLE
    - strings are valid UTF-8 or safely handled
    - page dimensions fit layout profile
    - pixel format is supported
    - compression method is supported
    - display profile is compatible with the reader
    - progress_start_ppm <= progress_end_ppm
    - progress values are <= 1,000,000
    - progress is monotonically non-decreasing
    - checksum fields when nonzero
    - xteink-x4-portrait uses GRAY2_PACKED only

## 40. Final POC Definition

The POC is successful when:

    1. A Python CLI converts an EPUB into a .binbook.
    2. A Python CLI converts a folder of already-rendered PNGs into a .binbook as a developer/debug path.
    3. The .binbook contains multiple compressed page blobs.
    4. For xteink-x4-portrait, all pages are GRAY2_PACKED.
    5. Text pages are 2-bit grayscale.
    6. Image pages are 2-bit grayscale for X4 and dithered/quantized to 4 grayscale levels.
    7. Images are inserted in reading order on separate pages.
    8. The file contains fixed binary source identity metadata.
    9. The file contains fixed binary book metadata.
    10. The file contains fixed binary rendition identity metadata.
    11. The file contains fixed binary rendering policies.
    12. The file contains fixed binary chrome policy metadata.
    13. The file contains fixed binary navigation metadata.
    14. The file contains a UTF-8 string table.
    15. The file contains version_major and version_minor.
    16. The decoder rejects unsupported major versions.
    17. The decoder rejects unsupported required reader features.
    18. The decoder can decode any page to PNG.
    19. The viewer can page through the book on desktop.
    20. The format is profile-aware and starts with xteink-x4-portrait.
    21. Page data starts at a header-defined offset.
    22. Page blob offsets are relative to page_data_offset.
    23. The file is self-contained and requires no sidecar.
    24. The .binbook file contains no JSON, CBOR, protobuf, or MessagePack sections.
    25. All required runtime metadata is stored as fixed binary records and a UTF-8 string table.
    26. CRC32 fields are implemented and validated when nonzero.
    27. SHA-256 profile/policy/rendition hashes are implemented.
    28. Page blob bounds and overlap validation are implemented.
    29. Progress monotonicity validation is implemented.
    30. Zero-length StringRefs are handled correctly.
    31. MIXED page_kind is rejected in v0.1.
    32. The X4 display conversion helper maps canonical BinBook GRAY2 to Xteink XTH LUT values correctly.
