# BinBook Format Specification

Language-agnostic specification for the BinBook compiled raster-book format.
Designed for low-RAM e-ink and embedded display devices.

---

## 1. Introduction

BinBook is a **compiled raster-book container**. It stores pre-rendered page content as compressed per-plane blobs. The format is:

- **Non-reflowable**: Changing font, margins, or layout requires recompilation
- **Pre-rendered**: All text rendering, pagination, and image processing happens at compile time
- **Binary-native**: No JSON, CBOR, protobuf, or MessagePack in the file format
- **Low-RAM friendly**: Fixed-size records, optional sections, seekable string table

### Intended Use

```
Compiler (desktop/cloud):
  EPUB → rendered pages → per-plane pixels → compressed plane blobs → .binbook

Firmware/reader (embedded):
  .binbook → page index → plane directory → decompress needed planes → display
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
    u16  reserved0;                  // 0
    u16  reserved1;                  // 0
    u16  header_size;                // 256
    u16  header_flags;              // 0 (reserved for future use)

    u64  file_size;                  // Total file size in bytes (0 = skip validation)
    u64  section_table_offset;       // Absolute offset to section table
    u32  section_table_length;       // Length of section table in bytes
    u16  section_table_entry_size;   // 40
    u16  section_count;              // Number of section table entries

    u16  page_index_entry_size;      // 128
    u16  nav_index_entry_size;       // 48

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
- **reserved0/reserved1**: Writers set these to 0. Readers ignore them.
- **header_flags**: Writers set this to 0. Readers ignore non-zero values.
- **file_size**: If nonzero, readers should validate the actual file is at least this size
- **section_table_offset**: Typically 256 (right after the header)
- **section_table_entry_size**: Must be 40
- **page_index_entry_size**: Must be 128
- **nav_index_entry_size**: Must be 48
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
| 40 | PAGE_INDEX | Yes | Yes (128 bytes each) |
| 41 | NAV_INDEX | Yes | Yes (48 bytes each) |
| 42 | PAGE_LABELS_RESERVED | No | Reserved |
| 43 | CHAPTER_INDEX | Yes | Yes (32 bytes each) |
| 44 | PAGE_CHUNK_INDEX | Yes | Yes (24 bytes each) |
| 45 | PAGE_TRANSITION_INDEX | Yes | Yes (24 bytes each) |
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

    u32  supported_storage_pixel_formats; // Bit flags: bit0=GRAY1, bit1=GRAY2, bit2=GRAY4, bit3=RGB565, bit4=RGB888, bit5=RGBA8888
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
| 3 | RGB565 | 16-bit, 5-6-5, little-endian |
| 4 | RGB888 | 24-bit, 8-8-8 |
| 5 | RGBA8888 | 32-bit, 8-8-8-8 |

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

    u32  required_compression_methods; // Bit flags (e.g., 0x2 for RLE_PACKBITS, 0x4 for LZ4)

    u16  max_stored_page_width_px;    // Maximum page width firmware must support
    u16  max_stored_page_height_px;   // Maximum page height firmware must support
    u32  max_uncompressed_page_size;  // Maximum decompressed size of the largest single plane
    u32  max_compressed_page_size;    // Maximum compressed size of any single plane blob

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
    StringRef reserved_compiler_info;

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
    chrome_policy_hash (32 bytes)
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

Array of `PageIndexEntry` records. Count = `section_entry.record_count`. Size per entry = `header.page_index_entry_size` (128 bytes).

```c
struct PageIndexEntry {
    u32  page_number;                 // 0-based page index
    u16  page_kind;                   // 1=TEXT, 2=IMAGE, 3=MIXED_RESERVED
    u16  pixel_format;                // 1=GRAY1, 2=GRAY2, 4=GRAY4, 8=RGB565, 16=RGB888, 32=RGBA8888

    u16  compression_method;          // Default: 0=NONE, 1=RLE_PACKBITS, 2=LZ4
    u16  update_hint;                 // 0=default, 1=full_refresh, 2=partial_refresh_ok
    u32  page_flags;                  // bit 0: per_plane_compression (1=each plane uses its own method)
                                      // bits 1-31: reserved

    u32  page_crc32;                  // CRC32 over all plane blobs (0 = not computed)

    u16  stored_width;                // Pixel width
    u16  stored_height;               // Pixel height
    u16  placement_x;                 // X offset within content box (usually 0)
    u16  placement_y;                 // Y offset within content box (usually 0)

