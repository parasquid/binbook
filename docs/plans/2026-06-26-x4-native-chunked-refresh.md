# Xteink X4 Native Chunked Refresh Implementation Plan

> Historical implementation plan. Its crate paths and API examples describe a
> superseded refresh milestone; current boundaries are in
> [`2026-06-30-rust-modular-foundation-refactor.md`](2026-06-30-rust-modular-foundation-refactor.md).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Do not delegate implementation to subagents in this repository; `AGENTS.md` asks agents to work sequentially and keep a todo tracker current.

**Goal:** Make Xteink X4 BinBooks compiler-native for SSD1677 streaming and default page display to differential refresh with compiler-generated chunk metadata.

**Architecture:** The Python compiler emits physical-order SSD1677-native 1bpp planes for X4 pages, split into independently PackBits-compressed 16-row chunks. Firmware reads chunk and transition metadata, streams one bounded chunk buffer at a time, and uses a shared refresh policy to choose full grayscale seed/cleanup, adjacent dirty partial refresh, or full-screen BW differential partial refresh.

**Tech Stack:** Python 3.13 with `uv`, BinBook Python writer/reader, Rust `no_std` reference parser and firmware crates, `ssd1677-driver`, `xteink-hal`, PackBits RLE, Xteink X4 SSD1677 hardware.

---

## File Structure

- Modify `BINBOOK_FORMAT_SPEC.md`: document X4 native planes, 16-row chunking, section IDs 44/45, and refresh semantics.
- Modify `binbook/constants.py`: add `PAGE_CHUNK_INDEX = 44` and `PAGE_TRANSITION_INDEX = 45`.
- Modify `binbook/structs.py`: add `PageChunkIndexEntry` and `PageTransitionIndexEntry`.
- Modify `binbook/pixels.py`: add X4 native plane conversion helpers.
- Modify `binbook/writer.py` and `binbook/page_compiler.py`: emit native chunked plane pages and new metadata sections.
- Modify `binbook/reader.py` and `binbook/inspect.py`: validate and report chunk/transition metadata.
- Modify tests under `tests/`: cover plane conversion, writer layout, reader validation, inspect output, and transitions.
- Modify `rust/src/section.rs`, `rust/src/lib.rs`, and add focused parser modules if needed: parse chunk and transition sections.
- Modify `rust/tests/integration.rs`: cover new section parsing and X4 plane-size behavior.
- Modify `firmware/crates/binbook-fw/src/display.rs`: chunk lookup, stream orchestration, page render entrypoint.
- Add `firmware/crates/binbook-fw/src/refresh.rs`: refresh policy and dirty chunk types.
- Modify `firmware/crates/binbook-fw/src/lib.rs`: export the new refresh module.
- Modify `firmware/crates/binbook-fw/src/main.rs`: call the new stateful page renderer.
- Modify `firmware/crates/binbook-fw/tests/firmware_logic.rs`: refresh policy, chunk lookup, and render-state tests.
- Regenerate `firmware/crates/binbook-fw/fixtures/nav_probe.binbook`.
- Modify `docs/reference/squidscript-and-xteink-reference.md`, `docs/reference/xteink-x4-firmware-flashing.md`, and `HANDOFF.md`.

## Phase 1: Python Structs And Constants

### Task 1: Add Section Constants And Struct Tests

**Files:**
- Modify: `binbook/constants.py`
- Modify: `binbook/structs.py`
- Modify: `tests/test_structs.py`

- [ ] Add section IDs to `binbook/constants.py`:

```python
class SectionId(IntEnum):
    INVALID = 0
    STRING_TABLE = 1
    DISPLAY_PROFILE = 10
    LAYOUT_PROFILE = 11
    READER_REQUIREMENTS = 12
    SOURCE_IDENTITY = 20
    BOOK_METADATA = 21
    RENDITION_IDENTITY = 22
    FONT_POLICY = 30
    TYPOGRAPHY_POLICY = 31
    IMAGE_POLICY = 32
    COMPRESSION_POLICY = 33
    CHROME_POLICY = 34
    PAGE_INDEX = 40
    NAV_INDEX = 41
    PAGE_LABELS_RESERVED = 42
    CHAPTER_INDEX = 43
    PAGE_CHUNK_INDEX = 44
    PAGE_TRANSITION_INDEX = 45
    PAGE_DATA = 50
```

