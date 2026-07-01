# Rust Multi-Format BinBook Compiler Implementation Plan

> **For agentic workers:** Execute this plan sequentially in the current branch/worktree. Do not delegate to subagents. Keep a todo tracker current. Every implementation task uses strict RED -> GREEN -> REFACTOR TDD: add the smallest discriminating test, run it and capture the intended failure, implement only enough to pass, then run the focused gate before continuing.

**Goal:** Replace the Python BinBook compiler, decoder, and inspector with a Rust executable named `binbook` that compiles static images and EPUB 2/3, while preserving the reusable firmware crate boundaries, supporting a future browser/WASM encoder, and proving the generated X4 fixture on live hardware.

**Architecture:** Path-free compiler crates accept borrowed bytes/resources and write to `Write + Seek`; the native CLI owns paths, atomic output, and input discovery. Existing reusable crates remain the owners of BinBook wire validation, GRAY2 conversion, and X4 geometry. Python is reduced to a transitional `binbook-support` executable containing only the viewer and kerning proof.

**Tech Stack:** Rust 2021/2024 workspace, `rbook` 0.7 read-only, `html5ever` 0.39, `cssparser` 0.37, `cosmic-text` 0.19, `image` 0.25 with explicit PNG/JPEG/WebP features, `fast_image_resize` 6, `resvg` 0.47, `wuff` 0.2, `clap`, `thiserror`, `crc32fast`, RustCrypto `md-5`/`sha2`, Python 3.13/`uv` only for viewer, proof tooling, fixture source-image generation, and compatibility tests.

---

## Non-Negotiable Scope

### Must ship

- Rust package, library, and executable named `binbook`; move the existing `cli/` crate to `crates/binbook/`.
- Stable commands:

  ```text
  binbook encode <INPUT> -o <OUTPUT> [--input-format auto|image|epub]
      [--profile xteink-x4-portrait] [--pixel-format gray2|gray1]
      [--no-dither] [--font-family literata|opendyslexic|sans-serif]
  binbook decode <BOOK> --page <N> -o <PNG>
  binbook inspect <BOOK> [--validate] [--strict] [--json]
  binbook diag ...
  ```

- Image compilation from one static PNG/JPEG/WebP/SVG or a non-recursive directory of those formats.
- EPUB 2/3 compilation with OPF metadata, linear spine, EPUB3 nav, NCX fallback, fragment-to-page navigation, common reflow CSS, embedded fonts, static images, and deterministic warnings/fallbacks.
- GRAY1 single-plane and X4 staged-GRAY2 three-plane output.
- Strict Rust inspection/validation and logical-orientation PNG decoding.
- Required `FONT_RESOURCE_INDEX` binary section and complete spec/reader/fixture migration.
- Compiler crates compile for `wasm32-unknown-unknown`; no browser binding or UI in this plan.
- Canonical firmware navigation fixture compiled by Rust and verified by flash, serial diagnostics, independent state queries, and a fresh `/dev/video1` capture.

### Must not ship

- No `encode-png-folder` alias.
- No Python EPUB encoder, decoder, inspector, or PNG-folder compiler after cutover.
- No PDF, CBZ, browser UI, `wasm-bindgen` API, JavaScript execution, DRM circumvention, system-font discovery, Rayon, or browser-grade CSS engine.
- No LZ4 encoder. Existing host LZ4 decode support may be reused.
- No firmware behavior, diagnostic protocol, SSD1677 sequence, refresh-policy, or serial feature-gate changes.
- No JSON/CBOR/protobuf content in `.binbook`.
- No historical-document rewrites.

## Locked Interfaces and Behavior

### Compiler crate graph

```text
binbook (native CLI)
├── binbook-compiler       path-free source dispatch and compile API
│   ├── binbook-epub       EPUB package/resources and styled document creation
│   │   └── binbook-document shared styled-flow intermediate representation
│   ├── binbook-render     reflow/pagination over binbook-document
│   ├── binbook-image      static image decode/resize/quantization orchestration
│   └── binbook-encode     deterministic BinBook assembly
│       └── binbook-compress  BinBook PackBits encoder
├── binbook-core           wire codecs, parsing, strict validation
├── binbook-decompress     existing PackBits/LZ4 decode
├── gray2-render           reusable quantization/plane transforms
├── xteink-x4-display      existing X4 dimensions/rotation/chunk geometry
└── binbook-diagnostic-protocol
```

