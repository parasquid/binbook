# BinBook Format Specification v0.1

Language-agnostic specification for the BinBook compiled raster-book format.
Designed for low-RAM e-ink and embedded display devices.

---

## 1. Introduction

BinBook is a **compiled raster-book container**. It stores pre-rendered page content as compressed grayscale page blobs. The format is:

- **Non-reflowable**: Changing font, margins, or layout requires recompilation
- **Pre-rendered**: All text rendering, pagination, and image processing happens at compile time
- **Binary-native**: No JSON, CBOR, protobuf, or MessagePack in the file format
- **Low-RAM friendly**: Fixed-size records, optional sections, seekable string table

### Intended Use

```
Compiler (desktop/cloud):
  EPUB → rendered pages → grayscale packed pixels → compressed blobs → .binbook

Firmware/reader (embedded):
  .binbook → page index → compressed blob → decompress → display
```

The firmware is responsible for: validation, decompression, pixel format conversion, and UI/chrome rendering.

---

## 2. Design Rationale

### Why All-Binary?

**Problem**: JSON/CBOR require a parser, string allocation, and dynamic memory. On 32KB-128KB RAM devices, this is expensive or impossible.

**Solution**: Fixed-size binary records with upfront sizes:
- `section_table_entry_size` tells you how big each section entry is
- `page_index_entry_size` tells you how big each page record is
- `nav_index_entry_size` tells you how big each nav record is

A firmware can:
1. Read the 256-byte header
2. Check `section_table_entry_size` (e.g., 40 bytes)
3. Allocate a 40-byte struct or read 40 bytes at a time
4. Iterate through `section_count` entries without dynamic parsing

### Why StringRef Instead of Inline Strings?

**Problem**: Variable-length strings in fixed records waste space or require complex parsing.

**Solution**: `StringRef { u32 offset; u32 length; }` — 8 bytes, always.
- Strings live in a separate `STRING_TABLE` section
- Firmware can seek directly to `section.offset + ref.offset` and read `ref.length` bytes
- No need to load the entire string table into RAM

### Why Page Blobs Are Content-Only?

**Problem**: Baking UI chrome (page numbers, progress bars, chapter titles) into page pixels wastes space and prevents firmware UI customization.

**Solution**: `ChromePolicy` section tells the firmware how to render UI, while page blobs contain only book content. The firmware composites chrome at display time.

### Why Canonical Pixel Values?

**Problem**: Different displays map grayscale values differently, including inverted polarity or display-specific update encodings.

**Solution**: BinBook defines canonical values:
- `0 = black, 1 = dark gray, 2 = light gray, 3 = white`

Firmware converts to display-native values. This decouples the format from any specific hardware.

### Why Relative Blob Offsets?

**Problem**: Absolute offsets in page index entries would require rewriting all entries if page data moves.

**Solution**: `PageIndexEntry.relative_blob_offset` is relative to `header.page_data_offset`. The firmware computes:
```
absolute_offset = header.page_data_offset + page.relative_blob_offset
```

---

## 3. Binary Format Specification

### 3.1 Endianness and Primitive Types

All multi-byte integers are **little-endian**.

| Type | Size | Description |
|------|------|-------------|
| `u8` | 1 byte | Unsigned 8-bit integer |
| `i16` | 2 bytes | Signed 16-bit integer |
| `u16` | 2 bytes | Unsigned 16-bit integer |
| `i32` | 4 bytes | Signed 32-bit integers |
| `u32` | 4 bytes | Unsigned 32-bit integer |
| `u64` | 8 bytes | Unsigned 64-bit integer |
| `bytes[n]` | n bytes | Raw byte array |
| `StringRef` | 8 bytes | `{ u32 offset; u32 length; }` |

**Note**: `i32` is used for signed fields like `character_spacing_milli_em`. All other integer fields are unsigned.

### 3.2 File Layout Overview

```
.binbook
├── Header (256 bytes, fixed)
├── Section Table (section_count × section_table_entry_size bytes)
├── STRING_TABLE section
├── DISPLAY_PROFILE section
├── LAYOUT_PROFILE section
├── READER_REQUIREMENTS section
├── SOURCE_IDENTITY section
├── BOOK_METADATA section
├── RENDITION_IDENTITY section
├── FONT_POLICY section
├── TYPOGRAPHY_POLICY section
├── IMAGE_POLICY section
├── COMPRESSION_POLICY section
├── CHROME_POLICY section
├── NAV_INDEX section (nav_index_entry_size × record_count bytes)
├── PAGE_INDEX section (page_index_entry_size × record_count bytes)
├── [optional zero padding for page_data alignment]
└── PAGE_DATA section (concatenated compressed page blobs)
```

**Important**: The section table is authoritative. Readers must not depend on physical order except:
- Header starts at byte 0
- `PAGE_DATA` offset is specified in the header

### 3.3 Header (256 bytes)

```c
struct BinBookHeader {
    u8   magic[8];                   // "BINBOOK\0" (0x42 0x49 0x4E 0x42 0x4F 0x4F 0x4B 0x00)
    u16  version_major;              // 0 for v0.1
    u16  version_minor;              // 1 for v0.1
    u16  header_size;                // 256
    u16  header_flags;              // 0 (reserved for future use)

    u64  file_size;                  // Total file size in bytes (0 = skip validation)
    u64  section_table_offset;       // Absolute offset to section table
    u32  section_table_length;       // Length of section table in bytes
    u16  section_table_entry_size;   // 40 for v0.1
    u16  section_count;              // Number of section table entries

    u16  page_index_entry_size;      // 76 for v0.1
    u16  nav_index_entry_size;       // 48 for v0.1

    u64  page_data_offset;           // Absolute offset to PAGE_DATA
    u64  page_data_length;           // Length of PAGE_DATA in bytes

    u32  file_crc32;                // CRC32 of entire file (0 = not computed)
    u32  header_crc32;              // CRC32 of header (0 = not computed)

    u8   reserved[188];             // Zero-filled; fixed header total is 256 bytes
};
```

