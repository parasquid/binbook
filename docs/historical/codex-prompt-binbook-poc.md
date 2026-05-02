# Codex Prompt: BinBook Python POC Encoder and Decoder

> Historical note: this prompt describes the original proof-of-concept direction and is no longer authoritative for BinBook 0.1. Use [`../../BINBOOK_FORMAT_SPEC.md`](../../BINBOOK_FORMAT_SPEC.md) for the current format and [`../../AGENTS.md`](../../AGENTS.md) for current implementation guidance.

You are implementing a Python proof of concept for BinBook, a universal compiled raster-book format for low-RAM e-ink/display devices.

Read this whole prompt before coding. Build the project incrementally, with tests where practical.

## Goal

Create a Python project that can:

1. Encode an EPUB into a .binbook file.
2. Encode an already-rendered folder of page PNGs into a .binbook file for development/debugging.
3. Decode a .binbook page into PNG.
4. View a .binbook on desktop using a simulation viewer.
5. Inspect a .binbook and print metadata/index information.

The first target profile is:

    xteink-x4-portrait

Use logical portrait page dimensions:

    480 × 800

For xteink-x4-portrait, all pages MUST be stored as:

    GRAY2_PACKED

This applies to both text pages and image pages.

The .binbook stores pre-rendered page content pixels only.

The .binbook does not store reflowable text for runtime rendering.

Page number, chapter title, progress, and other reader UI/chrome are not baked into page pixels. They are rendered by the decoder/viewer/firmware using metadata and ChromePolicy.

## Important Concept

BinBook is not an EPUB renderer on-device.

The compiler does this:

    EPUB → rendered pages → grayscale packed pixels → compressed page blobs → .binbook

The decoder does this:

    .binbook → page index → compressed blob → decompressed pixels → PNG/viewer

No reflow is done by the decoder.

Changing font, margins, zoom/font scale, line spacing, character spacing, word spacing, display profile, image policy, chrome policy, or grayscale policy requires recompilation or a separate rendition.

## Project Structure

Create a clean Python package.

Suggested layout:

    binbook-poc/
    ├── pyproject.toml
    ├── README.md
    ├── binbook/
    │   ├── __init__.py
    │   ├── cli.py
    │   ├── constants.py
    │   ├── structs.py
    │   ├── profiles.py
    │   ├── model.py
    │   ├── writer.py
    │   ├── reader.py
    │   ├── rle.py
    │   ├── pixels.py
    │   ├── strings.py
    │   ├── checksums.py
    │   ├── hashes.py
    │   ├── images.py
    │   ├── epub.py
    │   ├── render.py
    │   ├── viewer.py
    │   └── inspect.py
    └── tests/
        ├── test_structs.py
        ├── test_strings.py
        ├── test_rle.py
        ├── test_pixels.py
        ├── test_checksums.py
        ├── test_hashes.py
        └── test_roundtrip.py

Use Python 3.11+.

Use type hints.

Prefer clear code over clever code.

## CLI Overview

Implement a command named:

    binbook

The CLI should have these subcommands:

    encode
      Convert an EPUB into a self-contained .binbook file.

    encode-png-folder
      Developer/debug command. Convert a folder of already-rendered page PNGs into a .binbook file. This bypasses EPUB parsing/rendering and is used to validate the binary format, pixel packing, compression, page index, and decoder.

    decode
      Decode one page from a .binbook file and export it as a PNG.

    view
      Open a .binbook file in a desktop simulation viewer. The viewer should simulate how the book will look on the target device.

    inspect
      Print metadata, section table, page index summary, navigation summary, string table summary, checksum status, and compression statistics for a .binbook file.

## binbook encode --help