- [ ] Add both new section IDs to `REQUIRED_SECTIONS`.

- [ ] Add struct definitions to `binbook/structs.py`:

```python
PAGE_CHUNK_INDEX_ENTRY_SIZE = 24
PAGE_TRANSITION_INDEX_ENTRY_SIZE = 24

_PAGE_CHUNK_INDEX = struct.Struct("<IBBHHHIII")
_PAGE_TRANSITION_INDEX = struct.Struct("<IIIHHHHI")


@dataclass(frozen=True)
class PageChunkIndexEntry:
    page_number: int
    plane_slot: int
    chunk_index: int
    row_start: int
    row_count: int
    page_data_offset: int
    compressed_size: int
    uncompressed_size: int
    reserved0: int = 0

    def pack(self) -> bytes:
        return _PAGE_CHUNK_INDEX.pack(
            self.page_number,
            self.plane_slot,
            self.chunk_index,
            self.row_start,
            self.row_count,
            self.reserved0,
            self.page_data_offset,
            self.compressed_size,
            self.uncompressed_size,
        )

    @classmethod
    def unpack(cls, data: bytes, offset: int = 0) -> "PageChunkIndexEntry":
        (
            page_number,
            plane_slot,
            chunk_index,
            row_start,
            row_count,
            reserved0,
            page_data_offset,
            compressed_size,
            uncompressed_size,
        ) = _PAGE_CHUNK_INDEX.unpack_from(data, offset)
        return cls(
            page_number=page_number,
            plane_slot=plane_slot,
            chunk_index=chunk_index,
            row_start=row_start,
            row_count=row_count,
            reserved0=reserved0,
            page_data_offset=page_data_offset,
            compressed_size=compressed_size,
            uncompressed_size=uncompressed_size,
        )


@dataclass(frozen=True)
class PageTransitionIndexEntry:
    from_page_number: int
    to_page_number: int
    changed_chunk_mask: int
    first_changed_chunk: int
    changed_chunk_count: int
    flags: int = 0
    reserved0: int = 0
    reserved1: int = 0

    def pack(self) -> bytes:
        return _PAGE_TRANSITION_INDEX.pack(
            self.from_page_number,
            self.to_page_number,
            self.changed_chunk_mask,
            self.first_changed_chunk,
            self.changed_chunk_count,
            self.flags,
            self.reserved0,
            self.reserved1,
        )

    @classmethod
    def unpack(cls, data: bytes, offset: int = 0) -> "PageTransitionIndexEntry":
        (
            from_page_number,
            to_page_number,
            changed_chunk_mask,
            first_changed_chunk,
            changed_chunk_count,
            flags,
            reserved0,
            reserved1,
        ) = _PAGE_TRANSITION_INDEX.unpack_from(data, offset)
        return cls(
            from_page_number=from_page_number,
            to_page_number=to_page_number,
            changed_chunk_mask=changed_chunk_mask,
            first_changed_chunk=first_changed_chunk,
            changed_chunk_count=changed_chunk_count,
            flags=flags,
            reserved0=reserved0,
            reserved1=reserved1,
        )
```

- [ ] Add tests to `tests/test_structs.py`:

```python
def test_page_chunk_index_entry_roundtrip():
    entry = PageChunkIndexEntry(
        page_number=7,
        plane_slot=2,
        chunk_index=29,
        row_start=464,
        row_count=16,
        page_data_offset=123456,
        compressed_size=321,
        uncompressed_size=1600,
    )

    restored = PageChunkIndexEntry.unpack(entry.pack())

    assert restored == entry
    assert len(entry.pack()) == PAGE_CHUNK_INDEX_ENTRY_SIZE


def test_page_transition_index_entry_roundtrip():
    entry = PageTransitionIndexEntry(
        from_page_number=4,
        to_page_number=5,
        changed_chunk_mask=0b10101,
        first_changed_chunk=0,
        changed_chunk_count=5,
    )

    restored = PageTransitionIndexEntry.unpack(entry.pack())

    assert restored == entry
    assert len(entry.pack()) == PAGE_TRANSITION_INDEX_ENTRY_SIZE
```