The fields before `reserved` occupy 68 bytes. The `reserved[188]` tail brings the header to the fixed 256-byte `header_size`.

**Field Details**:

- **magic**: Must be exactly `0x42 0x49 0x4E 0x42 0x4F 0x4F 0x4B 0x00` ("BINBOOK\0")
- **version_major**: Readers must reject files where `version_major` != supported_major
- **version_minor**: Readers should tolerate higher minor versions unless they use unsupported features
- **header_flags**: Must be 0 in v0.1. Readers must ignore non-zero values.
- **file_size**: If nonzero, readers should validate the actual file is at least this size
- **section_table_offset**: Typically 256 (right after the header)
- **section_table_entry_size**: Must be 40 for v0.1
- **page_index_entry_size**: Must be 76 for v0.1
- **nav_index_entry_size**: Must be 48 for v0.1
- **page_data_offset**: Absolute offset to start of page blobs. Typically aligned to 64 KiB.
- **reserved**: Must be zero-filled by writer. Readers must ignore.

### 3.4 Section Table Entry (40 bytes)

```c
struct SectionEntry {
    u16  section_id;        // Section ID (see Section IDs below)
    u16  section_flags;     // 0 (reserved for future use)
    u64  offset;            // Absolute file offset to section data
    u64  length;            // Length of section data in bytes
    u32  entry_size;        // Size of each record (0 for non-record-based sections)
    u32  record_count;      // Number of records (0 for non-record-based sections)
    u32  crc32;            // CRC32 of section data (0 = not computed)
    u8   reserved[8];      // Zero-filled
};
```

**Section IDs**:

| ID | Name | Required | Record-Based |
|----|------|----------|--------------|
| 0 | INVALID | No | - |
| 1 | STRING_TABLE | Yes | No |
| 10 | DISPLAY_PROFILE | Yes | No |
| 11 | LAYOUT_PROFILE | Yes | No |
| 12 | READER_REQUIREMENTS | Yes | No |
| 20 | SOURCE_IDENTITY | Yes | No |
| 21 | BOOK_METADATA | Yes | No |
| 22 | RENDITION_IDENTITY | Yes | No |
| 30 | FONT_POLICY | Yes | No |
| 31 | TYPOGRAPHY_POLICY | Yes | No |
| 32 | IMAGE_POLICY | Yes | No |
| 33 | COMPRESSION_POLICY | Yes | No |
| 34 | CHROME_POLICY | Yes | No |
| 40 | PAGE_INDEX | Yes | Yes (76 bytes each) |
| 41 | NAV_INDEX | Yes | Yes (48 bytes each) |
| 42 | PAGE_LABELS_RESERVED | No | Reserved |
| 50 | PAGE_DATA | Yes | No (raw blobs) |
| 60-63 | RESERVED | No | Reserved for future use |

**Note**: `PAGE_DATA` has no records — `entry_size` and `record_count` are 0. Its offset/length must match `header.page_data_offset` and `header.page_data_length`.

### 3.5 StringRef (8 bytes)

```c
struct StringRef {
    u32  offset;   // Offset within STRING_TABLE section
    u32  length;   // Length in bytes (0 = empty string)
};
```

**Rules**:
- If `length == 0`, the `offset` field is ignored (firmware may set it to 0)
- Strings are UTF-8 encoded and NOT null-terminated
- `offset + length` must be within the STRING_TABLE section bounds
- Firmware can seek to `string_table_offset + ref.offset` and read `ref.length` bytes

### 3.6 STRING_TABLE Section

A contiguous block of UTF-8 bytes. No header, no index — just raw string data.

String references point into this block using `StringRef`:
```
STRING_TABLE:
├── [string 0 bytes] ─── referenced by StringRef { offset=0, length=... }
├── [string 1 bytes]
├── [string 2 bytes]
└── ...
```

**Encoding**: UTF-8. Invalid UTF-8 sequences should be handled gracefully (replacement characters).

### 3.7 DISPLAY_PROFILE Section (92 bytes + hashes)

```c
struct DisplayProfile {
    StringRef profile_id;               // e.g., "xteink-x4-portrait"
    StringRef device_family;             // e.g., "xteink"
    StringRef device_model;              // e.g., "x4"

    u16  logical_width_px;              // Page blob width (480 for xteink-x4-portrait)
    u16  logical_height_px;             // Page blob height (800 for xteink-x4-portrait)
    u16  physical_width_px;             // Physical panel width (800 for xteink-x4)
    u16  physical_height_px;            // Physical panel height (480 for xteink-x4)

    u8   logical_orientation;           // 1=portrait, 2=landscape, 0=unknown
    i16  logical_to_physical_rotation;  // Clockwise degrees: 0, 90, 180, or 270
    u8   scan_order_hint;               // 1=row_major_left_to_right_top_to_bottom

    u32  supported_storage_pixel_formats; // Bit flags: bit0=GRAY1, bit1=GRAY2, bit2=GRAY4
    u32  native_output_pixel_formats;   // Bit flags for display controller

    u16  native_output_format_hint;     // Reserved for firmware
    u16  display_controller_hint;       // Reserved for firmware

    u16  native_grayscale_levels;       // e.g., 4 for X4 grayscale update path
    u16  panel_grayscale_levels;        // e.g., 4 for X4 grayscale update path
    u8   framebuffer_bits_per_pixel;    // e.g., 2 for GRAY2 default
    u16  output_gray_mapping;           // 1=linear_black_to_white, 2=linear_white_to_black
    u8   reserved_u8;

    u8   display_profile_hash[32];      // SHA-256 of this section (hash field zeroed during computation)
    u8   reserved[32];
};
```

