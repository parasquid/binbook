# BinBook Documentation and Roadmap

> Historical note: this roadmap reflects the original proof-of-concept direction and is no longer authoritative for BinBook 0.1. Use [`../../BINBOOK_FORMAT_SPEC.md`](../../BINBOOK_FORMAT_SPEC.md) for the current format and [`../../AGENTS.md`](../../AGENTS.md) for current implementation guidance.

## 1. Project Summary

BinBook is a universal compiled raster-book format.

It is designed for low-RAM e-ink and embedded display devices that should not perform EPUB layout, CSS handling, font rendering, or image processing at runtime.

The main idea:

    desktop compiler understands books
    firmware understands pages
    display driver understands pixels

A BinBook file contains pre-rendered compressed page blobs plus fixed binary metadata for navigation, progress, display compatibility, typography/rendition identity, chrome policy, integrity checks, and future sync.

## 2. Why BinBook Exists

Small e-ink devices based on low-RAM microcontrollers are not good targets for full EPUB rendering.

Runtime EPUB rendering requires:

    - HTML parsing
    - CSS cascade
    - font loading
    - font shaping
    - font rasterization
    - line breaking
    - pagination
    - image decoding
    - image scaling
    - layout
    - framebuffer generation

BinBook moves this work to the compiler.

Runtime display becomes:

    - open file
    - validate magic/version
    - validate required features
    - validate section bounds
    - validate page blob bounds
    - read page index
    - find compressed page blob
    - decompress
    - convert pixels if needed
    - display

## 3. Core Format Identity

Format name:

    BinBook

File extension:

    .binbook

Optional future short extension:

    .bbk

The format is universal, but each compiled file targets a display profile.

Example:

    book.epub
      → book.xteink-x4.binbook
      → book.xteink-x3.binbook
      → book.generic-600x800.binbook
      → book.future-16gray.binbook

The container format is universal.

The compiled artifact is display-specific.

## 4. First POC Target

First target profile:

    xteink-x4-portrait

Logical page size:

    480 × 800

Text pages:

    2-bit grayscale, GRAY2_PACKED

Image pages:

    2-bit grayscale, GRAY2_PACKED

Image processing:

    source image → grayscale → resize/rotate → dither/quantize to 4 levels → GRAY2_PACKED

Compression:

    PackBits-style RLE

Container:

    single self-contained .binbook file

Metadata encoding:

    fixed binary records + UTF-8 string table

Integrity:

    CRC32 fields for file/header/section/page validation when nonzero

Hashes:

    SHA-256 for profile/policy/rendition hashes

Decoder/viewer:

    Python desktop simulation viewer

## 5. Xteink X4 / XTC / XTCH Implications

Xteink native formats distinguish:

    XTC / XTG:
      1-bit monochrome pages

    XTCH / XTH:
      2-bit / 4-level grayscale pages

BinBook does not use XTG/XTH as its internal page blob format.

BinBook stores canonical logical pages in row-major order.

For xteink-x4-portrait, BinBook emits GRAY2_PACKED pages only.

The X4 firmware/display backend converts canonical BinBook GRAY2 into the display controller’s native update order and LUT mapping.

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

Conversion:

    BinBook black      0 → XTH 3
    BinBook dark gray  1 → XTH 1
    BinBook light gray 2 → XTH 2
    BinBook white      3 → XTH 0

This keeps BinBook portable and keeps Xteink-specific scan/LUT handling in the firmware display backend.

## 6. Important Design Decisions

### 6.1 Single file, not map + binary pair

BinBook should be one authoritative file.

Good:

    book.binbook

Avoid as default runtime design:

    book.map
    book.bin

Reason:

    - easier Calibre integration
    - easier copying to SD card
    - easier hashing/sync
    - fewer stale/missing pair problems
    - simpler user file management

### 6.2 Page data offset is stored in the header

The reader must not assume page blobs start at a hardcoded byte offset.

The header stores:

    page_data_offset
    page_data_length

The compiler should align page_data_offset to 64 KiB by default.

The reader must read the actual value from the header.

### 6.3 Page blob offsets are relative

Each page index entry stores:

    relative_blob_offset

The actual file offset is:

    absolute_offset = page_data_offset + relative_blob_offset

This allows the metadata/index region to grow without rewriting every page offset.

### 6.4 Content pixels only

Page blobs do not contain page numbers, chapter titles, progress bars, or status UI.

Firmware/viewer draws those.

The BinBook file stores layout metadata and ChromePolicy so the reader knows where content and optional chrome should go.

### 6.5 No runtime reflow

No reflow happens on the device.