`binbook-compiler`, `binbook-document`, `binbook-epub`, `binbook-render`, `binbook-image`, `binbook-encode`, and `binbook-compress` must not depend on `Path`, `File`, serial APIs, firmware crates, OS fonts, or system services. Native path handling belongs only in `crates/binbook`.

### Path-free compiler API

Create these public concepts in `crates/binbook-compiler/src/lib.rs`, split into focused modules before any file exceeds 250 logical lines:

```rust
pub struct NamedInput<'a> {
    pub name: &'a str,
    pub bytes: &'a [u8],
}

pub enum CompileSource<'a> {
    ImageSequence(&'a [NamedInput<'a>]),
    Epub(NamedInput<'a>),
}

pub enum ProfileId {
    XteinkX4Portrait,
}

pub enum StoragePixelFormat {
    Gray1,
    Gray2,
}

pub enum FontFamily {
    Literata,
    OpenDyslexic,
}

pub struct CompileOptions {
    pub profile: ProfileId,
    pub pixel_format: StoragePixelFormat,
    pub dither: bool,
    pub forced_font: Option<FontFamily>,
}

pub enum CompileEvent<'a> {
    Progress { phase: CompilePhase, completed: u32, total: u32 },
    Warning(&'a CompileWarning),
}

pub trait CompileObserver {
    fn on_event(&mut self, event: CompileEvent<'_>);
}

pub fn compile<W: std::io::Write + std::io::Seek>(
    source: CompileSource<'_>,
    options: &CompileOptions,
    output: &mut W,
    observer: &mut impl CompileObserver,
) -> Result<CompileSummary, CompileError>;
```

`sans-serif` is a CLI alias for `FontFamily::OpenDyslexic`; it is not a third library variant. `CompilePhase` is an exhaustive enum containing `ReadSource`, `Parse`, `Layout`, `Rasterize`, `Compress`, `Assemble`, and `Validate`. `CompileWarning` carries a stable code enum, a human message, and optional resource/spine context. `CompileSummary` reports page count, warning count, output byte length, source format, and output pixel format.

Use typed error variants for source detection, EPUB package/resource, HTML/CSS, font, image, render, compression, format assembly, and output failures. Do not use stringly errors in library APIs.

### Input behavior

- `auto`: directory -> image sequence; EPUB ZIP/mimetype -> EPUB; static image magic/extension -> image; otherwise error.
- Image directory enumeration is non-recursive, accepts UTF-8 filenames only, sorts normalized relative names lexically, and supports mixed static formats.
- Unsupported/animated/multipage directory entries emit warning codes and are skipped. Fail if zero encodable pages remain.
- A single unsupported/animated/multipage input fails because no valid book can be produced.
- EPUB unsupported images/content emit warnings and render deterministic alt text or a crossed placeholder box; compilation continues unless the spine/package is unreadable.
- Native CLI writes to a sibling temporary file and renames only after compilation and strict validation succeed. Failure leaves no output or temporary file.

### EPUB reflow boundary

Support HTML defaults, linked/internal/inline CSS, specificity/source order/inheritance, block and inline flow, headings, paragraphs, `br`, `hr`, emphasis, links, ordered/unordered lists, blockquotes, preformatted text, figures, inline/static images, margins, padding, font family/size/weight/style/stretch, line height, letter/word spacing, indentation, alignment, whitespace, and page-break properties.

Basic tables use equal-width columns inside the content box, row height equal to the tallest cell, and page breaks only between rows. A row taller than one page degrades to sequential cell blocks and emits a warning.

Treat floats, positioned elements, flex/grid, columns, and unsupported display modes as normal block/inline flow with warnings. Omit scripts, forms, audio, and video with warnings. Respect `display:none`. Respect soft hyphens but do not add dictionary hyphenation.

Use `cosmic-text` for Unicode shaping, bidi, word-or-glyph wrapping, styled spans, fallback, and glyph rasterization. Render at 2x logical resolution and Lanczos-downsample before GRAY quantization.