- [ ] Run:

```bash
uv run pytest tests/test_structs.py -q
```

Expected result after implementation: all struct tests pass.

## Phase 2: Native Plane Generation

### Task 2: Add X4 Plane Conversion Helpers

**Files:**
- Modify: `binbook/pixels.py`
- Create or modify: `tests/test_x4_native_planes.py`

- [ ] Add constants and helpers to `binbook/pixels.py`:

```python
X4_PHYSICAL_WIDTH = 800
X4_PHYSICAL_HEIGHT = 480
X4_ROW_BYTES = 100
X4_CHUNK_ROWS = 16
X4_CHUNK_BYTES = X4_ROW_BYTES * X4_CHUNK_ROWS
X4_CHUNKS_PER_PLANE = X4_PHYSICAL_HEIGHT // X4_CHUNK_ROWS


def x4_logical_to_physical(logical_x: int, logical_y: int) -> tuple[int, int]:
    return 799 - logical_y, logical_x


def _clear_native_bit(row: bytearray, physical_x: int) -> None:
    ram_x = X4_PHYSICAL_WIDTH - 1 - physical_x
    row[ram_x // 8] &= ~(0x80 >> (ram_x % 8)) & 0xFF


def gray2_packed_to_x4_native_planes(data: bytes, logical_width: int, logical_height: int) -> tuple[bytes, bytes, bytes]:
    if logical_width != 480 or logical_height != 800:
        raise ValueError("xteink-x4-portrait native planes require logical 480x800 input")
    pixels = unpack_gray2(data, logical_width, logical_height)
    msb_rows = [bytearray([0xFF] * X4_ROW_BYTES) for _ in range(X4_PHYSICAL_HEIGHT)]
    lsb_rows = [bytearray([0xFF] * X4_ROW_BYTES) for _ in range(X4_PHYSICAL_HEIGHT)]
    bw_rows = [bytearray([0xFF] * X4_ROW_BYTES) for _ in range(X4_PHYSICAL_HEIGHT)]

    for logical_y in range(logical_height):
        for logical_x in range(logical_width):
            gray = pixels[logical_y * logical_width + logical_x]
            physical_x, physical_y = x4_logical_to_physical(logical_x, logical_y)
            if gray in (0, 1):
                _clear_native_bit(msb_rows[physical_y], physical_x)
            if gray in (0, 2):
                _clear_native_bit(lsb_rows[physical_y], physical_x)
            if gray < 2:
                _clear_native_bit(bw_rows[physical_y], physical_x)

    return (
        b"".join(msb_rows),
        b"".join(lsb_rows),
        b"".join(bw_rows),
    )


def split_x4_plane_chunks(plane: bytes) -> list[bytes]:
    expected = X4_ROW_BYTES * X4_PHYSICAL_HEIGHT
    if len(plane) != expected:
        raise ValueError(f"expected {expected} bytes, got {len(plane)}")
    return [
        plane[i * X4_CHUNK_BYTES : (i + 1) * X4_CHUNK_BYTES]
        for i in range(X4_CHUNKS_PER_PLANE)
    ]
```

- [ ] Add tests with a mostly white full-size packed page so expected bits are
  easy to inspect:

```python
from binbook.pixels import (
    X4_CHUNK_BYTES,
    X4_CHUNKS_PER_PLANE,
    X4_ROW_BYTES,
    gray2_packed_to_x4_native_planes,
    pack_gray2,
    split_x4_plane_chunks,
)


def _single_pixel_page(gray: int, x: int = 0, y: int = 0) -> bytes:
    pixels = [3] * (480 * 800)
    pixels[y * 480 + x] = gray
    return pack_gray2(pixels, 480, 800)


def test_x4_native_planes_map_black_pixel_to_all_planes():
    msb, lsb, bw = gray2_packed_to_x4_native_planes(_single_pixel_page(0), 480, 800)

    assert len(msb) == 48_000
    assert len(lsb) == 48_000
    assert len(bw) == 48_000
    assert msb[99] == 0x7F
    assert lsb[99] == 0x7F
    assert bw[99] == 0x7F


def test_x4_native_planes_map_gray_levels():
    dark_msb, dark_lsb, dark_bw = gray2_packed_to_x4_native_planes(_single_pixel_page(1), 480, 800)
    light_msb, light_lsb, light_bw = gray2_packed_to_x4_native_planes(_single_pixel_page(2), 480, 800)

    assert dark_msb[99] == 0x7F
    assert dark_lsb[99] == 0xFF
    assert dark_bw[99] == 0x7F
    assert light_msb[99] == 0xFF
    assert light_lsb[99] == 0x7F
    assert light_bw[99] == 0xFF


def test_split_x4_plane_chunks_returns_30_1600_byte_chunks():
    plane = bytes(range(256)) * 188
    chunks = split_x4_plane_chunks(plane[:48_000])

    assert len(chunks) == X4_CHUNKS_PER_PLANE
    assert {len(chunk) for chunk in chunks} == {X4_CHUNK_BYTES}
    assert chunks[0] == plane[:X4_CHUNK_BYTES]
    assert chunks[1] == plane[X4_CHUNK_BYTES : 2 * X4_CHUNK_BYTES]
```

- [ ] Run:

```bash
uv run pytest tests/test_x4_native_planes.py -q
```

Expected result after implementation: tests pass.

## Phase 3: Writer Emits Chunked X4 Pages

### Task 3: Replace Single-Blob X4 Page Encoding

**Files:**
- Modify: `binbook/writer.py`
- Modify: `binbook/page_compiler.py`
- Modify: `tests/test_roundtrip.py`
- Modify: `tests/test_validation.py`
- Add or modify: `tests/test_x4_chunked_writer.py`

- [ ] Replace `EncodedPage.compressed` with fields that can represent native
  chunks while preserving the existing source metadata:

```python
@dataclass(frozen=True)
class EncodedPlane:
    slot: int
    chunks: tuple[bytes, ...]
    uncompressed_size: int

    @property
    def compressed(self) -> bytes:
        return b"".join(self.chunks)


@dataclass(frozen=True)
class EncodedPage:
    planes: tuple[EncodedPlane, ...]
    page_crc32: int
    page_kind: int = PageKind.IMAGE
    source_spine_index: int = UINT32_MAX
    chapter_nav_index: int = UINT32_MAX
```

- [ ] Add a helper in `page_compiler.py` to create native X4 pages:

```python
def encoded_page(packed: bytes, kind: int, spine_index: int) -> EncodedPage:
    msb, lsb, bw = gray2_packed_to_x4_native_planes(packed, 480, 800)
    planes: list[EncodedPlane] = []
    crc_parts: list[bytes] = []
    for slot, plane in enumerate((msb, lsb, bw)):
        chunks = tuple(encode_packbits(chunk) for chunk in split_x4_plane_chunks(plane))
        encoded = EncodedPlane(slot=slot, chunks=chunks, uncompressed_size=len(plane))
        planes.append(encoded)
        crc_parts.append(encoded.compressed)
    return EncodedPage(
        planes=tuple(planes),
        page_crc32=crc32(b"".join(crc_parts)),
        page_kind=kind,
        source_spine_index=spine_index,
    )
```

- [ ] Update `writer.build_binbook()` to concatenate plane chunks into
  `PAGE_DATA`, build a `PAGE_CHUNK_INDEX` section, and build a
  `PAGE_TRANSITION_INDEX` section before finalizing the section table.