Changing font, margins, orientation, screen size, font scale, line spacing, character spacing, word spacing, or grayscale policy requires recompilation.

### 6.6 No JSON, CBOR, protobuf, or MessagePack in v0.1

BinBook v0.1 required runtime metadata is all binary.

Use:

    - fixed binary header
    - fixed binary section table
    - fixed binary display profile
    - fixed binary layout profile
    - fixed binary reader requirements
    - fixed binary source identity
    - fixed binary book metadata
    - fixed binary rendition identity
    - fixed binary font policy
    - fixed binary typography policy
    - fixed binary image policy
    - fixed binary compression policy
    - fixed binary chrome policy
    - fixed binary page index
    - fixed binary navigation index
    - UTF-8 string table
    - raw page data region

The inspect tool may output JSON to stdout for humans, but the .binbook file itself should not contain JSON/CBOR/protobuf metadata sections.

### 6.7 Zero padding and CRCs

Padding and reserved bytes are zero-filled.

Padding has no sigil/magic value.

Use CRC32 for integrity, not padding markers.

Only the file header has a required magic:

    "BINBOOK\0"

## 7. Runtime Reader Model

The reader should:

    1. Open .binbook.
    2. Read fixed header.
    3. Validate magic.
    4. Validate version.
    5. Validate file_size.
    6. Read section table.
    7. Validate required sections exist.
    8. Read reader requirements.
    9. Validate required features.
    10. Validate display profile compatibility.
    11. Read layout profile.
    12. Read string table.
    13. Read page index.
    14. Validate page blob bounds.
    15. Validate page blob non-overlap.
    16. Validate progress monotonicity.
    17. Read navigation index.
    18. Decode requested pages on demand.
    19. Cache compressed page blobs if RAM permits.

## 8. Cache Model

Firmware should cache compressed blobs, not decoded pages.

Decoded pages are too large for ESP32-C3-class devices.

For example, 480 × 800 logical pages:

    2-bit full page:
      480 × 800 × 2 / 8 = 96,000 bytes

Recommended cache policy:

    - budget by bytes, not page count
    - prefer current, next, previous
    - bias forward
    - skip unusually large page blobs if needed
    - decode only the current visible page or strip

For X4 v0.1, GRAY4 decoded pages should not be required.

## 9. Typography Model

Typography is compile-time metadata.

Typography affects rendered page blobs and pagination.

Changing typography requires recompilation.

TypographyPolicy includes:

    - base font size
    - minimum font size
    - maximum font size
    - font weight
    - font scale
    - line height
    - paragraph spacing
    - character spacing
    - word spacing
    - text alignment
    - hyphenation policy
    - hyphenation language
    - widow/orphan policy

Use fixed-point integer fields:

    font_scale_milli:
      1000 = 100%

    line_height_milli:
      1250 = 1.25

    character_spacing_milli_em:
      20 = +0.02em

    word_spacing_milli_em:
      100 = +0.10em

## 10. Image Policy

Every EPUB image is extracted from the text flow and placed on its own image page.

For X4:

    - image pages are GRAY2_PACKED
    - images are quantized/dithered to 4 grayscale levels
    - no GRAY4 output for xteink-x4-portrait v0.1

Image page rules:

    - insert at original reading-order position
    - no cropping
    - rotate if it improves fit
    - resize to fit content box
    - flatten alpha to alpha_background_gray
    - center on image_background_gray
    - apply dithering method and strength
    - pack as the profile’s allowed image pixel format

Deferred:

    - long image splitting
    - spread handling
    - crop-to-fill
    - captions
    - inline image preservation

## 11. Chrome Policy

ChromePolicy stores firmware/viewer display hints for reader chrome.

Chrome is not baked into page blobs.

ChromePolicy can express:

    - show page numbers
    - show percent
    - show chapter marks
    - show chapter title
    - progress bar mode
    - progress bar position
    - dark mode / negative rendering hint

Firmware may ignore ChromePolicy if the UI has its own settings.

## 12. Rendition Identity

A rendition is one exact compiled output.

A different font size, font scale, margins, display profile, image policy, chrome policy, or compression policy produces a different rendition.

RenditionIdentity stores:

    - rendition hash
    - display profile hash
    - layout profile hash
    - font policy hash
    - typography policy hash
    - image policy hash
    - compression policy hash
    - chrome policy hash
    - compiler name
    - compiler version
    - created timestamp

## 13. Navigation Model

Navigation is stored in a fixed binary NAV_INDEX section.

Each entry stores:

    - nav index, 0-based
    - nav type
    - level
    - title StringRef
    - source href StringRef
    - source spine index
    - rendered page number
    - optional parent/child/sibling indexes