Honor embedded TTF/OTF/WOFF/WOFF2 fonts and standard EPUB font obfuscation. Reject DRM-protected resources. Without `--font-family`, honor CSS font selection and fall back to bundled Literata; with `--font-family`, ignore EPUB `@font-face` and force the selected bundled family.

### FONT_RESOURCE_INDEX wire contract

Update `BINBOOK_FORMAT_SPEC.md` before implementation. Add required section ID `35`, record size `80`, sorted by normalized source path then face index:

```c
struct FontResourceIndexEntry {
    u32 font_index;
    u16 source_kind;       // 1=bundled, 2=epub
    u16 flags;             // bit0=used, bit1=primary, bit2=fallback, bit3=obfuscated
    u16 weight;            // CSS numeric weight
    u16 stretch_milli;     // 1000 = normal
    u8  style;             // 0=normal, 1=italic, 2=oblique
    u8  reserved0;
    u16 reserved1;
    StringRef family;
    StringRef source_path;
    u8  sha256[32];        // decoded font bytes
    u32 face_index;
    u32 reserved2;
    u8  reserved3[8];
};
```

All reserved fields must be zero. Include only faces actually used during rasterization, including fallback faces. In EPUB-preserve mode `FONT_POLICY.font_sha256` is SHA-256 of the complete section bytes. In forced mode it is the forced font file digest. An image-only book has an empty section and a zero font digest.

## Task 1: Freeze Baselines and Add the Wire-Spec RED Tests

**Files:**
- Modify: `BINBOOK_FORMAT_SPEC.md`
- Modify: `crates/binbook-core/src/section.rs`
- Create: `crates/binbook-core/src/font_resource.rs`
- Create: `crates/binbook-core/tests/font_resources.rs`
- Modify: `tests/test_validation.py`

- [x] Record baseline outputs before changing code:

  ```bash
  cargo test --workspace
  cargo test -p binbook-fw --features diagnostic-console
  uv run pytest -q
  sha256sum crates/binbook-core/tests/fixtures/nav_probe.binbook \
    crates/xteink-x4-display/tests/fixtures/nav_probe.binbook \
    firmware/crates/binbook-fw/fixtures/nav_probe.binbook
  ```

  Save relevant output in `/tmp/rust-compiler-baseline.txt`. Any failure must be classified as pre-existing before proceeding.

- [x] Update the format spec with the exact section definition above, required-section ordering, string-reference rules, digest rule, and validation checklist.
- [x] Add Rust tests that expect section ID 35, reject a missing font section, reject nonzero reserved fields, reject duplicate/non-contiguous indices, reject invalid string refs, and accept an empty section.
- [x] Add a Python wire-record RED test and the minimum reader/writer compatibility required to keep the required-section migration green. The original delayed-Python step was superseded because making section 35 required would otherwise leave every existing Python-generated fixture invalid.
- [x] Run:

  ```bash
  cargo test -p binbook-core --test font_resources -- --nocapture
  uv run pytest -q tests/test_validation.py
  ```

  Expected RED: Rust lacks the font record/required section and Python lacks the section contract.
- [x] Implement the no-allocation record parser/types, required-section directory update, empty Python writer section, transitional reader compatibility, and regenerated canonical fixtures.
- [x] Run `cargo test -p binbook-core`; expected GREEN.
- [x] Commit exact paths with `feat(binbook-core): add font resource index records`.

## Task 2: Add Shared Wire Encoders and Strict Validation

**Files:**
- Modify/split: `crates/binbook-core/src/header.rs`, `section.rs`, `page.rs`, `chunk.rs`, `transition.rs`, `navigation.rs`, `profile.rs`
- Create: `crates/binbook-core/src/encode.rs`
- Create: `crates/binbook-core/src/validate.rs`
- Create: `crates/binbook-core/tests/encoding.rs`
- Create: `crates/binbook-core/tests/strict_validation.rs`