    u32  source_spine_index;          // EPUB spine index (UINT32_MAX = none)
    u32  chapter_nav_index;           // NAV_INDEX index (UINT32_MAX = none)

    u32  progress_start_ppm;          // Progress at start of page (0 to 1,000,000)
    u32  progress_end_ppm;            // Progress at end of page

    // --- Inline plane directory (32 bytes) ---
    u8   plane_bitmap;                // Which planes are stored (see section 5.3)
    u8   plane_compression[4];        // Per-plane compression (only if page_flags bit 0)
    u8   plane_dir_padding[3];        // Alignment to 4-byte boundary
    u32  offset_plane_0;              // Byte offset from PAGE_DATA start
    u32  size_plane_0;                // Compressed size in bytes
    u32  offset_plane_1;              // Byte offset from PAGE_DATA start
    u32  size_plane_1;                // Compressed size in bytes
    u32  offset_plane_2;              // Byte offset from PAGE_DATA start
    u32  size_plane_2;                // Compressed size in bytes
    u32  offset_plane_3;              // Byte offset from PAGE_DATA start (future: delta plane)
    u32  size_plane_3;                // Compressed size in bytes (future)

    u8   reserved[44];                // Future use
};
```

Total: 128 bytes per entry.

**progress values**: Parts per million (0 to 1,000,000). Must be monotonically non-decreasing across pages.

**Per-plane compression**: When `page_flags` bit 0 is set, each plane uses its own compression method from `plane_compression[4]`. When clear, all planes use `compression_method`.

**Plane offsets**: `offset_plane_N` is relative to `header.page_data_offset`. Plane blob offsets must be 4-byte aligned within PAGE_DATA.

### 3.19 NAV_INDEX Section (Record-Based)

Array of `NavIndexEntry` records. Count = `section_entry.record_count`. Size per entry = `header.nav_index_entry_size` (48 bytes).

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

### 3.20 CHAPTER_INDEX Section (Record-Based)

Array of `ChapterIndexEntry` records. Count = `section_entry.record_count`. Size per entry = `section_entry.entry_size` (32 bytes).

This is the reader-facing selectable chapter table. It contains a compact,
directly indexable subset of navigation entries that should appear in a chapter
picker. Readers use this section for fast chapter list windows and selected
chapter lookup instead of scanning `NAV_INDEX`.

```c
struct ChapterIndexEntry {
    u32  chapter_index;              // 0-based chapter row index
    u32  nav_index;                  // Corresponding NAV_INDEX record

    StringRef title;                 // Display title for this chapter row

    u32  rendered_page_number;       // Page number this chapter points to

    u16  level;                      // Nesting level copied from NAV_INDEX
    u16  nav_type;                   // 3=CHAPTER, 4=SECTION