**Pixel Format Bit Flags**:

| Bit | Flag | Description |
|-----|------|-------------|
| 0 | GRAY1_PACKED | 1-bit, 8 pixels per byte |
| 1 | GRAY2_PACKED | 2-bit, 4 pixels per byte |
| 2 | GRAY4_PACKED | 4-bit, 2 pixels per byte |

**output_gray_mapping values**:
- `0` = unknown
- `1` = linear_black_to_white (canonical BinBook: 0=black, 3=white)
- `2` = linear_white_to_black (Xteink XTH: 0=white, 3=black)
- `3` = xteink_xth_lut (specific LUT mapping)

### 3.8 LAYOUT_PROFILE Section (68 bytes + hashes)

```c
struct LayoutProfile {
    u16  full_page_width_px;           // Must equal DisplayProfile.logical_width_px
    u16  full_page_height_px;          // Must equal DisplayProfile.logical_height_px

    u16  reserved_header_height_px;    // Chrome area at top
    u16  reserved_footer_height_px;    // Chrome area at bottom

    u16  margin_top_px;
    u16  margin_right_px;
    u16  margin_bottom_px;
    u16  margin_left_px;

    u16  content_x_px;                // Derived: margin_left_px
    u16  content_y_px;                // Derived: margin_top_px + header_height
    u16  content_width_px;            // Derived: full_width - left_margin - right_margin
    u16  content_height_px;           // Derived: full_height - margins - header - footer

    u8   reading_direction;           // 1=left_to_right, 2=right_to_left
    u8   page_turn_direction;          // 1=next_is_right_or_down, 2=next_is_left_or_up
    u8   reserved_u8[2];

    u16  page_background_gray;         // Grayscale value for page background (0-3 for GRAY2)
    u16  reserved_u16;

    u32  layout_flags;                // Reserved for future use
    u8   layout_profile_hash[32];     // SHA-256 of this section
    u8   reserved[32];
};
```

**Content Box Derivation** (must match stored values):
```
content_x_px     = margin_left_px
content_y_px     = margin_top_px + reserved_header_height_px
content_width_px = full_page_width_px - margin_left_px - margin_right_px
content_height_px = full_page_height_px - margin_top_px - margin_bottom_px
                    - reserved_header_height_px - reserved_footer_height_px
```

### 3.9 READER_REQUIREMENTS Section (72 bytes)

```c
struct ReaderRequirements {
    u64  feature_flags;               // Bit flags for optional features used
    u64  required_reader_features;    // Bit flags firmware must support

    u32  required_storage_pixel_formats;  // Bit flags (e.g., 0x2 for GRAY2 default, 0x1 for GRAY1 fast mode)
    u16  required_output_grayscale_levels; // 2, 4, 16, or 0=unknown
    u16  fallback_output_policy;      // 1=reject, 2=allow_downquantize, 3=allow_dither_to_1bit

    u32  required_compression_methods; // Bit flags (e.g., 0x2 for RLE_PACKBITS)

    u16  max_stored_page_width_px;    // Maximum page width firmware must support
    u16  max_stored_page_height_px;   // Maximum page height firmware must support
    u32  max_uncompressed_page_size;  // Maximum decompressed blob size
    u32  max_compressed_page_size;    // Maximum compressed blob size

    u8   reserved[36];
};
```

**Feature Flags** (used in both `feature_flags` and `required_reader_features`):

| Bit | Name | Description |
|-----|------|-------------|
| 0 | has_page_crc32 | PageIndexEntry.page_crc32 is meaningful |
| 1 | has_file_crc32 | header.file_crc32 is meaningful |
| 2 | has_rendition_hash | RenditionIdentity contains valid hash |
| 3 | has_nav_index | NAV_INDEX section is present |
| 4 | has_source_identity | SOURCE_IDENTITY section is present |
| 5 | has_book_metadata | BOOK_METADATA section is present |
| 6 | has_font_policy | FONT_POLICY section is present |
| 7 | has_typography_policy | TYPOGRAPHY_POLICY section is present |
| 8 | has_image_policy | IMAGE_POLICY section is present |
| 9 | has_compression_policy | COMPRESSION_POLICY section is present |
| 10 | has_chrome_policy | CHROME_POLICY section is present |

**Required Reader Features**: Firmware must support all bits set here. If a required feature is missing, reject the file.

### 3.10 SOURCE_IDENTITY Section (88 bytes + hashes)

```c
struct SourceIdentity {
    u16  source_type;                 // 1=EPUB, 0=unknown
    u16  source_flags;                 // Reserved
    u64  original_file_size;           // Size of source file in bytes
    u8   original_md5[16];            // MD5 digest of source file
    u8   original_sha256[32];         // SHA-256 digest of source file
    StringRef original_filename;       // e.g., "book.epub"
    StringRef source_package_identifier; // e.g., ISBN
    u8   reserved[32];
};
```

### 3.11 BOOK_METADATA Section (56 bytes + hashes)