- [x] Add RED tests for exact little-endian bytes of every header/section/page/chunk/transition/nav/chapter/font record and for undersized output buffers reporting exact `required`/`provided` sizes.
- [x] Add corruption-matrix RED tests for section/page CRCs, bounds, ordering, reserved bytes, required features, profiles, strings, planes, chunks, transitions, nav/chapter links, and font records. Each mutation must assert a distinct typed `ValidationCode`.
- [x] Run the two new test targets and confirm failures are due to missing encoders/validator.
- [x] Implement `encode_into(&mut [u8])` on typed records and a visitor-based `validate_all` API. Continue after independent validation errors only when bounds remain safe; never index corrupted offsets.
- [x] Refactor existing parsers to share constants/types with encoders instead of duplicating layouts.
- [x] Run:

  ```bash
  cargo test -p binbook-core --test encoding
  cargo test -p binbook-core --test strict_validation
  cargo test -p binbook-core
  cargo check -p binbook-core --no-default-features --target riscv32imc-unknown-none-elf
  ```

- [x] Commit `feat(binbook-core): add wire encoders and strict validation`.

## Task 3: Implement the PackBits Encoder

**Files:**
- Create: `crates/binbook-compress/Cargo.toml`
- Create: `crates/binbook-compress/src/lib.rs`
- Create: `crates/binbook-compress/src/packbits.rs`
- Create: `crates/binbook-compress/tests/packbits.rs`
- Modify: root `Cargo.toml`

- [x] Add RED golden/property tests for empty input, 1/2/127/128-byte repeat and literal boundaries, alternating bytes, split runs, incompressible data, and the BinBook-specific `0x80` one-byte repeat rule.
- [x] Require every encoded sample to decode exactly through `binbook-decompress`; include inputs larger than 8 KiB.
- [x] Implement deterministic PackBits encoding only. Use caller-provided output for the low-level API and a host convenience `Vec` wrapper.
- [x] Run:

  ```bash
  cargo test -p binbook-compress
  cargo clippy -p binbook-compress --all-targets -- -D warnings
  cargo check -p binbook-compress --target wasm32-unknown-unknown
  ```

- [x] Commit `feat(binbook-compress): add packbits encoding`.

## Task 4: Build the Deterministic Container Writer

**Files:**
- Create: `crates/binbook-encode/Cargo.toml`
- Create focused modules under `crates/binbook-encode/src/` for model, strings, policies, indices, hashing, and writer
- Create: `crates/binbook-encode/tests/layout.rs`
- Create: `crates/binbook-encode/tests/roundtrip.rs`
- Modify: root `Cargo.toml`

- [ ] Add RED tests for exact section order, 64 KiB page-data alignment, deterministic string-table deduplication, section/page CRCs, policy hashes, font-section digest, page progress ranges, plane offsets, chunk offsets, and bidirectional adjacent transitions.
- [ ] Add a round-trip RED test requiring generated bytes to open and pass `binbook_core::validate_all`.
- [ ] Implement a `BookBuilder` receiving already compiled pages, policies, metadata, navigation, and used-font records. Write through `Write + Seek`; do not accept paths.
- [ ] Compute section/page CRCs and source/font/rendition SHA hashes. Leave optional header/file CRC fields zero. Use `created_unix_time = 0` for reproducible output.
- [ ] Run focused tests, WASM check, Clippy, and `cargo test -p binbook-core`.
- [ ] Commit `feat(binbook-encode): add deterministic container writer`.

## Task 5: Extend Reusable Quantization and X4 Plane APIs

**Files:**
- Modify: `crates/gray2-render/src/lib.rs`
- Create focused modules/tests for quantization and row/image transforms
- Modify: `crates/xteink-x4-display/src/profile.rs` only if a pure geometry helper is missing
- Test: `crates/gray2-render/tests/quantization.rs`, `golden.rs`

- [ ] Add RED tests matching Python GRAY1/GRAY2 thresholds, Floyd-Steinberg propagation, exact four-level values, caller-buffer failures, X4 logical-to-physical corner mapping, staged planes, and 30×1600-byte chunk decomposition.
- [ ] Include an image wider/taller than scratch rows so tests cannot pass through accidental full-buffer assumptions.
- [ ] Implement caller-owned quantization error buffers and reuse `canonical_row_to_staged`; do not create a second X4 coordinate formula.
- [ ] Run host tests, Clippy, and RISC-V no-default-feature checks for both reusable crates.
- [ ] Commit `feat(gray2-render): add compiler quantization primitives`.

## Task 6: Implement Static Image Compilation and Decoding

**Files:**
- Create: `crates/binbook-image/Cargo.toml`
- Create focused decode, resize, orient, compile, and output modules
- Create fixtures/tests under `crates/binbook-image/tests/`