Expected help text shape:

    Usage:
      binbook encode INPUT_EPUB -o OUTPUT_BINBOOK [options]

    Description:
      Convert an EPUB into a self-contained BinBook file.

      The compiler reads the EPUB, processes the spine in reading order,
      renders text into grayscale page blobs, extracts images into separate
      image pages, writes binary navigation metadata, and stores compressed
      page data in the .binbook file.

      The output is non-reflowable. Changing font, margins, layout, profile,
      typography settings, chrome settings, or quality settings requires
      recompiling the source EPUB.

    Arguments:
      INPUT_EPUB
        Path to the source EPUB file.

    Required options:
      -o, --output OUTPUT_BINBOOK
        Path to the output .binbook file.

    Options:
      --profile PROFILE
        Target display profile.
        Default: xteink-x4-portrait

      --font FONT_TTF
        Path to a TTF font file.

      --font-mode MODE
        Font handling mode.
        Values:
          default  Use EPUB fonts when available; use the provided TTF as fallback/default.
          force    Ignore EPUB font-family CSS and use the provided TTF globally.
        Default: force for the first POC unless default mode is implemented.

      --font-size PX
        Base font size in pixels.
        Default: 24

      --font-weight WEIGHT
        Font weight hint.
        Example: 400, 500, 600, 700.
        Default: 400

      --min-font-size PX
        Minimum rendered font size in pixels.
        Default: 18

      --max-font-size PX
        Maximum rendered font size in pixels. 0 means unused.
        Default: 0

      --font-scale SCALE
        Compile-time text scale.
        Example: 1.00, 1.10, 1.25
        Stored as font_scale_milli.
        Default: 1.00

      --line-height RATIO
        Line height ratio.
        Stored as line_height_milli.
        Default: 1.25

      --paragraph-spacing-before PX
        Extra pixels before each paragraph.
        Default: 0

      --paragraph-spacing-after PX
        Extra pixels after each paragraph.
        Default: 8

      --char-spacing EM
        Extra character spacing in em units.
        Example: 0.02 means +0.02em.
        Stored as character_spacing_milli_em.
        Default: 0

      --word-spacing EM
        Extra word spacing in em units.
        Stored as word_spacing_milli_em.
        Default: 0

      --text-align MODE
        Text alignment.
        Values:
          left
          center
          right
          justify
          preserve
        Default: left for force mode.

      --hyphenation MODE
        Hyphenation behavior.
        Values:
          off
          on
          preserve
        Default: off

      --hyphenation-language LANG
        Language tag used by hyphenation logic.
        Example: en, en-US, ja, fil.
        Default: EPUB language if available.

      --margin-top PX
      --margin-right PX
      --margin-bottom PX
      --margin-left PX

      --header-height PX
        Reserved header area height for firmware/viewer chrome.

      --footer-height PX
        Reserved footer area height for firmware/viewer chrome.

      --dither METHOD
        Dithering method for image and grayscale conversion.
        Values:
          none
          floyd-steinberg
          ordered-bayer
        Default: floyd-steinberg

      --dither-strength VALUE
        Dithering strength.
        Example: 1.00 means normal strength.
        Stored as dithering_strength_milli.
        Default: 1.00

      --dark-mode
        Render pages/chrome in negative or dark-mode policy.
        Stored in ChromePolicy and/or layout/chrome flags.

      --show-page-numbers
      --show-percent
      --show-chapter-title
      --show-chapter-marks
        Default chrome-policy hints for firmware/viewer.

      --debug
        Print extra compiler diagnostics.

    Example:
      binbook encode input.epub -o output.binbook \
        --profile xteink-x4-portrait \
        --font ./font.ttf \
        --font-mode force \
        --font-size 24 \
        --font-weight 400 \
        --min-font-size 18 \
        --font-scale 1.10 \
        --line-height 1.25 \
        --char-spacing 0.02 \
        --paragraph-spacing-after 8 \
        --dither floyd-steinberg \
        --dither-strength 1.00

## encode-png-folder Metadata

When encoding from a folder of already-rendered PNGs (no EPUB source), populate
the binary sections as follows:

    SourceIdentity:
      source_type          = 0 (unknown)
      original_file_size   = 0
      original_md5         = all zeros
      original_sha256      = all zeros
      original_filename    = empty StringRef
      source_package_identifier = empty StringRef

    BookMetadata:
      All StringRef fields are empty StringRefs.
      series_index_milli   = 0
      metadata_flags       = 0

    TypographyPolicy:
      All fields set to neutral/zero defaults.

    FontPolicy:
      font_mode            = 0 (unknown)
      All hash and StringRef fields are empty/zero.

All other policy sections (ImagePolicy, CompressionPolicy, ChromePolicy) use
the values supplied on the command line or their documented defaults.

source_spine_index for every page is UINT32_MAX (no spine).
chapter_nav_index for every page is UINT32_MAX (no chapter).

The NAV_INDEX section MUST still be present (required section) but MAY have
record_count = 0.

## Xteink X4 Profile Rule

For xteink-x4-portrait:

    - all pages MUST be GRAY2_PACKED
    - text pages MUST be GRAY2_PACKED
    - image pages MUST be GRAY2_PACKED
    - source images are resized/rotated/processed, then quantized/dithered to 4 grayscale levels
    - GRAY4_PACKED remains part of the universal BinBook format for future profiles but MUST NOT be emitted for xteink-x4-portrait v0.1
    - intended output grayscale levels = 4
    - required storage pixel formats = GRAY2_PACKED only

## Xteink XTC/XTCH Compatibility Notes

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

## Metadata Encoding Policy

Do not use JSON for BinBook metadata.

Do not use CBOR for BinBook metadata.

Do not use Protocol Buffers for BinBook metadata.