```c
struct BookMetadata {
    StringRef title;                   // Book title
    StringRef subtitle;                // Book subtitle (may be empty)
    StringRef author;                  // Author name(s)
    StringRef publisher;               // Publisher name
    StringRef language;                // Language tag (e.g., "en")
    StringRef series_name;             // Series name (may be empty)

    u32  series_index_milli;          // 1000 = book 1, 1500 = book 1.5, 0 = unused
    u32  metadata_flags;              // Reserved

    u8   reserved[32];
};
```

### 3.12 RENDITION_IDENTITY Section (80 bytes + hashes)

```c
struct RenditionIdentity {
    u8   rendition_hash[32];          // SHA-256 over canonical rendition bytes
    u8   display_profile_hash[32];    // Copy of DisplayProfile hash
    u8   layout_profile_hash[32];     // Copy of LayoutProfile hash
    u8   font_policy_hash[32];        // Copy of FontPolicy hash
    u8   typography_policy_hash[32];  // Copy of TypographyPolicy hash
    u8   image_policy_hash[32];       // Copy of ImagePolicy hash
    u8   compression_policy_hash[32]; // Copy of CompressionPolicy hash
    u8   chrome_policy_hash[32];      // Copy of ChromePolicy hash

    StringRef compiler_name;          // e.g., "binbook-python-poc"
    StringRef compiler_version;       // e.g., "0.1.0"

    u64  created_unix_time;           // Unix timestamp of compilation
    u8   reserved[32];
};
```

**rendition_hash computation**:
```
SHA-256(
    source_epub_sha256 (32 bytes) +
    display_profile_hash (32 bytes) +
    layout_profile_hash (32 bytes) +
    font_policy_hash (32 bytes) +
    typography_policy_hash (32 bytes) +
    image_policy_hash (32 bytes) +
    compression_policy_hash (32 bytes) +
    chrome_policy_hash (32 bytes) +
    u32(compiler_version_utf8_length) + compiler_version_utf8_bytes
)
```

### 3.13 FONT_POLICY Section (88 bytes + hashes)

```c
struct FontPolicy {
    u16  font_mode;                   // 1=default, 2=force, 0=unknown
    u16  font_flags;                  // Bit flags

    u8   font_sha256[32];             // SHA-256 of the font file used

    StringRef font_name;              // Font family name
    StringRef font_original_path;     // Path to font file (informational)
    StringRef renderer_name;           // Renderer used (e.g., "Pillow")

    u8   font_policy_hash[32];        // SHA-256 of this section
    u8   reserved[32];
};
```

**font_flags**:

| Bit | Name | Description |
|-----|------|-------------|
| 0 | force_custom_font | Ignore EPUB font-family |
| 1 | preserve_epub_font_family | Use EPUB fonts when available |
| 2 | allow_synthetic_bold | Synthesize bold if needed |
| 3 | allow_synthetic_italic | Synthesize italic if needed |
| 4 | preserve_epub_font_weight | Use EPUB font-weight |
| 5 | preserve_epub_font_style | Use EPUB font-style |

### 3.14 TYPOGRAPHY_POLICY Section (72 bytes + hashes)

```c
struct TypographyPolicy {
    u16  base_font_size_px;           // e.g., 24
    u16  minimum_font_size_px;        // e.g., 18
    u16  maximum_font_size_px;        // e.g., 0 = unused
    u16  font_weight;                 // CSS-like: 400=normal, 700=bold

    u32  font_scale_milli;            // 1000 = 100%, 1100 = 110%
    u32  line_height_milli;           // 1000 = 1.0, 1250 = 1.25

    u16  paragraph_spacing_before_px; // Pixels before paragraph
    u16  paragraph_spacing_after_px;  // Pixels after paragraph

    i32  character_spacing_milli_em;  // Signed: 0=normal, 50=+0.05em
    i32  word_spacing_milli_em;       // Signed: thousandths of font size

    u8   text_align;                  // 1=left, 2=center, 3=right, 4=justify, 5=preserve
    u8   hyphenation_policy;          // 1=disabled, 2=enabled, 3=preserve
    u8   widow_orphan_policy;         // 1=disabled, 2=basic, 3=preserve
    u8   reserved_u8;

    StringRef hyphenation_language;    // e.g., "en", "en-US"

    u32  typography_flags;            // Bit flags

    u8   typography_policy_hash[32];  // SHA-256 of this section
    u8   reserved[32];
};
```

**font_scale_milli**: 1000 = 100%, 1100 = 110%. Applied at compile time, not runtime.

**typography_flags**:

| Bit | Name | Description |
|-----|------|-------------|
| 0 | enforce_minimum_font_size | |
| 1 | enforce_maximum_font_size | |
| 2 | force_line_height | |
| 3 | preserve_epub_line_height | |
| 4 | force_character_spacing | |
| 5 | preserve_epub_character_spacing | |
| 6 | force_word_spacing | |
| 7 | preserve_epub_word_spacing | |
| 8 | force_text_align | |
| 9 | preserve_epub_text_align | |
| 10 | hyphenation_enabled | |
| 11 | widow_orphan_control_enabled | |

### 3.15 IMAGE_POLICY Section (52 bytes + hashes)