- [ ] Add compact deterministic fixtures covering PNG alpha, JPEG, WebP, SVG, portrait/landscape resize, exact-size input, malformed input, animated/multipage rejection, GRAY1, and staged GRAY2.
- [ ] Add RED assertions for white alpha compositing, aspect-preserving Lanczos contain, centered padding, logical orientation, exact plane/chunk counts, and decoded PNG dimensions.
- [ ] Implement with explicit codec features only. Disable default `image`/`resvg` features that introduce unrequested formats, native parallelism, or system fonts.
- [ ] Add decode tests for NONE, PackBits, and existing host LZ4 paths; reject page indices outside the book.
- [ ] Run package tests, Clippy, WASM check, and Python pixel-parity comparisons. Use exact pixels for non-resized fixtures and RMSE ≤3/255 only for resampling/rasterizer comparisons.
- [ ] Commit `feat(binbook-image): compile and decode static images`.

## Task 7: Implement the EPUB Source Layer

**Files:**
- Create: `crates/binbook-document/Cargo.toml`
- Create focused node, style, resource, navigation, font, and diagnostic modules under `crates/binbook-document/src/`
- Create: `crates/binbook-document/tests/model.rs`
- Create: `crates/binbook-epub/Cargo.toml`
- Create focused package, resources, navigation, html, css, fonts, and diagnostics modules
- Create synthetic EPUB2/EPUB3 fixtures under `crates/binbook-epub/tests/fixtures/`

- [ ] Add RED model tests for typed block/inline nodes, inherited/computed style, resolved resource IDs, navigation anchors, font-face declarations, and deterministic diagnostic ordering. Implement the smallest path-free `binbook-document` model needed to pass before starting EPUB parsing.
- [ ] Run `cargo test -p binbook-document`; expected GREEN before adding `binbook-epub` as a dependent crate.
- [ ] Build synthetic fixtures containing EPUB3 nav, EPUB2 NCX, nested relative paths, fragments, linked/internal/inline CSS, PNG/JPEG/WebP/SVG, TTF/OTF/WOFF/WOFF2, standard obfuscation, malformed optional resources, and an encrypted/DRM marker.
- [ ] Add RED tests for metadata, hashes, linear spine, nav hierarchy, normalized resources, cascade specificity/order/inheritance, `display:none`, font-face resolution, obfuscation, unsupported-feature warning codes, and DRM rejection.
- [ ] Wrap `rbook` behind BinBook-owned types; do not expose dependency types from public APIs. Use `Cursor<&[u8]>`, read-only/default-features-off configuration, and no filesystem calls.
- [ ] Implement the locked CSS subset and emit only `binbook-document` types; `binbook-epub` must not depend on `binbook-render`.
- [ ] Run package tests, Clippy, and `cargo test -p binbook-epub --target wasm32-unknown-unknown --no-run`.
- [ ] Commit `feat(binbook-epub): parse styled epub documents`.

## Task 8: Implement Common Reflow Rendering

**Files:**
- Create: `crates/binbook-render/Cargo.toml`
- Create focused document, style, font, inline, block, table, pagination, raster, and navigation modules
- Create: `crates/binbook-render/tests/reflow.rs`, `fonts.rs`, `navigation.rs`, `golden.rs`

- [ ] Add RED tests for paragraph wrapping, styled spans, headings, lists, blockquotes, preformatted text, bidi, soft hyphens, page breaks, images, equal-column tables, oversized-row degradation, anchor mapping, and missing-glyph fallback.
- [ ] Add font-usage RED tests proving only actually rasterized faces enter `FONT_RESOURCE_INDEX`, with deterministic ordering and digest; test forced-font mode separately.
- [ ] Implement `cosmic-text` with default features off, `std`+rasterization only, supplied font bytes only, and word-or-glyph wrapping.
- [ ] Render at 2x, downsample with Lanczos, then call `binbook-image`/`gray2-render` for quantization and native page construction.
- [ ] Ensure every warning has a stable code, resource/spine context, and deterministic ordering.
- [ ] Run focused tests, Clippy, WASM no-run test, and golden decoded-page comparisons.
- [ ] Commit `feat(binbook-render): add epub reflow pagination`.

## Task 9: Compose the Path-Free Compiler