    u32  source_spine_index;         // EPUB spine index (UINT32_MAX = none)
    u32  chapter_flags;              // Reserved
};
```

Rules:

- `chapter_index` values are contiguous from 0.
- `rendered_page_number` must be within `PAGE_INDEX.record_count`.
- `nav_type` must be selectable by the reader. The current writer emits
  `CHAPTER` and `SECTION` entries.
- `title` must reference `STRING_TABLE` and may be reused from the matching
  `NAV_INDEX` entry.

### 3.21 PAGE_CHUNK_INDEX Section (Record-Based)

Array of `PageChunkIndexEntry` records. Count =
`section_entry.record_count`. Size per entry = `section_entry.entry_size`
(24 bytes).

This section gives readers direct access to independently compressed chunks
inside a page plane. It exists so constrained firmware can decode and stream a
bounded number of display rows without loading or scanning a full plane.

For `xteink-x4-portrait`, every page stores 3 planes and every plane stores 30
chunks, so each page has 90 chunk records.

```c
struct PageChunkIndexEntry {
    u32 page_number;        // PAGE_INDEX page_number
    u8  plane_slot;         // 0=MSB/red, 1=LSB/black, 2=BW, 3=future
    u8  chunk_index;        // 0-based chunk index within the plane
    u16 row_start;          // First physical row in this chunk
    u16 row_count;          // Number of physical rows in this chunk
    u16 reserved0;          // Must be 0
    u32 page_data_offset;   // Byte offset from PAGE_DATA start
    u32 compressed_size;    // Compressed chunk size in bytes
    u32 uncompressed_size;  // Decompressed chunk size in bytes
};
```

Rules:

- Records are sorted by `page_number`, then `plane_slot`, then `chunk_index`.
- `page_data_offset` is relative to `header.page_data_offset`.
- Every chunk range must be fully contained inside its parent plane blob from
  `PAGE_INDEX`.
- For `xteink-x4-portrait`, `row_count` is 16, `uncompressed_size` is 1600,
  and each plane has chunk indices 0 through 29.

### 3.22 PAGE_TRANSITION_INDEX Section (Record-Based)

Array of `PageTransitionIndexEntry` records. Count =
`section_entry.record_count`. Size per entry = `section_entry.entry_size`
(24 bytes).

This section is a compiler-generated fast-path for page transitions. It lets
firmware perform chunk-level differential BW partial refresh without comparing
previous and current page chunks on-device.

For `xteink-x4-portrait`, writers emit records for adjacent page transitions in
both directions: `N -> N+1` and `N+1 -> N`.

```c
struct PageTransitionIndexEntry {
    u32 from_page_number;      // Previous displayed page
    u32 to_page_number;        // Target page
    u32 changed_chunk_mask;    // Bit N set when BW chunk N differs
    u16 first_changed_chunk;   // First changed chunk, or 0 if none
    u16 changed_chunk_count;   // Contiguous window covering changed chunks
    u16 flags;                 // Must be 0
    u16 reserved0;             // Must be 0
    u32 reserved1;             // Must be 0
};
```

Rules:

- `from_page_number` and `to_page_number` must reference existing pages.
- `changed_chunk_mask` uses bit `chunk_index`; X4 uses bits 0 through 29.
- If `changed_chunk_mask == 0`, then `first_changed_chunk == 0` and
  `changed_chunk_count == 0`.
- Firmware may use either the exact mask or the contiguous
  `first_changed_chunk`/`changed_chunk_count` window.
- Non-adjacent jumps need not have transition records. Firmware may fall back to
  full-screen BW differential partial refresh.

### 3.23 PAGE_DATA Section

Raw concatenated plane blobs. No page-local headers — the page index entry's
plane directory is the authority.

```
PAGE_DATA:
├── [plane 0 blob page 0]    ← PAGE_INDEX[0].offset_plane_0
├── [plane 1 blob page 0]    ← PAGE_INDEX[0].offset_plane_1
├── [plane 2 blob page 0]    ← PAGE_INDEX[0].offset_plane_2
├── [plane 0 blob page 1]    ← PAGE_INDEX[1].offset_plane_0
├── ...
```

Each blob is:
1. Read from `header.page_data_offset + page.offset_plane_N`
2. Validate size = `page.size_plane_N`
3. Optionally validate CRC32 if `page.page_crc32 != 0`
4. Decompress using per-plane or page-default compression method

If `PAGE_CHUNK_INDEX` records exist for the blob, constrained readers may read
and decompress each chunk independently instead of reading the full blob. Chunk
records do not add headers inside `PAGE_DATA`; they are metadata pointing at
subranges of the raw plane blobs.

Plane blob offsets must be 4-byte aligned within PAGE_DATA. Writers pad
between blobs with zero bytes.

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
| RGB565 | 2 bytes/pixel | `width * 2` bytes |
| RGB888 | 3 bytes/pixel | `width * 3` bytes |
| RGBA8888 | 4 bytes/pixel | `width * 4` bytes |

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

### 4.6 RGB565 (16-bit, 5-6-5)

Each pixel is 2 bytes, little-endian:
```
bits 15..11 = red (5 bits)
bits 10..5  = green (6 bits)
bits 4..0   = blue (5 bits)
```

Canonical values: standard RGB565 mapping.

### 4.7 RGB888 (24-bit)

Each pixel is 3 bytes:
```
byte 0 = red
byte 1 = green
byte 2 = blue
```

### 4.8 RGBA8888 (32-bit)

Each pixel is 4 bytes:
```
byte 0 = red
byte 1 = green
byte 2 = blue
byte 3 = alpha
```

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

## 5.1 Compression Methods

| Value | Method       | Best for |
|-------|--------------|----------|
| 0     | NONE         | Already-native pixel data |
| 1     | RLE_PACKBITS | Uniform content, e-paper MSB/LSB planes |
| 2     | LZ4          | Textured/dithered content, color, BW planes |

**Per-plane compression**: When `page_flags` bit 0 is set, each plane uses its own method from `plane_compression[4]`. When clear, all planes use `compression_method`.

### 5.2 Decompressed Plane Sizes

The firmware computes decompressed plane sizes from `pixel_format`,
`stored_width`, and `stored_height`. No `uncompressed_size` field is needed.

| Pixel Format | Plane | Decompressed Size |
|-------------|-------|-------------------|
| GRAY1_PACKED | 0 (MSB) | `stored_width / 8 * stored_height` |
| GRAY1_PACKED | 1 (LSB) | `stored_width / 8 * stored_height` |
| GRAY1_PACKED | 2 (BW)  | `stored_width / 8 * stored_height` |
| GRAY2_PACKED | 0 (MSB) | `stored_width / 8 * stored_height` |
| GRAY2_PACKED | 1 (LSB) | `stored_width / 8 * stored_height` |
| GRAY2_PACKED | 2 (BW)  | `stored_width / 8 * stored_height` |
| GRAY4_PACKED | 0 (MSB) | `stored_width / 4 * stored_height` |
| GRAY4_PACKED | 1 (LSB) | `stored_width / 4 * stored_height` |
| GRAY4_PACKED | 2 (BW)  | `stored_width / 4 * stored_height` |
| RGB565       | 0 (full) | `stored_width * 2 * stored_height` |
| RGB888       | 0 (full) | `stored_width * 3 * stored_height` |
| RGBA8888     | 0 (full) | `stored_width * 4 * stored_height` |

For e-paper GRAY2, each of the 3 planes (MSB, LSB, BW) is a 1-bit
framebuffer. For `xteink-x4-portrait`, planes are stored in physical SSD1677
order: `800 / 8 * 480 = 48,000` bytes per plane. The logical GRAY2 page is
decomposed into these 1-bit display planes by the writer.

For color, slot 0 holds the full pixel buffer in the declared format.

### 5.3 Plane Bitmap Interpretation

The `plane_bitmap` bits indicate which of the 4 slot pairs are present. What
each slot means depends on `pixel_format`:

**GRAY1 / GRAY2 / GRAY4 (e-paper):**

| Bit | Value | Plane | Description |
|-----|-------|-------|-------------|
| 0   | 0x01  | 0     | MSB plane |
| 1   | 0x02  | 1     | LSB plane |
| 2   | 0x04  | 2     | BW plane (1-bit dithered) |
| 3   | 0x08  | 3     | Delta plane (future) |

**RGB565 / RGB888 / RGBA8888 (color LCD/OLED):**

| Bit | Value | Plane | Description |
|-----|-------|-------|-------------|
| 0   | 0x01  | 0     | Full pixel buffer |
| 1-3 | —     | —     | Reserved |

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
- `PageIndexEntry.page_crc32`: CRC32 over all plane blobs for that page

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
            CHROME_POLICY, PAGE_INDEX, NAV_INDEX, CHAPTER_INDEX,
            PAGE_CHUNK_INDEX, PAGE_TRANSITION_INDEX, PAGE_DATA};
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

// Step 6: Read directly indexed chapter rows
chapter_section = sections[CHAPTER_INDEX];
chapter = read_ChapterIndexEntry(chapter_section.offset + chapter_index * chapter_section.entry_size);

// Step 7: Read chunk and transition metadata
chunk_section = sections[PAGE_CHUNK_INDEX];
transition_section = sections[PAGE_TRANSITION_INDEX];

// Step 8: Decode a specific page using plane directory and optional chunks
page = pages[page_number];

if (page.page_crc32 != 0) {
    // CRC32 covers all plane blobs concatenated in slot order
    crc_data = concat plane blobs for present planes;
    if (crc32(crc_data) != page.page_crc32) error("page CRC mismatch");
}

// Decompress only the planes needed for the current refresh mode
per_plane = page.page_flags & 1;  // per_plane_compression bit
for (slot = 0; slot < 4; slot++) {
    if (!(page.plane_bitmap & (1 << slot))) continue;  // plane not present
    method = per_plane ? page.plane_compression[slot] : page.compression_method;
    offset = header.page_data_offset + page.offset_plane[slot];
    compressed_blob = read_bytes(offset, page.size_plane[slot]);
    decompressed = decompress(compressed_blob, method);
    // Decompressed size computed from pixel_format, stored_width, stored_height
    // Use decompressed data for the appropriate display plane
}

// Constrained readers may instead iterate PAGE_CHUNK_INDEX entries for the
// selected page/plane, decompress each chunk, and stream it immediately.

// Step 9: Use pixels in the profile's storage order
// For xteink-x4-portrait, default GRAY2 pages are stored as native SSD1677
// physical-order planes. The writer has already applied logical-to-physical
// rotation and display-plane decomposition.
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
6. CHAPTER_INDEX
7. PAGE_INDEX
8. PAGE_CHUNK_INDEX
9. PAGE_TRANSITION_INDEX
10. Zero padding
11. PAGE_DATA

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
| Logical-to-physical rotation | 270 degrees clockwise |
| Default pixel format | GRAY2_PACKED |
| Supported lower format | GRAY1_PACKED |
| Supported color formats | RGB565, RGB888, RGBA8888 |
| Grayscale levels | 4 for GRAY2, 2 for GRAY1 |
| Logical GRAY2 row size | 120 bytes (480 / 4) |
| Native plane row size | 100 bytes (800 / 8) |
| Native chunk size | 16 rows = 1,600 bytes decompressed |
| Compression | RLE_PACKBITS |

**Logical-to-physical rotation mapping for 270 degrees clockwise**:
```
physical_x = logical_height_px - 1 - logical_y
physical_y = logical_x
```

For `xteink-x4-portrait`, the compiler stores default `GRAY2_PACKED` output as
SSD1677-native 1-bit planes in physical `800x480` row order. The compiler
applies the logical-to-physical rotation before writing page data. Firmware does
not need to rotate logical portrait pixels on the device.

Required GRAY2 planes for X4:

| Plane slot | Bitmap | Contents | SSD1677 RAM |
|------------|--------|----------|-------------|
| 0 | 0x01 | MSB/red grayscale plane | secondary/red RAM (`0x26`) |
| 1 | 0x02 | LSB/black grayscale plane | black RAM (`0x24`) |
| 2 | 0x04 | BW fast-refresh plane | black RAM for current page, red RAM for previous page |

Each plane is divided into 30 independently compressed 16-row chunks. Chunk
metadata is stored in `PAGE_CHUNK_INDEX`.

**Canonical GRAY2 values**:
- `0 = black`
- `1 = dark gray`
- `2 = light gray`
- `3 = white`

**Canonical GRAY1 values**:
- `0 = black`
- `1 = white`

These canonical values define rendering input. For X4 output, the writer
converts them to native SSD1677 plane polarity. White bits remain set; active
pigment bits are cleared.

**Default refresh behavior**:

- First render or cleanup cadence: stream plane 0 to red RAM, plane 1 to black
  RAM, then trigger grayscale refresh.
- Adjacent page turn with a transition record: stream only changed BW chunks
  from the previous page to red RAM and current page to black RAM, then trigger
  partial refresh.
- Non-adjacent jump without a transition record: stream the full previous BW
  plane and full current BW plane, then trigger partial refresh.

---

## 10. Validation Rules

Readers/firmware should validate:

### File-Level
- [ ] Magic matches "BINBOOK\0"
- [ ] `file_size` is 0 or actual file size >= `file_size`
- [ ] `section_table_offset` + `section_table_length` <= file size
- [ ] `section_table_entry_size` is supported (40)

### Section-Level
- [ ] All required sections present
- [ ] All section offsets/lengths within file bounds
- [ ] `PAGE_DATA` offset/length match header
- [ ] Section CRC32s valid (if nonzero)

### Page-Level
- [ ] `page_index_entry_size` is supported (128)
- [ ] `nav_index_entry_size` is supported (48)
- [ ] `PAGE_CHUNK_INDEX.entry_size` is supported (24)
- [ ] `PAGE_TRANSITION_INDEX.entry_size` is supported (24)
- [ ] All plane blobs within `PAGE_DATA` bounds
- [ ] No overlapping plane blobs
- [ ] Plane blob offsets are 4-byte aligned
- [ ] `pixel_format` is supported
- [ ] `compression_method` is supported
- [ ] `page_kind` != MIXED_RESERVED (3)
- [ ] `progress_start_ppm` <= `progress_end_ppm`
- [ ] Progress is monotonically non-decreasing
- [ ] Page dimensions match layout profile
- [ ] `plane_bitmap` bits are valid for `pixel_format`
- [ ] Delta plane bit 3 is not set (future)

### Chunk-Level
- [ ] Chunk records are sorted by page, plane slot, then chunk index
- [ ] Chunk records reference existing pages and present plane slots
- [ ] Chunk `page_data_offset` and `compressed_size` are within `PAGE_DATA`
- [ ] Chunk ranges are contained within their parent plane blob
- [ ] `uncompressed_size` matches profile chunk geometry
- [ ] X4 GRAY2 pages have 90 chunk records: 3 planes × 30 chunks

### Chapter-Level
- [ ] `CHAPTER_INDEX.entry_size` is supported (32)
- [ ] `chapter_index` values are contiguous from 0
- [ ] `rendered_page_number` targets an existing page
- [ ] `nav_type` is a selectable chapter row type

### String-Level
- [ ] All `StringRef` offsets/lengths within STRING_TABLE bounds
- [ ] Strings are valid UTF-8 (or handled gracefully)

### Profile-Specific (xteink-x4-portrait)
- [ ] Pages are `GRAY2_PACKED` by default, or `GRAY1_PACKED` only when the file/profile explicitly declares the lower-quality storage format
- [ ] Default `GRAY2_PACKED` pages set `plane_bitmap = 0x07`
- [ ] Default GRAY2 plane blobs are physical `800x480` native 1-bit planes
- [ ] Default GRAY2 planes are chunked as 30 chunks of 16 rows each
- [ ] Adjacent page pairs have forward and backward transition records
- [ ] `READER_REQUIREMENTS.required_storage_pixel_formats` matches the emitted page format
- [ ] `DISPLAY_PROFILE.logical_width/height` = (480, 800)
- [ ] `DISPLAY_PROFILE.physical_width/height` = (800, 480)
- [ ] `DISPLAY_PROFILE.logical_to_physical_rotation` = 270
- [ ] `LAYOUT_PROFILE.full_page_width/height` matches display profile

---

## 11. Examples

### 11.1 Minimal Valid File (Hex Dump)

This example is illustrative only and must be regenerated after the
`PAGE_CHUNK_INDEX` and `PAGE_TRANSITION_INDEX` sections are implemented in the
writer. The section layouts above are authoritative.

A minimal `.binbook` file with:
- 1 page (GRAY2_PACKED for `xteink-x4-portrait`)
- No navigation entries
- Minimal metadata

**Header** (first 256 bytes):
```
00000000: 42 49 4e 42 4f 4f 4b 00  00 00 00 00 00 01 00 00  |BINBOOK.........|
00000010: 00 00 00 00 00 00 00 00  00 01 00 00 00 00 00 00  |................|
00000020: 28 01 00 00 10 00 4c 01  04 00 04 00 00 00 10 00  |(.....L.........|
00000030: 00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  |................|
00000040: 00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  |................|
... (remainder of header is zero-filled to 256 bytes)
```

**Legend**:
- Bytes 0-7: `magic` = "BINBOOK\0"
- Bytes 8-9: `reserved0` = 0
- Bytes 10-11: `reserved1` = 0
- Bytes 12-13: `header_size` = 256
- Bytes 14-15: `header_flags` = 0
- Bytes 16-23: `file_size` = 0 (skip validation)
- Bytes 24-31: `section_table_offset` = 256 (0x100)
- Bytes 32-35: `section_table_length` = 296 (0x128 = 40 bytes × 7 sections + 16 bytes for PAGE_DATA)
- Bytes 36-37: `section_table_entry_size` = 40
- Bytes 38-39: `section_count` = 15 (all required sections)
- Bytes 40-41: `page_index_entry_size` = 128
- Bytes 42-43: `nav_index_entry_size` = 48

*(Note: Full hex dump would be ~100+ lines. The above shows header structure. In practice, use `binbook inspect` to see a real file's structure.)*

### 11.2 StringRef Example

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

### 11.3 GRAY1_PACKED Pixel Example

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

### 11.4 GRAY2_PACKED Pixel Example

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
// 3 = MIXED_RESERVED
```

### Compression Methods
```c
#define COMPRESS_NONE         0
#define COMPRESS_RLE_PACKBITS 1
#define COMPRESS_LZ4          2
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