```c
struct ImagePolicy {
    u16  image_page_mode;             // 1=separate_page
    u16  image_pixel_format;          // 1=GRAY1, 2=GRAY2, 4=GRAY4
    u16  image_fit_mode;               // 1=fit_inside_no_crop, 2=crop_to_fill, 3=split_long
    u16  image_rotation_policy;       // 0=never, 1=rotate_if_better_fit

    u16  image_dither_method;          // 0=none, 1=floyd_steinberg, 2=ordered_bayer
    u16  image_background_gray;        // Grayscale value for image background
    u16  alpha_background_gray;        // Grayscale value for alpha flattening
    u16  dithering_strength_milli;     // 1000 = normal strength

    u16  skip_tiny_images_below_px;   // Skip images smaller than this
    u16  max_source_image_width_px;    // Max source image width
    u16  max_source_image_height_px;   // Max source image height
    u16  reserved_u16;

    u32  image_flags;                  // Reserved

    u8   image_policy_hash[32];        // SHA-256 of this section
    u8   reserved[32];
};
```

### 3.16 COMPRESSION_POLICY Section (44 bytes + hashes)

```c
struct CompressionPolicy {
    u16  default_compression_method;   // 0=NONE, 1=RLE_PACKBITS
    u32  allowed_compression_methods;  // Bit flags

    u16  block_model;                  // 1=whole_page_blob, 2=strip_blobs
    u16  strip_height_px;              // For strip model (reserved)

    u32  compression_flags;            // Reserved

    u8   compression_policy_hash[32];  // SHA-256 of this section
    u8   reserved[32];
};
```

### 3.17 CHROME_POLICY Section (40 bytes + hashes)

```c
struct ChromePolicy {
    u8   reserved[4];

    u16  progress_bar_mode;            // 0=none, 1=book_progress, 2=chapter_progress
    u16  progress_bar_position;        // 1=top, 2=bottom

    u32  chrome_flags;                // Bit flags

    u8   chrome_policy_hash[32];       // SHA-256 of this section
    u8   reserved2[32];
};
```

**chrome_flags**:

| Bit | Name | Description |
|-----|------|-------------|
| 0 | dark_mode_negative | Runtime hint: invert chrome at display time |
| 1 | show_page_numbers | |
| 2 | show_page_number_x_of_y | Format "X of Y" |
| 3 | show_percent | Show progress percentage |
| 4 | show_chapter_marks | |
| 5 | show_chapter_title | |

**Note**: `dark_mode_negative` does NOT invert page blobs. Page blobs are always canonical (light mode). The firmware applies inversion at display time if needed.

### 3.18 PAGE_INDEX Section (Record-Based)

Array of `PageIndexEntry` records. Count = `section_entry.record_count`. Size per entry = `header.page_index_entry_size` (76 bytes for v0.1).

```c
struct PageIndexEntry {
    u32  page_number;                 // 0-based page index
    u16  page_kind;                   // 1=TEXT, 2=IMAGE, 3=MIXED_RESERVED (reject in v0.1)
    u16  pixel_format;                // 1=GRAY1, 2=GRAY2, 4=GRAY4

    u16  compression_method;          // 0=NONE, 1=RLE_PACKBITS
    u16  update_hint;                 // 0=default, 1=full_refresh, 2=partial_refresh_ok
    u32  page_flags;                 // Reserved

    u64  relative_blob_offset;        // Relative to header.page_data_offset
    u32  compressed_size;             // Size of compressed blob
    u32  uncompressed_size;          // Size of decompressed pixels
    u32  page_crc32;                 // CRC32 of compressed blob (0 = not computed)

    u16  stored_width;                // Pixel width of stored blob
    u16  stored_height;               // Pixel height of stored blob
    u16  placement_x;                 // X offset within content box (usually 0)
    u16  placement_y;                 // Y offset within content box (usually 0)

    u32  source_spine_index;          // EPUB spine index (UINT32_MAX = none)
    u32  chapter_nav_index;           // NAV_INDEX index (UINT32_MAX = none)

    u32  progress_start_ppm;          // Progress at start of page (0 to 1,000,000)
    u32  progress_end_ppm;            // Progress at end of page

    u8   reserved[16];
};
```

**Absolute blob offset computation**:
```c
absolute_offset = header.page_data_offset + page.relative_blob_offset
```

**progress values**: Parts per million (0 to 1,000,000). Must be monotonically non-decreasing across pages.

### 3.19 NAV_INDEX Section (Record-Based)

Array of `NavIndexEntry` records. Count = `section_entry.record_count`. Size per entry = `header.nav_index_entry_size` (48 bytes for v0.1).

```c
struct NavIndexEntry {
    u32  nav_index;                   // 0-based nav entry index
    u16  nav_type;                    // 1=COVER, 2=TOC, 3=CHAPTER, 4=SECTION, 5=LANDMARK
    u16  level;                       // Nesting level (0 = top-level)

    StringRef title;                  // Display title for this nav entry
    StringRef source_href;            // Original EPUB href (informational)

    u32  source_spine_index;          // EPUB spine index (UINT32_MAX = none)
    u32  rendered_page_number;        // Page number this nav entry points to

    u32  parent_nav_index;           // Parent in nav tree (UINT32_MAX = none)
    u32  first_child_nav_index;       // First child (UINT32_MAX = none)
    u32  next_sibling_nav_index;      // Next sibling (UINT32_MAX = none)

    u32  nav_flags;                   // Reserved
};
```

**Note**: UINT32_MAX = `0xFFFFFFFF`. Used for "none" or "no parent/child/sibling".

### 3.20 PAGE_DATA Section

Raw concatenated compressed page blobs. No page-local headers.

```
PAGE_DATA:
├── [compressed page 0 blob] ─── referenced by PAGE_INDEX[0]
├── [compressed page 1 blob]
├── [compressed page 2 blob]
└── ...
```

Each blob is:
1. Read from `header.page_data_offset + page.relative_blob_offset`
2. Validate size = `page.compressed_size`
3. Optionally validate CRC32 if `page.page_crc32 != 0`
4. Decompress using `page.compression_method`
5. Verify decompressed size = `page.uncompressed_size`