- [ ] In `_page_index()`, set `plane_bitmap = 0x07`, set all three plane offsets
  and sizes, and set `plane_compression = [RLE_PACKBITS, RLE_PACKBITS,
  RLE_PACKBITS, 0]`.

- [ ] Add a writer test:

```python
def test_x4_writer_emits_three_chunked_planes(tmp_path):
    profile = get_profile("xteink-x4-portrait")
    packed = pack_gray2([3] * (480 * 800), 480, 800)
    page = encoded_page(packed, PageKind.TEXT, UINT32_MAX)
    path = tmp_path / "chunked.binbook"

    path.write_bytes(build_binbook([page], profile, source_name="chunked-test"))
    reader = BinBookReader.open(path, validate=True)

    assert reader.pages[0].plane_dir.bitmap == 0x07
    assert reader.pages[0].plane_dir.sizes[0] > 0
    assert reader.pages[0].plane_dir.sizes[1] > 0
    assert reader.pages[0].plane_dir.sizes[2] > 0
    assert len(reader.page_chunks) == 90
    assert {entry.uncompressed_size for entry in reader.page_chunks} == {1600}
```

- [ ] Add a transition test:

```python
def test_adjacent_transition_index_marks_changed_chunks(tmp_path):
    profile = get_profile("xteink-x4-portrait")
    white = pack_gray2([3] * (480 * 800), 480, 800)
    black = pack_gray2([0] * (480 * 800), 480, 800)
    path = tmp_path / "transitions.binbook"

    path.write_bytes(build_binbook([
        encoded_page(white, PageKind.TEXT, UINT32_MAX),
        encoded_page(black, PageKind.TEXT, UINT32_MAX),
    ], profile, source_name="transition-test"))
    reader = BinBookReader.open(path, validate=True)

    forward = next(t for t in reader.page_transitions if t.from_page_number == 0 and t.to_page_number == 1)
    backward = next(t for t in reader.page_transitions if t.from_page_number == 1 and t.to_page_number == 0)
    assert forward.changed_chunk_mask == (1 << 30) - 1
    assert backward.changed_chunk_mask == (1 << 30) - 1
    assert forward.first_changed_chunk == 0
    assert forward.changed_chunk_count == 30
```

- [ ] Run:

```bash
uv run pytest tests/test_x4_native_planes.py tests/test_x4_chunked_writer.py tests/test_roundtrip.py tests/test_validation.py -q
```

Expected result after implementation: all selected tests pass.

## Phase 4: Reader And Inspect Support

### Task 4: Parse And Validate New Sections

**Files:**
- Modify: `binbook/reader.py`
- Modify: `binbook/inspect.py`
- Modify: `tests/test_inspect.py`
- Modify: `tests/test_sections.py`

- [ ] In `BinBookReader`, parse section 44 into `self.page_chunks` and section
  45 into `self.page_transitions`.

- [ ] Validate for X4 pages:
  - section 44 exists;
  - each page has exactly 90 chunk records;
  - every chunk record is inside `PAGE_DATA`;
  - chunk `uncompressed_size` is `1600`;
  - transition records only reference valid page numbers;
  - adjacent page pairs have both forward and backward records.

- [ ] Update inspect JSON output to include:

```json
{
  "chunk_count": 90,
  "transition_count": 2
}
```

Use the existing inspect output style rather than introducing a new CLI mode.

- [ ] Run:

```bash
uv run pytest tests/test_sections.py tests/test_inspect.py tests/test_validation.py -q
```

Expected result after implementation: selected tests pass.

## Phase 5: Rust Reference Parser

### Task 5: Parse Chunk And Transition Metadata In Rust

**Files:**
- Modify: `rust/src/section.rs`
- Modify: `rust/src/lib.rs`
- Create: `rust/src/chunk_index.rs`
- Create: `rust/src/transition_index.rs`
- Modify: `rust/tests/integration.rs`

- [ ] Add section constants in `rust/src/section.rs`:

```rust
pub const SECTION_PAGE_CHUNK_INDEX: u16 = 44;
pub const SECTION_PAGE_TRANSITION_INDEX: u16 = 45;
pub const PAGE_CHUNK_INDEX_ENTRY_SIZE: usize = 24;
pub const PAGE_TRANSITION_INDEX_ENTRY_SIZE: usize = 24;
```

- [ ] Add the optional sections to `RequiredSections` as `Option<Section>` fields.

- [ ] Fix `page_plane_uncompressed_size()` for e-paper planes:

```rust
pub fn page_plane_uncompressed_size(pixel_format: u16, width: u16, height: u16) -> usize {
    let pixels = width as usize * height as usize;
    match pixel_format {
        page_index::PIXEL_FORMAT_GRAY1_PACKED => pixels / 8,
        page_index::PIXEL_FORMAT_GRAY2_PACKED => pixels / 8,
        4 => pixels / 4,
        8 => pixels * 2,
        16 => pixels * 3,
        32 => pixels * 4,
        _ => pixels,
    }
}
```

- [ ] Add parser structs:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageChunkEntry {
    pub page_number: u32,
    pub plane_slot: u8,
    pub chunk_index: u8,
    pub row_start: u16,
    pub row_count: u16,
    pub page_data_offset: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageTransitionEntry {
    pub from_page_number: u32,
    pub to_page_number: u32,
    pub changed_chunk_mask: u32,
    pub first_changed_chunk: u16,
    pub changed_chunk_count: u16,
    pub flags: u16,
}
```

- [ ] Add `BinBook` methods:

```rust
pub fn page_chunk_entry(&mut self, index: u32) -> Result<chunk_index::PageChunkEntry, Error>;
pub fn page_transition_entry(&mut self, index: u32) -> Result<transition_index::PageTransitionEntry, Error>;
pub fn chunk_count(&self) -> u32;
pub fn transition_count(&self) -> u32;
```

- [ ] Add tests:

```rust
#[test]
fn gray2_plane_uncompressed_size_is_native_1bpp_plane() {
    assert_eq!(binbook::page_plane_uncompressed_size(2, 800, 480), 48_000);
}

#[test]
fn chunk_and_transition_counts_are_exposed() {
    let mut book = open_fixture();

    assert!(book.chunk_count() > 0);
    assert!(book.transition_count() > 0);
    let chunk = book.page_chunk_entry(0).unwrap();
    assert_eq!(chunk.uncompressed_size, 1600);
}
```

- [ ] Run:

```bash
cd rust && cargo test
```

Expected result after implementation: Rust reference crate tests pass.

## Phase 6: Firmware Refresh Policy

### Task 6: Add Host-Tested Refresh Policy

**Files:**
- Create: `firmware/crates/binbook-fw/src/refresh.rs`
- Modify: `firmware/crates/binbook-fw/src/lib.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] Add `pub mod refresh;` to `lib.rs`.

- [ ] Create `refresh.rs`:

```rust
pub const X4_CHUNK_COUNT: u8 = 30;
pub const DEFAULT_FULL_REFRESH_CADENCE: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshDecision {
    FullGrayscale,
    AdjacentDirtyPartial { changed_chunk_mask: u32 },
    FullScreenDifferential,
    Noop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshState {
    previous_page: Option<u32>,
    fast_refresh_count: u32,
    full_refresh_cadence: u32,
}

impl RefreshState {
    pub const fn new() -> Self {
        Self {
            previous_page: None,
            fast_refresh_count: 0,
            full_refresh_cadence: DEFAULT_FULL_REFRESH_CADENCE,
        }
    }

    pub fn decide(&self, target_page: u32, transition_mask: Option<u32>) -> RefreshDecision {
        let Some(previous_page) = self.previous_page else {
            return RefreshDecision::FullGrayscale;
        };
        if previous_page == target_page {
            return RefreshDecision::Noop;
        }
        if self.fast_refresh_count >= self.full_refresh_cadence {
            return RefreshDecision::FullGrayscale;
        }
        if let Some(mask) = transition_mask {
            return RefreshDecision::AdjacentDirtyPartial { changed_chunk_mask: mask };
        }
        RefreshDecision::FullScreenDifferential
    }

    pub fn record_success(&mut self, target_page: u32, decision: RefreshDecision) {
        self.previous_page = Some(target_page);
        match decision {
            RefreshDecision::FullGrayscale => self.fast_refresh_count = 0,
            RefreshDecision::AdjacentDirtyPartial { .. } | RefreshDecision::FullScreenDifferential => {
                self.fast_refresh_count = self.fast_refresh_count.saturating_add(1);
            }
            RefreshDecision::Noop => {}
        }
    }

    pub fn previous_page(&self) -> Option<u32> {
        self.previous_page
    }
}
```