Nested TOCs can be represented two ways:

    simple readers:
      use level only

    richer readers:
      use parent/child/sibling indexes

The firmware does not parse EPUB navigation.

It only reads BinBook navigation entries.

## 14. Progress Model

Use normalized progress.

Each page has:

    progress_start_ppm
    progress_end_ppm

Progress unit:

    parts per million

Meaning:

    0 = 0%
    1,000,000 = 100%

Rules:

    - first page starts at 0
    - final page ends at 1,000,000
    - per-page progress_start_ppm <= progress_end_ppm
    - progress is monotonically non-decreasing across pages

## 15. Integrity Model

CRC32:

    - file_crc32
    - header_crc32
    - section crc32
    - page_crc32

CRC32 fields use IEEE 802.3 / PKZIP CRC32.

If a CRC32 field is 0, validation is skipped.

Hashes:

    - all *_hash[32] fields are SHA-256 raw bytes
    - profile/policy hashes are computed over section data with own hash field zeroed
    - rendition_hash is computed from source hash and profile/policy hashes

## 16. Development Roadmap

### Phase 0: Repository setup

Deliverables:

    - pyproject.toml
    - package skeleton
    - CLI skeleton
    - README
    - tests running

### Phase 1: Binary constants and structs

Deliverables:

    - constants/enums
    - primitive binary packing helpers
    - fixed header struct
    - section table struct
    - StringRef struct
    - reader requirements struct
    - display profile struct
    - layout profile struct
    - source identity struct
    - book metadata struct
    - rendition identity struct
    - font policy struct
    - typography policy struct
    - image policy struct
    - compression policy struct
    - chrome policy struct
    - page index entry struct
    - nav index entry struct

Acceptance:

    - all structs can pack/unpack roundtrip
    - entry sizes match spec
    - reserved bytes are zero-filled by writer

### Phase 2: Integrity helpers

Deliverables:

    - CRC32 IEEE/PKZIP helper
    - SHA-256 hash helpers
    - policy/profile hash computation
    - rendition hash computation

### Phase 3: String table

Deliverables:

    - string table builder
    - string deduplication where practical
    - StringRef creation
    - StringRef validation
    - zero-length StringRef handling
    - UTF-8 validation for viewer/inspect
    - streaming string access helper

### Phase 4: Binary primitives

Deliverables:

    - RLE encoder/decoder (BinBook variant; differs from standard Apple PackBits at 0x80)
    - GRAY1 pack/unpack
    - GRAY2 pack/unpack
    - GRAY4 pack/unpack
    - row padding: ceil(width / pixels_per_byte), unused bits zero-filled
    - X4 GRAY2 → XTH LUT mapping helper
    - tests

### Phase 5: PNG-folder round trip

Deliverables:

    - encode-png-folder command
    - all-binary BinBook writer
    - all-binary BinBook reader
    - decode command

Acceptance:

    - folder of PNGs can become .binbook
    - selected page can decode back to PNG
    - X4 profile emits GRAY2 only
    - page data offset and section table are valid
    - .binbook contains no JSON/CBOR/protobuf metadata

### Phase 6: Validation layer

Deliverables:

    - version validation
    - feature flag validation
    - required reader feature validation
    - unsupported major version rejection
    - unsupported required feature rejection
    - file_size validation (skip if file_size == 0)
    - section bounds validation
    - page blob bounds validation
    - page blob overlap validation
    - progress monotonicity validation
    - MIXED_RESERVED page_kind rejection in v0.1
    - X4 GRAY4 rejection
    - LayoutProfile full_page vs DisplayProfile logical dimensions consistency
    - LayoutProfile content box vs margin fields consistency

### Phase 7: Inspect tool

Deliverables:

    - inspect command
    - human-readable output
    - optional inspect --json output to stdout
    - strict validation mode

### Phase 8: Desktop simulation viewer

Deliverables:

    - simple viewer
    - next/previous page
    - jump to page
    - show page number
    - show chapter title if known
    - optional debug content-box overlay
    - optional simulated firmware chrome

### Phase 9: EPUB metadata and spine

Deliverables:

    - read EPUB
    - extract title/author/language/package ID
    - compute MD5/SHA-256
    - iterate spine items
    - create rough page sequence

### Phase 10: Text rendering

Deliverables:

    - extract readable text from spine HTML
    - render text using Pillow and TTF
    - apply typography policy
    - word wrap
    - paginate
    - output TEXT pages as GRAY2_PACKED for X4

### Phase 11: Image extraction