---

## 4. Pixel Formats and Packing

### 4.1 Storage Order

All page blobs are stored in **row-major order**:
- Top row first
- Bottom row last
- Within each row: left-to-right
- Logical origin (0,0) = top-left of content area

### 4.2 Row Padding

Each row is padded to the nearest whole byte boundary:

| Format | Pixels/Byte | Row Size |
|--------|-------------|----------|
| GRAY1_PACKED | 8 | `ceil(width / 8)` bytes |
| GRAY2_PACKED | 4 | `ceil(width / 4)` bytes |
| GRAY4_PACKED | 2 | `ceil(width / 2)` bytes |

**Rule**: Unused bits in the last byte of each row must be zero-filled by the writer. Readers must ignore unused bits.

### 4.3 GRAY1_PACKED (1-bit, 8 pixels/byte)

```
Byte layout (leftmost pixel in highest bit):
  bit 7 = pixel 0 (leftmost)
  bit 6 = pixel 1
  bit 5 = pixel 2
  bit 4 = pixel 3
  bit 3 = pixel 4
  bit 2 = pixel 5
  bit 1 = pixel 6
  bit 0 = pixel 7 (rightmost)
```

Canonical values: `0 = black, 1 = white`

### 4.4 GRAY2_PACKED (2-bit, 4 pixels/byte)

```
Byte layout (leftmost pixel in highest bits):
  bits 7..6 = pixel 0 (leftmost)
  bits 5..4 = pixel 1
  bits 3..2 = pixel 2
  bits 1..0 = pixel 3 (rightmost)
```

Canonical values:
- `0b00` (0) = black
- `0b01` (1) = dark gray
- `0b10` (2) = light gray
- `0b11` (3) = white

**Example**: For a 480 px wide page:
- Row size = 480 / 4 = 120 bytes (exact, no padding needed)

### 4.5 GRAY4_PACKED (4-bit, 2 pixels/byte)

```
Byte layout:
  bits 7..4 = pixel 0 (leftmost, high nibble)
  bits 3..0 = pixel 1 (rightmost, low nibble)
```

Canonical values: `0 = black, 15 = white`

---

## 5. Compression

### 5.1 RLE_PACKBITS (compression_method = 1)

A simple run-length encoding scheme.

**Control byte interpretation**:

| Control Byte | Meaning |
|--------------|---------|
| 0..127 | Literal run: copy `(control + 1)` bytes from input |
| 128..255 | Repeated run: repeat `(control & 0x7F) + 1` bytes of the next input byte |

**Important**: Unlike standard Apple PackBits, **0x80 is treated as a 1-byte repeat run** (not a no-op):
- Standard PackBits: 0x80 = no-op
- BinBook PackBits: 0x80 = repeat 1 byte

**Encode example**:
```
Input:  [0x01, 0x02, 0x03, 0x03, 0x03, 0x04]
Output: [0x02, 0x01, 0x02, 0x03, 0x82, 0x04]
         ^^^^  ^^^^^^^^^^  ^^^^^^^^^^^  ^^^^
         lit3  3 literals   repeat 3x     lit1
```

**Decode algorithm** (pseudocode):
```c
while (input not exhausted) {
    control = read_u8();
    if (control <= 127) {
        // Literal run: copy (control + 1) bytes
        count = control + 1;
        copy count bytes from input to output;
    } else {
        // Repeated run: repeat (control & 0x7F) + 1 bytes
        count = (control & 0x7F) + 1;
        byte_to_repeat = read_u8();
        for (i = 0; i < count; i++) {
            output.write(byte_to_repeat);
        }
    }
}
```

---

## 6. CRC32 and Hashing

### 6.1 CRC32 (IEEE 802.3 / PKZIP)

**Parameters**:
- Polynomial: `0xEDB88320`
- Initial value: `0xFFFFFFFF`
- Final XOR: `0xFFFFFFFF`

**CRC fields in format**:
- `header.file_crc32`: Entire file (0 to `header.file_size`), with `file_crc32` field set to 0 during computation
- `header.header_crc32`: 256-byte header, with `header_crc32` field set to 0 during computation
- `SectionEntry.crc32`: Section data from `offset` to `offset + length`
- `PageIndexEntry.page_crc32`: Compressed page blob

**If CRC field is 0**: Checksum not computed, validation skipped.

### 6.2 SHA-256 Hashes

All `*_hash[32]` fields are **raw 32-byte SHA-256 digests** (not hex strings).

**Section hash computation**:
```
hash = SHA-256(section_data with hash field zeroed out)
```

**Rendition hash**: See section 3.12 for the canonical byte sequence.

---

## 7. Decoding Algorithm

Pseudocode for firmware/reader:

```c
// Step 1: Read and validate header
header = read_bytes(256);
if (memcmp(header.magic, "BINBOOK\0", 8) != 0) {
    error("invalid magic");
}
if (header.version_major != SUPPORTED_MAJOR) {
    error("unsupported major version");
}
if (header.section_table_entry_size != EXPECTED_SECTION_ENTRY_SIZE) {
    error("unsupported section entry size");
}

// Step 2: Read section table
sections = {};
for (i = 0; i < header.section_count; i++) {
    offset = header.section_table_offset + i * header.section_table_entry_size;
    entry = read_SectionEntry(offset);
    sections[entry.section_id] = entry;
}

// Step 3: Validate required sections
required = {STRING_TABLE, DISPLAY_PROFILE, LAYOUT_PROFILE, READER_REQUIREMENTS,
            SOURCE_IDENTITY, BOOK_METADATA, RENDITION_IDENTITY,
            FONT_POLICY, TYPOGRAPHY_POLICY, IMAGE_POLICY, COMPRESSION_POLICY,
            CHROME_POLICY, PAGE_INDEX, NAV_INDEX, PAGE_DATA};
for (id in required) {
    if (id not in sections) error("missing required section");
}

// Step 4: Validate reader requirements
rr_data = read_section(READER_REQUIREMENTS);
required_features = read_u64(rr_data + 8);
if (required_features & ~SUPPORTED_FEATURES) {
    error("unsupported required features");
}

// Step 5: Find page index
pi_section = sections[PAGE_INDEX];
pages = [];
for (i = 0; i < pi_section.record_count; i++) {
    offset = pi_section.offset + i * header.page_index_entry_size;
    pages.append(read_PageIndexEntry(offset));
}

// Step 6: Decode a specific page
page = pages[page_number];
absolute_offset = header.page_data_offset + page.relative_blob_offset;
compressed_blob = read_bytes(absolute_offset, page.compressed_size);

if (page.page_crc32 != 0) {
    if (crc32(compressed_blob) != page.page_crc32) error("page CRC mismatch");
}

decompressed = decompress(compressed_blob, page.compression_method);
if (len(decompressed) != page.uncompressed_size) error("size mismatch");

// Step 7: Convert pixels to display format
// For xteink-x4-portrait, default pages are GRAY2_PACKED logical portrait pixels.
// Firmware rotates logical pixels into the physical framebuffer using
// DISPLAY_PROFILE.logical_to_physical_rotation.
```

---

## 8. Encoding Guidance

### 8.1 Page Data Alignment

The compiler should align `page_data_offset` to a useful boundary (default: 64 KiB):

```python
page_data_offset = align_up(end_of_metadata, 65536)
```

**Firmware must not hardcode 64 KiB** — always read `header.page_data_offset`.

### 8.2 Reserved Bytes

All reserved bytes must be zero-filled by the writer. This allows future extensions without breaking existing readers.

### 8.3 Section Table Order

While the section table is authoritative (readers must use it), the recommended physical order is:
1. Header
2. Section table
3. STRING_TABLE
4. Policy/metadata sections (any order)
5. NAV_INDEX
6. PAGE_INDEX
7. Zero padding
8. PAGE_DATA

### 8.4 String Table Construction

1. Maintain a list of unique strings
2. Assign each string a sequential offset within the table
3. Create `StringRef { offset, length }` for each
4. Concatenate all strings (UTF-8 encoded) into the STRING_TABLE section

---

## 9. Profile Definitions

### 9.1 xteink-x4-portrait

**Target Device**: Xteink X4 e-reader in portrait orientation.

| Property | Value |
|----------|-------|
| `profile_id` | "xteink-x4-portrait" |
| Logical dimensions | 480 × 800 px |
| Physical dimensions | 800 × 480 px (device is physically landscape) |
| Logical orientation | Portrait |
| Logical-to-physical rotation | 90 degrees clockwise |
| Default pixel format | GRAY2_PACKED |
| Supported lower format | GRAY1_PACKED |
| Grayscale levels | 4 for GRAY2, 2 for GRAY1 |
| GRAY2 row size | 120 bytes (480 / 4) |
| GRAY1 row size | 60 bytes (480 / 8) |
| Compression | RLE_PACKBITS |

**Logical-to-physical rotation mapping for 90 degrees clockwise**:
```
physical_x = logical_y
physical_y = logical_width_px - 1 - logical_x
```

The stored page blob remains logical portrait `480x800` row-major data. Firmware rotates into the physical `800x480` display framebuffer.

The default X4 output for BinBook v0.1 is canonical `GRAY2_PACKED`. A compiler may emit `GRAY1_PACKED` for an explicit lower-quality or fast-mode configuration, but readers must not assume all X4 books are 1-bit.

**Canonical GRAY2 values**:
- `0 = black`
- `1 = dark gray`
- `2 = light gray`
- `3 = white`

**Canonical GRAY1 values**:
- `0 = black`
- `1 = white`

**X4 display-backend note**: X4 firmware should convert from canonical BinBook `GRAY2_PACKED` into its native grayscale buffer/update sequence.

---

## 10. Validation Rules

Readers/firmware should validate:

### File-Level
- [ ] Magic matches "BINBOOK\0"
- [ ] `version_major` is supported
- [ ] `version_minor` is acceptable (or features used are supported)
- [ ] `file_size` is 0 or actual file size >= `file_size`
- [ ] `section_table_offset` + `section_table_length` <= file size
- [ ] `section_table_entry_size` is supported (40 for v0.1)

### Section-Level
- [ ] All required sections present
- [ ] All section offsets/lengths within file bounds
- [ ] `PAGE_DATA` offset/length match header
- [ ] Section CRC32s valid (if nonzero)

### Page-Level
- [ ] `page_index_entry_size` is supported (76 for v0.1)
- [ ] `nav_index_entry_size` is supported (48 for v0.1)
- [ ] All page blobs within `PAGE_DATA` bounds
- [ ] No overlapping page blobs
- [ ] `pixel_format` is supported
- [ ] `compression_method` is supported
- [ ] `page_kind` != MIXED_RESERVED (3) in v0.1
- [ ] `progress_start_ppm` <= `progress_end_ppm`
- [ ] Progress is monotonically non-decreasing
- [ ] Page dimensions match layout profile