- [ ] Add tests:

```rust
#[test]
fn refresh_policy_seeds_with_full_grayscale() {
    let state = RefreshState::new();

    assert_eq!(state.decide(0, None), RefreshDecision::FullGrayscale);
}

#[test]
fn refresh_policy_uses_adjacent_dirty_mask_after_seed() {
    let mut state = RefreshState::new();
    let seed = state.decide(0, None);
    state.record_success(0, seed);

    assert_eq!(
        state.decide(1, Some(0b101)),
        RefreshDecision::AdjacentDirtyPartial { changed_chunk_mask: 0b101 }
    );
}

#[test]
fn refresh_policy_uses_full_screen_differential_for_jump_without_transition() {
    let mut state = RefreshState::new();
    let seed = state.decide(0, None);
    state.record_success(0, seed);

    assert_eq!(state.decide(9, None), RefreshDecision::FullScreenDifferential);
}

#[test]
fn refresh_policy_cleanup_after_five_fast_refreshes() {
    let mut state = RefreshState::new();
    let seed = state.decide(0, None);
    state.record_success(0, seed);
    for page in 1..=5 {
        let decision = state.decide(page, Some(1));
        if page < 5 {
            assert!(matches!(decision, RefreshDecision::AdjacentDirtyPartial { .. }));
        }
        state.record_success(page, decision);
    }

    assert_eq!(state.decide(6, Some(1)), RefreshDecision::FullGrayscale);
}

#[test]
fn failed_render_does_not_advance_previous_page() {
    let state = RefreshState::new();

    assert_eq!(state.previous_page(), None);
}
```

- [ ] Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic refresh_policy
```

Expected result after implementation: refresh policy tests pass.

## Phase 7: Firmware Chunk Streaming

### Task 7: Add Chunk Lookup And Streaming Entry Points

**Files:**
- Modify: `firmware/crates/binbook-fw/src/display.rs`
- Modify: `firmware/crates/binbook-fw/src/main.rs`
- Modify: `firmware/crates/binbook-fw/tests/firmware_logic.rs`

- [ ] Replace the old single-plane predicate with X4 native validation:

```rust
pub fn is_supported_x4_native_gray2_page(page: &binbook::PageInfo) -> bool {
    page.pixel_format == binbook::page_index::PIXEL_FORMAT_GRAY2_PACKED
        && page.compression_method == binbook::page_index::COMPRESSION_RLE_PACKBITS
        && page.stored_width == DISPLAY_WIDTH
        && page.stored_height == DISPLAY_HEIGHT
        && (page.plane_dir.bitmap & 0x07) == 0x07
}
```

- [ ] Add chunk-slice lookup using Rust chunk metadata parsed in Phase 5. The
  helper must return a slice from embedded book bytes without copying a full
  page:

```rust
pub fn embedded_chunk_slice<'a>(
    book_bytes: &'a [u8],
    page_data_offset: u64,
    chunk: &binbook::chunk_index::PageChunkEntry,
) -> Option<&'a [u8]> {
    let offset = page_data_offset.checked_add(chunk.page_data_offset as u64)?;
    let start = usize::try_from(offset).ok()?;
    let size = usize::try_from(chunk.compressed_size).ok()?;
    let end = start.checked_add(size)?;
    if end > book_bytes.len() {
        return None;
    }
    Some(&book_bytes[start..end])
}
```

- [ ] Add a display orchestration function with this signature:

```rust
pub fn display_page_with_policy<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut binbook::BinBook<&[u8], &mut [u8; BINBOOK_SCRATCH_BYTES]>,
    book_bytes: &[u8],
    delay: &dyn xteink_hal::Delay,
    refresh_state: &mut RefreshState,
    target_page: u32,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