Deliverables:

    - find images in spine reading order
    - remove/suppress images from text flow
    - insert dedicated IMAGE pages
    - rotate/resize/center
    - 4-level grayscale dithering
    - output IMAGE pages as GRAY2_PACKED for X4

### Phase 12: Navigation mapping

Deliverables:

    - parse EPUB TOC where practical
    - create nav entries
    - map nav entries to rendered pages best effort
    - page index entries reference chapter_nav_index

### Phase 13: CLI polish

Deliverables:

    - encode command stable
    - encode-png-folder command stable
    - decode command stable
    - view command stable
    - inspect command stable
    - helpful errors
    - README examples

### Phase 14: Later Calibre plugin

Not part of POC.

Future deliverables:

    - Calibre output plugin
    - GUI options for profile/font/margins/typography/chrome/preset
    - send-to-device workflow
    - possibly device plugin

## 17. POC Acceptance Criteria

The POC is successful when:

    1. binbook encode-png-folder creates a valid .binbook.
    2. binbook decode can render a selected page to PNG.
    3. binbook inspect prints sane metadata.
    4. binbook view allows desktop page navigation.
    5. binbook encode input.epub works on at least one simple EPUB.
    6. For xteink-x4-portrait, all pages are stored as GRAY2_PACKED.
    7. Text pages are stored as GRAY2_PACKED.
    8. Image pages are stored as GRAY2_PACKED for X4.
    9. Image pages are separate from text flow.
    10. Page data starts at a header-defined offset.
    11. Page blob offsets are relative to page_data_offset.
    12. The file is self-contained and requires no sidecar.
    13. The file contains version_major and version_minor.
    14. The reader rejects unsupported major versions.
    15. The reader rejects unsupported required reader features.
    16. The file contains no JSON, CBOR, protobuf, or MessagePack metadata sections.
    17. Required metadata is stored as fixed binary records and a UTF-8 string table.
    18. Typography policy is encoded.
    19. Image policy is encoded.
    20. Compression policy is encoded.
    21. Chrome policy is encoded.
    22. Rendition identity is encoded.
    23. CRC32 validation is implemented for nonzero checksum fields.
    24. SHA-256 policy/profile/rendition hashes are implemented.
    25. Page blob bounds and non-overlap validation are implemented.
    26. Progress monotonicity validation is implemented.
    27. Zero-length StringRefs are handled correctly.
    28. MIXED_RESERVED page_kind (3) is rejected in v0.1.
    29. X4 canonical GRAY2 to XTH LUT conversion is implemented/tested.
    30. SectionEntry is exactly 40 bytes; NavIndexEntry is exactly 48 bytes; PageIndexEntry is exactly 76 bytes.
    31. Row padding is zero-filled; readers tolerate non-zero unused bits in last byte of row.

## 18. Future Features

Possible future work:

    - Calibre plugin
    - firmware reader for Xteink X4
    - Xteink X3 profile
    - custom margin CLI options
    - multiple font files
    - EPUB embedded font support
    - better HTML/CSS rendering
    - image splitting
    - crop-to-fill image option
    - per-page compression selection
    - strip-based decoding
    - per-strip page blobs
    - 16-gray-level device profiles using GRAY4_PACKED
    - KOReader sync bridge
    - bookmarks
    - annotations
    - full-text search index
    - format validator
    - fuzz testing
    - sample BinBook corpus
    - optional debug metadata section, if needed

## 19. Non-goals for Now

Do not implement yet:

    - firmware
    - Calibre plugin
    - KOReader sync
    - annotations
    - text search
    - reflow
    - on-device font changes
    - exact EPUB CSS fidelity
    - inline image preservation
    - per-region mixed bit depth
    - JSON metadata inside .binbook
    - CBOR metadata inside .binbook
    - protobuf metadata inside .binbook
    - XTH/XTG as BinBook internal storage

## 20. Final Architecture Summary

BinBook is:

    - universal as a container
    - display-profiled per compiled file
    - source-aware
    - navigation-aware
    - rendition-aware
    - typography-aware
    - chrome-aware
    - integrity-checkable
    - sync-ready
    - compressed
    - seekable
    - non-reflowable
    - all-binary for required runtime metadata
    - designed for low-RAM readers

The compiler owns:

    - EPUB parsing
    - font rendering
    - typography policy
    - pagination
    - image extraction
    - image conversion
    - grayscale quantization
    - compression
    - CRC/hash generation
    - binary metadata/index generation
    - string table generation

The reader owns:

    - validation
    - page lookup
    - compressed blob caching
    - checksum validation
    - decompression
    - pixel unpacking/conversion
    - device-specific display conversion
    - UI/chrome rendering
    - display output