### String-Level
- [ ] All `StringRef` offsets/lengths within STRING_TABLE bounds
- [ ] Strings are valid UTF-8 (or handled gracefully)

### Profile-Specific (xteink-x4-portrait)
- [ ] Pages are `GRAY2_PACKED` by default, or `GRAY1_PACKED` only when the file/profile explicitly declares the lower-quality storage format
- [ ] `READER_REQUIREMENTS.required_storage_pixel_formats` matches the emitted page format
- [ ] `DISPLAY_PROFILE.logical_width/height` = (480, 800)
- [ ] `DISPLAY_PROFILE.physical_width/height` = (800, 480)
- [ ] `DISPLAY_PROFILE.logical_to_physical_rotation` = 90
- [ ] `LAYOUT_PROFILE.full_page_width/height` matches display profile

---

## 11. Versioning

| Field | v0.1 Value |
|-------|------------|
| `version_major` | 0 |
| `version_minor` | 1 |
| `header_size` | 256 |
| `section_table_entry_size` | 40 |
| `page_index_entry_size` | 76 |
| `nav_index_entry_size` | 48 |

**Rules**:
- Readers **must reject** files where `version_major` != supported_major
- Readers **should not reject** files solely because `version_minor` > supported_minor
- Readers **may reject** same-major newer-minor files if they use unsupported features/sections

---

## 12. Examples

### 12.1 Minimal Valid File (Hex Dump)

A minimal `.binbook` file with:
- 1 page (GRAY2_PACKED, 480×800, RLE compressed)
- No navigation entries
- Minimal metadata

**Header** (first 256 bytes):
```
00000000: 42 49 4e 42 4f 4f 4b 00  00 00 01 00 00 01 00 00  |BINBOOK.........|
00000010: 00 00 00 00 00 00 00 00  00 01 00 00 00 00 00 00  |................|
00000020: 28 01 00 00 10 00 4c 01  04 00 04 00 00 00 10 00  |(.....L.........|
00000030: 00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  |................|
00000040: 00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  |................|
... (remainder of header is zero-filled to 256 bytes)
```

**Legend**:
- Bytes 0-7: `magic` = "BINBOOK\0"
- Bytes 8-9: `version_major` = 0
- Bytes 10-11: `version_minor` = 1
- Bytes 12-13: `header_size` = 256
- Bytes 14-15: `header_flags` = 0
- Bytes 16-23: `file_size` = 0 (skip validation)
- Bytes 24-31: `section_table_offset` = 256 (0x100)
- Bytes 32-35: `section_table_length` = 296 (0x128 = 40 bytes × 7 sections + 16 bytes for PAGE_DATA)
- Bytes 36-37: `section_table_entry_size` = 40
- Bytes 38-39: `section_count` = 15 (all required sections)
- Bytes 40-41: `page_index_entry_size` = 76
- Bytes 42-43: `nav_index_entry_size` = 48

*(Note: Full hex dump would be ~100+ lines. The above shows header structure. In practice, use `binbook inspect` to see a real file's structure.)*

### 12.2 StringRef Example

STRING_TABLE contains: `"xteink-x4-portrait"` (20 bytes) starting at offset 0.

```
StringRef for "xteink-x4-portrait":
  offset = 0x00000000
  length = 0x00000014 (20 decimal)
```

In binary (little-endian):
```
00 00 00 00  14 00 00 00
```

### 12.3 GRAY1_PACKED Pixel Example

An 8×1 pixel row:

Canonical values:
```
Black White White Black White Black Black White
  0     1     1    0     1    0     0     1
```

Packed into 1 byte:
```
bit 7..0 = 0 1 1 0 1 0 0 1 = 0x69
```

### 12.4 GRAY2_PACKED Pixel Example

A 4×4 pixel image (compressed for clarity):

Canonical values:
```
Black  DarkGray  LightGray  White
   0       1         2         3
```

Packed into 4 bytes (1 byte per 4 pixels):
```
Byte 0: pixels 0-3 of row 0: 00 01 10 11 = 0x1B
Byte 1: pixels 0-3 of row 1: 01 10 11 00 = 0x6C
Byte 2: pixels 0-3 of row 2: 10 11 00 01 = 0xB1
Byte 3: pixels 0-3 of row 3: 11 00 01 10 = 0xC6
```

---

## 13. Differences from EPUB

| Aspect | EPUB | BinBook |
|--------|------|---------|
| Reflowable | Yes | No |
| Runtime rendering | Yes (requires engine) | No (pre-rendered) |
| Font changing | Yes | No (recompile needed) |
| File size | Larger (HTML+CSS+fonts) | Smaller (compressed pixels) |
| RAM required | High (parse+render) | Low (decompress+display) |
| Format | ZIP+XML | Single binary file |

---

## Appendix A: Constants Reference

### Pixel Formats
```c
#define GRAY1_PACKED  1
#define GRAY2_PACKED  2
#define GRAY4_PACKED  4
```

### Page Kinds
```c
#define PAGE_KIND_TEXT   1
#define PAGE_KIND_IMAGE  2
// 3 = MIXED_RESERVED (reject in v0.1)
```

### Compression Methods
```c
#define COMPRESS_NONE         0
#define COMPRESS_RLE_PACKBITS 1
```

### Orientation
```c
#define ORIENTATION_UNKNOWN 0
#define ORIENTATION_PORTRAIT 1
#define ORIENTATION_LANDSCAPE 2
```

### UINT32_MAX
```c
#define UINT32_MAX 0xFFFFFFFF
// Used for: no spine index, no chapter, no parent/child/sibling
```