Do not use MessagePack or other dynamically parsed object formats for required runtime metadata.

The POC must implement the BinBook format as an all-binary container.

Use:
    - fixed binary header
    - fixed binary section table
    - fixed binary display profile record
    - fixed binary layout profile record
    - fixed binary reader requirements record
    - fixed binary source identity record
    - fixed binary book metadata record
    - fixed binary rendition identity record
    - fixed binary font policy record
    - fixed binary typography policy record
    - fixed binary image policy record
    - fixed binary compression policy record
    - fixed binary chrome policy record
    - fixed binary page index records
    - fixed binary navigation index records
    - UTF-8 string table
    - raw page data region

The inspect command may output JSON to stdout, but the .binbook file itself must not require JSON, CBOR, protobuf, or MessagePack to read.

## Section IDs

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

## Binary Layout

Use one single .binbook file.

No required sidecar files.

Use little-endian binary.

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

The section table is authoritative.

## Padding and Reserved Bytes

All padding and reserved bytes written by the compiler MUST be zero-filled.

Readers MUST ignore padding and reserved bytes.

Readers MUST NOT depend on padding contents for parsing.

No magic values or sigils are used for padding.

The only required v0.1 magic value is:

    magic[8] = "BINBOOK\0"

## CRC32

All CRC32 fields use IEEE 802.3 / PKZIP CRC32.

Parameters:
    Polynomial: 0xEDB88320
    Initial value: 0xFFFFFFFF
    Final XOR: 0xFFFFFFFF

If a CRC32 field is 0, the checksum is considered not computed and validation should be skipped.

## Hashes

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

## v0.1 Entry Sizes

Expected v0.1 entry sizes:

    SectionEntry: 40 bytes
    PageIndexEntry: 76 bytes
    NavIndexEntry: 48 bytes

Header fields should therefore be:

    section_table_entry_size = 40
    page_index_entry_size = 76
    nav_index_entry_size = 48

Readers MUST reject files where these entry-size values do not match the v0.1 expectations unless they explicitly support that alternate same-major layout.

## Structs

Implement the structs exactly as defined in [`binbook-poc-spec.md`](binbook-poc-spec.md).

Important structs:
    Header
    SectionEntry
    StringRef
    ReaderRequirements
    DisplayProfile
    LayoutProfile
    SourceIdentity
    BookMetadata
    RenditionIdentity
    FontPolicy
    TypographyPolicy
    ImagePolicy
    CompressionPolicy
    ChromePolicy
    PageIndexEntry
    NavIndexEntry

## Pixel Formats

Pixel storage order:
    - Page blobs are stored in row-major order.
    - Rows are stored from top to bottom.
    - Pixels within each row are stored left to right.
    - The logical origin (0,0) is the top-left of the content area.
    - Firmware with a different native scan order must convert during display.

GRAY2_PACKED:
    4 pixels per byte
    leftmost pixel in highest bits
    0 = black
    1 = dark gray
    2 = light gray
    3 = white

GRAY4_PACKED:
    2 pixels per byte
    leftmost pixel in high nibble
    0 = black
    15 = white

For xteink-x4-portrait, emit GRAY2_PACKED only.

## Compression

Implement RLE_PACKBITS.

Rules:
    control byte 0..127:
      literal run of control + 1 bytes

    control byte 128..255:
      repeated-byte run of (control & 127) + 1 bytes
      followed by one byte to repeat

## Validation

The reader must validate:
    - magic
    - version
    - required reader features
    - file_size (skip check if file_size == 0)
    - section bounds
    - required sections
    - StringRef bounds
    - page blob bounds
    - no overlapping page blobs
    - supported pixel formats
    - supported compression methods
    - page dimensions
    - progress ranges
    - progress monotonicity
    - checksum fields when nonzero
    - MIXED_RESERVED page_kind (3) rejected in v0.1
    - X4 profile emits/accepts GRAY2 pages only unless a future profile says otherwise
    - LayoutProfile full_page dimensions match DisplayProfile logical dimensions
    - LayoutProfile content box is consistent with margin fields

## EPUB Handling

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

## Text Rendering

For POC, a practical approach is acceptable:
    - Extract readable text from spine HTML.
    - Use Pillow ImageDraw with the chosen TTF.
    - Word-wrap text into the content box.
    - Paginate based on font metrics and line height.
    - Apply typography policy.

## Image Rendering

For xteink-x4-portrait:
    1. Load image with Pillow.
    2. Convert to grayscale.
    3. Decide whether rotation improves fit into the content box.
    4. Rotate if beneficial.
    5. Resize to fit inside content box.
    6. Do not crop.
    7. Flatten alpha to ImagePolicy.alpha_background_gray.
    8. Center on image_background_gray.
    9. Dither/quantize to 4 grayscale levels.
    10. Pack as GRAY2_PACKED.
    11. Store as IMAGE page.