;
```

The implementation must keep this function small by delegating page validation,
transition lookup, full grayscale streaming, dirty/full-screen BW differential
streaming, and state update. Update `RefreshState` only after all chunk writes
and the SSD1677 refresh return `Ok(())`.

- [ ] For dirty partial refresh, use chunk-aligned windows. Each chunk window is:
  - `x = 0`
  - `y = chunk_index * 16`
  - `width = 800`
  - `height = 16`

- [ ] Stream previous page BW plane chunks to red RAM and target page BW chunks
  to black RAM before triggering `RefreshMode::Partial`.

- [ ] Add host tests for chunk-slice bounds and validation. Add policy tests in
  Phase 6 before display orchestration tests so failures are easier to isolate.

- [ ] Run:

```bash
cd firmware && cargo test -p binbook-fw --test firmware_logic
```

Expected result after implementation: binbook-fw host tests pass.

## Phase 8: Fixtures, Docs, And Hardware Verification

### Task 8: Regenerate Fixtures And Update Docs

**Files:**
- Modify: `firmware/scripts/build-nav-probe-fixture.py`
- Modify: `firmware/crates/binbook-fw/fixtures/nav_probe.binbook`
- Modify: `BINBOOK_FORMAT_SPEC.md`
- Modify: `docs/reference/squidscript-and-xteink-reference.md`
- Modify: `docs/reference/xteink-x4-firmware-flashing.md`
- Modify: `HANDOFF.md`

- [ ] Update the fixture builder so its self-checks assert:

```python
for page in reader.pages:
    assert page.plane_dir.bitmap == 0x07
assert len(reader.page_chunks) == len(reader.pages) * 3 * 30
assert len(reader.page_transitions) == max(0, len(reader.pages) - 1) * 2
```

- [ ] Regenerate the fixture:

```bash
uv run firmware/scripts/build-nav-probe-fixture.py
```

Expected result: script prints the regenerated page and chunk summary and exits
successfully.

- [ ] Update docs to describe:
  - X4 native plane storage;
  - 16-row chunking;
  - compiler-generated adjacent transition maps;
  - first/full cleanup grayscale refresh;
  - adjacent dirty partial refresh;
  - arbitrary full-screen BW differential fallback.

- [ ] Run full Python tests:

```bash
uv run pytest -q
```

Expected result: pass.

- [ ] Run firmware host tests:

```bash
cd firmware && cargo test --workspace
```

Expected result: pass.

- [ ] Build firmware with the pinned nightly command from `AGENTS.md`:

```bash
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
```

Expected result: release firmware binary builds.

- [ ] Hardware verification requires escalated host access. Flash the Xteink X4
  using the repo’s current flash script or update the script if the binary name
  changes. Do not run serial, flashing, or monitor commands in parallel.

- [ ] Collect debug evidence behind `debug-log` or an equivalent compile-time
  guard showing:
  - first page render uses `FullGrayscale`;
  - adjacent page turn uses a transition mask;
  - arbitrary jump uses `FullScreenDifferential`;
  - fifth fast refresh triggers `FullGrayscale`.

- [ ] Update `HANDOFF.md` with:
  - commands run;
  - pass/fail status;
  - hardware result;
  - any remaining blockers.

## Final Verification Checklist

- [ ] `uv run pytest -q` passes.
- [ ] `cd firmware && cargo test --workspace` passes.
- [ ] Pinned nightly firmware release build passes.
- [ ] `nav_probe.binbook` has native planes, chunk index, and transition index.
- [ ] Hardware evidence confirms default differential refresh behavior.
- [ ] `HANDOFF.md` reflects the current state without relying on chat context.