**Files:**
- Create: `crates/binbook-compiler/Cargo.toml`
- Create modules matching the locked public API
- Create: `crates/binbook-compiler/tests/compile_images.rs`, `compile_epub.rs`, `warnings.rs`

- [ ] Add RED E2E library tests that compile image and EPUB sources into `Cursor<Vec<u8>>`, validate the result through `binbook-core`, decode pages, inspect metadata/navigation/font records, and verify progress/warning events.
- [ ] Test a failing output sink and confirm the error category is preserved.
- [ ] Implement exhaustive dispatch with no paths or CLI types in the compiler crate.
- [ ] Run:

  ```bash
  cargo test -p binbook-compiler
  cargo clippy -p binbook-compiler --all-targets -- -D warnings
  cargo check -p binbook-compiler --target wasm32-unknown-unknown
  cargo test -p binbook-compiler --target wasm32-unknown-unknown --no-run
  cargo tree -p binbook-compiler
  ```

  Inspect the tree and reject OS-font, serial, firmware, Rayon, or native-only dependencies.
- [ ] Commit `feat(binbook-compiler): add path-free compile API`.

## Task 10: Rename and Extend the Rust CLI

**Files:**
- Move: `cli/` -> `crates/binbook/`
- Modify: root `Cargo.toml`
- Split existing oversized `crates/binbook/src/lib.rs` by diagnostic responsibility before adding compiler commands
- Create CLI modules for args, encode, decode, inspect, input discovery, diagnostics, and atomic output
- Create: `crates/binbook/tests/compiler_cli.rs`, `help.rs`

- [ ] Add RED help snapshots/semantic assertions proving the executable is `binbook`, includes examples, exposes `encode/decode/inspect/diag`, and does not expose `encode-png-folder`.
- [ ] Add RED process-level tests for image file, mixed directory, EPUB, explicit format mismatch, unsupported single input, all-skipped directory, strict invalid-book inspection, JSON-only stdout, out-of-range decode, warnings on stderr, and no partial output.
- [ ] Move the crate and preserve every diagnostic command/feature behavior. Rename Rust imports from `binbook_cli` to `binbook`.
- [ ] Implement auto detection by file signature plus path shape; extensions are hints, not sole trust boundaries.
- [ ] Implement atomic sibling-temp output and cleanup on every error.
- [ ] Run default and `serial-device` CLI tests plus all existing protocol/transport tests.
- [ ] Commit `feat(binbook): add multiformat compiler commands`.

## Task 11: Cut Python Back to Support-Only Tools

**Files:**
- Modify: `pyproject.toml`
- Modify: `binbook/cli.py`
- Modify: `binbook/reader.py`, `constants.py`, `structs.py` for font-section viewer compatibility
- Remove compiler-only Python modules only after Rust behavioral replacements pass
- Migrate/remove corresponding Python tests only after mapping each assertion to a named Rust test

- [ ] Add RED tests that `binbook-support --help` exposes only `view` and `kerning-proof`, and that its viewer opens a Rust-generated book containing section 35.
- [ ] Rename the console entrypoint to `binbook-support`; remove Python encode/decode/inspect commands and implementation imports.
- [ ] Teach the transitional Python reader to parse/validate or safely skip valid `FONT_RESOURCE_INDEX` records so the viewer remains usable.
- [ ] Before deleting any Python test/module, create an assertion-migration table in the plan execution notes mapping it to a passing Rust test. Do not delete tests merely to obtain green output.
- [ ] Run full pytest plus the proof test when available:

  ```bash
  uv run pytest -q
  uv run pytest -q tests/test_kerning_proof.py --run-proof
  ```

- [ ] Commit `refactor(python): retain viewer and kerning support only`.

## Task 12: Make Rust the Canonical Navigation-Fixture Compiler

**Files:**
- Modify: `firmware/scripts/build-nav-probe-fixture.py`
- Regenerate: all three `nav_probe.binbook` fixture copies
- Modify: fixture tests under `tests/`, `binbook-core`, `xteink-x4-display`, and `binbook-fw` as required for section 35