Do not emit GRAY4_PACKED for X4 v0.1.

## Tests

Add tests for:
    - binary struct pack/unpack
    - explicit v0.1 entry sizes: SectionEntry=40, PageIndexEntry=76, NavIndexEntry=48
    - zero-filled reserved/padding from writer
    - string table insert/lookup
    - zero-length StringRef handling
    - StringRef validation
    - CRC32 known vectors
    - SHA-256 section hash behavior
    - RLE roundtrip
    - RLE control byte 0x80 treated as 1-repeat (not no-op)
    - GRAY1 pack/unpack
    - GRAY2 pack/unpack
    - GRAY4 pack/unpack
    - row padding: last byte zero-filled for widths not a multiple of pixels_per_byte
    - X4 profile rejects GRAY4 page emission
    - X4 image pages are quantized to GRAY2
    - BinBook canonical GRAY2 to Xteink XTH LUT conversion table
    - PNG-folder encode
    - PNG-folder encode: source_spine_index and chapter_nav_index are UINT32_MAX
    - page decode
    - inspect validation
    - unsupported version rejection
    - unsupported feature rejection
    - malformed StringRef rejection
    - page blob out-of-bounds rejection
    - page blob overlap rejection
    - progress monotonicity validation
    - MIXED_RESERVED page_kind (3) rejection in v0.1
    - LayoutProfile full_page dimensions must match DisplayProfile logical dimensions
    - LayoutProfile content box derived correctly from margin fields
    - file_size = 0 skips file size validation

## Development Order

Implement in this order:
    1. constants/enums
    2. binary struct definitions
    3. CRC32 and SHA-256 helpers
    4. StringRef and string table
    5. RLE codec
    6. pixel packing
    7. X4 canonical GRAY2 and XTH LUT mapping helpers
    8. profile/layout/policy models
    9. binary writer for PNG-folder input
    10. binary reader
    11. version and feature validation
    12. checksum validation
    13. page bounds and overlap validation
    14. progress validation
    15. decode page to PNG
    16. inspect command
    17. desktop simulation viewer
    18. EPUB metadata extraction
    19. text rendering from EPUB spine
    20. image extraction and image-page insertion
    21. nav mapping
    22. polish CLI

Do not start with Calibre plugin.
Do not start with firmware.

## Acceptance Criteria

The POC is acceptable when:
    1. binbook encode-png-folder creates a valid .binbook from already-rendered page PNGs.
    2. binbook decode can render a selected page to PNG.
    3. binbook inspect prints sane metadata.
    4. binbook view provides a desktop simulation viewer.
    5. binbook encode input.epub works on at least one simple EPUB.
    6. For xteink-x4-portrait, all pages are stored as GRAY2_PACKED.
    7. Text pages are stored as GRAY2_PACKED.
    8. Image pages are stored as GRAY2_PACKED for X4 and are quantized/dithered to 4 grayscale levels.
    9. Image pages are separate from text flow.
    10. Page data starts at a header-defined offset.
    11. Page blob offsets are relative to page_data_offset.
    12. The file is self-contained and requires no sidecar.
    13. The file includes version_major and version_minor.
    14. The reader rejects unsupported major versions.
    15. The reader rejects unsupported required reader features.
    16. The .binbook file contains no JSON, CBOR, protobuf, or MessagePack sections.
    17. All required runtime metadata is stored as fixed binary records and a UTF-8 string table.
    18. The file contains typography policy, image policy, compression policy, chrome policy, reader requirements, and rendition identity sections.
    19. The reader validates CRC32 fields when nonzero.
    20. The reader validates SHA-256 policy/profile hashes when practical.
    21. The reader rejects overlapping page blobs.
    22. The reader validates progress monotonicity.
    23. The reader handles zero-length StringRefs correctly.
    24. The reader rejects MIXED page_kind in v0.1.
    25. The X4 display conversion helper maps canonical BinBook GRAY2 to Xteink XTH LUT values correctly.

## Non-goals

Do not implement yet:
    - Calibre plugin
    - actual Xteink firmware
    - KOReader sync
    - annotations
    - text search
    - on-device reflow
    - on-device font changes
    - image splitting
    - crop-to-fill image option
    - exact EPUB CSS fidelity
    - per-region mixed bit depth
    - CBOR metadata
    - protobuf metadata
    - JSON metadata inside the .binbook file
    - XTH/XTG as BinBook internal storage

## Notes

Prefer explicit validation errors.

Prefer readable binary-writing code.

Keep the format versioned.

Write comments explaining the binary layout.

Make the code easy to port later to C/C++ firmware.