- [ ] Add a RED fixture test that checks required section 35, 16 pages, 1,440 chunks, 30 transitions, orientation markers, unique page labels, all four gray levels, and byte-identical fixture copies.
- [ ] Change the Python script to generate source PNGs in a temporary directory and invoke an explicit Rust compiler path. Add `--compiler target/debug/binbook`; do not import the Python writer.
- [ ] Build and regenerate:

  ```bash
  cargo build -p binbook
  UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline \
    python firmware/scripts/build-nav-probe-fixture.py \
    --compiler target/debug/binbook
  ```

- [ ] Run fixture, parser, display, firmware, and full Python tests. Confirm the three fixture hashes match.
- [ ] Commit the script, tests, and exact three fixture paths with `build(fixtures): compile nav probe with rust`.

## Task 13: Update Current Documentation and Roadmap

**Files:**
- Modify: `README.md`, `AGENTS.md`, `BINBOOK_FORMAT_SPEC.md`
- Modify current references: `docs/reference/rust-crate-architecture.md`, `rust-development-standards.md`, `xteink-x4-firmware-flashing.md`, `xteink-x4-agent-device-verification.md`
- Create: `docs/reference/compiler-roadmap.md`
- Modify: `HANDOFF.md`

- [ ] Document `binbook encode` auto/override behavior, supported image types, EPUB subset/degradations, embedded-font policy, warnings, decode/inspect contracts, and `binbook-support` commands.
- [ ] Document the new crate graph, WASM-safe restrictions, build/test commands, and section 35.
- [ ] Roadmap entries must explicitly cover `binbook-wasm`, browser Blob/stream adapters, progress/warning bindings, browser UI, PDF, CBZ, and later source backends. Mark them aspirational, not completion blockers.
- [ ] Update every current `cargo ... -p binbook-cli` or `target/debug/binbook-cli` reference to `binbook`. Leave `docs/historical/` unchanged.
- [ ] Run stale-reference checks and fail on unexpected current hits:

  ```bash
  rg -n 'binbook-cli|encode-png-folder|uv run binbook encode' \
    README.md AGENTS.md Cargo.toml pyproject.toml binbook crates firmware docs/reference
  ```

- [ ] Commit `docs: document rust compiler and wasm roadmap`.

## Task 14: Full Automated Verification

- [ ] Start from a clean Cargo build to prevent stale artifacts masking failures:

  ```bash
  cargo clean
  cargo fmt --all -- --check
  cargo test --workspace
  cargo test -p binbook-document
  cargo test -p binbook-fw --features diagnostic-console
  cargo test -p binbook --features serial-device
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  cargo check -p binbook-core --no-default-features --target riscv32imc-unknown-none-elf
  cargo check -p gray2-render --no-default-features --target riscv32imc-unknown-none-elf
  cargo check -p binbook-compiler --target wasm32-unknown-unknown
  cargo test -p binbook-compiler --target wasm32-unknown-unknown --no-run
  uv run pytest -q
  git diff --check
  ```

- [ ] Build the pinned firmware release exactly:

  ```bash
  cd firmware && \
    RUSTC="$(rustup which --toolchain nightly rustc)" \
    rustup run nightly cargo build -p binbook-fw \
      --features firmware-bin,diagnostic-console \
      --target riscv32imc-unknown-none-elf --release
  ```

- [ ] Drive the native surface end-to-end with an image directory and an EPUB. For each: encode, strict inspect, decode page 0, verify dimensions/pixels/metadata, and save transcripts under `/tmp/binbook-compiler-acceptance/`.
- [ ] Update `HANDOFF.md` with an acceptance matrix before hardware work. Every row must name requirement, implementation path, automated test, serial/query evidence applicability, and webcam evidence applicability.

## Task 15: Mandatory Live Xteink X4 Verification

Hardware commands own `/dev/ttyACM0` exclusively and must run sequentially. Do not parallelize flash, serial, diagnostic, or webcam work. A failed flash/serial/camera command is evidence to record, not a reason to skip the gate.

- [ ] Record the starting fixture SHA and build binary SHA in `HANDOFF.md`.
- [ ] Flash the Rust-compiled canonical fixture:

  ```bash
  FW_FEATURES="firmware-bin,diagnostic-console" \
    firmware/scripts/flash-xteink-x4-nav-probe.sh
  ```

  Record chip, flash size, application size, final flash result, and USB re-enumeration.

- [ ] Capture at least 15 seconds of boot serial using the exact pyserial command from `AGENTS.md`. Record the complete relevant output; do not substitute `espflash monitor`.
- [ ] Establish independent baselines, one command at a time:

  ```bash
  cargo run -p binbook --features serial-device -- diag hello --port /dev/ttyACM0
  cargo run -p binbook --features serial-device -- diag status --port /dev/ttyACM0
  ```

  Require protocol 1, max frame 512, firmware `binbook-fw`, target `xteink-x4`, `page_count=16`, zero protocol errors, and `last_error=0`.

- [ ] Use a discriminating non-default state and independently confirm it:

  ```bash
  cargo run -p binbook --features serial-device -- diag page --port /dev/ttyACM0 goto 3
  cargo run -p binbook --features serial-device -- diag status --port /dev/ttyACM0
  cargo run -p binbook --features serial-device -- diag page --port /dev/ttyACM0 goto 0
  cargo run -p binbook --features serial-device -- diag status --port /dev/ttyACM0
  cargo run -p binbook --features serial-device -- diag logs --port /dev/ttyACM0 --since 0
  ```

  Verify response opcode/sequence/status/payload, `3 -> 0` state change, render success events, no display error, and follow-up STATUS confirmation.

- [ ] Capture a fresh native-resolution webcam image:

  ```bash
  ffmpeg -hide_banner -loglevel error -f video4linux2 \
    -i /dev/video1 -frames:v 1 /tmp/binbook-rust-compiler-webcam.jpg
  ffmpeg -hide_banner -loglevel error \
    -i /tmp/binbook-rust-compiler-webcam.jpg \
    -vf 'crop=440:770:770:250' -frames:v 1 \
    /tmp/binbook-rust-compiler-panel.jpg
  ```

- [ ] Inspect both actual files, show the user both paths, and record what is visibly present. Require correct TL/TR/BL/BR markers and unique shapes, TOP/RIGHT/BOTTOM/LEFT labels, center crosshair/rulers, asymmetric top-left triangle, page number/orientation, unclipped border, no stale regions, and distinct black/dark-gray/light-gray/white swatches. The bezel is not rendered content.
- [ ] If visible output fails, do not claim completion. Record the exact first divergence, serial/log evidence, capture paths, and leave the acceptance row failed.
- [ ] Replace `HANDOFF.md` with the current final state, including exact commands/output, starting/ending state, hashes, serial transcript, webcam paths, observed panel result, known failures, and the completed acceptance matrix.

## Task 16: Final Adversarial Completion Audit

- [ ] Re-read this plan and map every Must Ship requirement to implementation, automated test, and observed evidence.
- [ ] Attempt to disprove completion by:
  - corrupting section 35 and verifying strict inspect fails;
  - reverting one PackBits branch in a local patch and confirming its test fails, then restoring the patch;
  - running EPUB compilation with an unsupported CSS feature and confirming a stable warning plus usable output;
  - compiling from a directory containing one valid and one unsupported file and confirming one page plus warning;
  - running `goto 0` from page 3 and checking STATUS/log rather than trusting acknowledgement;
  - verifying the webcam file timestamp and source are from the current `/dev/video1` run.
- [ ] Run `git status --short`, inspect the full diff, and verify no unrelated user changes, generated caches, temporary assets, or historical-doc edits are included.
- [ ] Do not write “complete”, “passed”, or “all commands work” while any acceptance cell is missing, hardware is unobserved, a response is placeholder-only, or source contradicts the claim.

## Completion Criteria

- All tasks are checked and every intermediate focused test was green before the next task began.
- Rust `binbook` compiles image and EPUB inputs, strictly inspects and decodes its output, and preserves all diagnostic commands.
- Python exposes only `binbook-support view` and `binbook-support kerning-proof`.
- Required font-resource metadata is specified, encoded, parsed, validated, and visible to the transitional viewer.
- Compiler crates pass native, WASM compile/no-run, Clippy, and workspace gates without forbidden dependencies.
- Canonical fixture bytes were produced through Rust and all fixture copies match.
- The exact firmware was flashed; serial, HELLO, STATUS, state change, and logs were independently verified.
- A fresh `/dev/video1` capture was inspected and proves orientation, page identity, clipping, stale-region behavior, and four grayscale levels.
- `HANDOFF.md` contains current evidence and a complete acceptance matrix.
