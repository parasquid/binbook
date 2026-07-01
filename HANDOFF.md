# Handoff: Rust Multi-Format Compiler

Date: 2026-07-01
Active plan: `docs/plans/2026-07-01-rust-multiformat-compiler.md`
Current task: Task 11 — Python support-only cutover

## Completed

Tasks 1 through 10 are complete.

- Added required `FONT_RESOURCE_INDEX` section ID 35 and its 80-byte record contract to `BINBOOK_FORMAT_SPEC.md`.
- Added no-allocation Rust parsing with typed source/style enums and validation of indices, flags, reserved bytes, and string references.
- Added Python record packing/unpacking plus an empty required writer section so transitional Python fixtures and viewer remain compatible.
- Regenerated all three canonical `nav_probe.binbook` copies; they are byte-identical.
- Updated the exact section-table scratch requirement from 720 to 760 bytes.
- Added allocation-free typed wire encoders for header, section, page, navigation, chapter, chunk, transition, and font-resource records.
- Added visitor-based strict validation with stable `ValidationCode` categories for bounds, ordering, reserved bytes, CRCs, required features, profiles, strings, planes, chunks, transitions, navigation/chapter links, and fonts.
- Shared all record-size constants between parsers and encoders.
- Fixed the transitional Python writer to align every plane blob to four bytes and keep page/chunk indices consistent with padding.
- Regenerated all canonical `nav_probe.binbook` copies with aligned plane offsets.
- Added `binbook-compress`, with a no-std caller-buffer PackBits encoder and an alloc-gated `Vec` convenience API.
- Matched the transitional Python encoder's deterministic run selection, including 127/128 boundaries and split-run behavior.
- Required all PackBits test vectors, including a 9,217-byte mixed-pattern input, to decode through `binbook-decompress`.
- Added path-free `binbook-encode` with a `BookBuilder` that targets any `Write + Seek` sink.
- Added typed page/plane/chunk, metadata, source, navigation, font-policy, and used-font models.
- Emit all 19 required sections in canonical order with 64 KiB page-data alignment, deterministic string deduplication, section/page CRCs, policy/font/rendition SHA-256 hashes, progress ranges, aligned planes, chunk indices, and bidirectional adjacent transitions.
- Source and decoded-font constructors compute their SHA-256 digests directly from caller-provided bytes. Reproducible timestamp and optional header/file CRC fields remain zero.
- Added exact GRAY1/GRAY2 threshold quantization, caller-buffer Floyd-Steinberg row state, and MSB-first GRAY1/GRAY2 row packing to `gray2-render`.
- Added allocation-free full-image staged-plane conversion that reuses `canonical_row_to_staged`, plus borrowed plane chunk iteration.
- Added X4 logical GRAY2 packing through the existing `logical_to_physical` mapping, avoiding a second coordinate formula.
- Added path-free PNG, JPEG, WebP, and SVG decoding with explicit codec features and no Rayon, system fonts, or OS font discovery.
- Added APNG/animation rejection, white alpha flattening before Lanczos resampling, centered contain/padding, exact X4 orientation, GRAY1/GRAY2 compilation, and 30-chunk PackBits planes.
- Added BinBook page decoding for NONE, PackBits, and host LZ4 plus PNG output and typed out-of-range rejection.
- Added the path-free `binbook-document` model with typed block/inline nodes, computed styles, normalized resource IDs, navigation, fonts, and deterministically sorted diagnostics.
- Added `binbook-epub` with EPUB2/EPUB3 metadata, linear spine, EPUB3 nav/EPUB2 NCX, nested resource resolution, HTML conversion, the locked CSS subset, `display:none`, embedded font-face resolution, IDPF/Adobe deobfuscation, WOFF/WOFF2 decoding, stable degradation diagnostics, and DRM rejection.
- Kept all public EPUB APIs dependency-free and filesystem-free. `rbook` 0.7.9 requires an owned `'static` reader, so parsing copies the input into `Cursor<Vec<u8>>`; this is the only deviation from the plan's requested borrowed cursor.
- Added `binbook-render` using `cosmic-text` 0.19 without default/system-font features, supplied font bytes only, styled rich-text shaping, word-or-glyph wrapping, deterministic pagination, page-break and anchor mapping, structural block rendering, equal-width table rows, and oversized-row degradation.
- Rasterization occurs at 960×1600, then `binbook-image` downsamples with Lanczos and routes through the established GRAY2 quantization/X4 native-plane compiler.
- Used-font records include only selected raster faces in deterministic order; forced-font mode is separate, and missing glyphs plus source diagnostics become stable context-bearing warnings.
- Added the path-free `binbook-compiler` API with locked source/options/event/summary types, exhaustive image-sequence/EPUB dispatch, typed failure categories, built-in font selection, strict in-memory validation before output, and no path or CLI ownership.
- Image and EPUB compilation now compose decode/parse, layout, 2× raster, compression, assembly, validation, metadata/navigation/font records, warning callbacks, and phase progress into a caller-owned `Write + Seek` sink.
- Moved `cli/` to `crates/binbook/`, renamed the package/library/executable to `binbook`, and preserved all diagnostic commands, protocol builders, serial transport, staged-gray exercise, and navigation-burst behavior.
- Added native `encode`, `decode`, and strict/JSON `inspect` commands with signature-based input detection, lexical non-recursive image-directory discovery, warning-and-skip behavior, locked profile/pixel/font options, logical PNG decoding, and atomic sibling-temp writes that preserve existing output and clean up every failure path.
- Split the former 1,157-line CLI library into responsibility-focused modules; every Rust source module is now below 250 logical lines.

## TDD evidence

RED:

- `cargo test -p binbook-core --test font_resources` failed because `FontResourceIndexEntry`, its enums/size, and `InvalidFontResource` did not exist.
- `uv run pytest -q tests/test_font_resources.py` failed during import because the Python record type and size did not exist.

GREEN:

- `cargo test -p binbook-core`: passes, including 3 font-resource tests and the missing-section-35 test.
- `cargo test --workspace`: passes after fixture regeneration.
- `uv run pytest -q`: 99 passed, 26 skipped.
- `cargo fmt --all -- --check`: passes.
- Focused fixture/validation matrix: 28 passed.

Task 2 RED:

- `cargo test -p binbook-core --test encoding` failed on missing encoder types and constants.
- `cargo test -p binbook-core --test strict_validation` failed on missing validator API and typed validation codes.
- The first strict-valid fixture check exposed unaligned plane offsets (`24080`, `28539`); this was corrected in the writer and fixtures rather than weakening the validator.

Task 2 GREEN:

- `cargo test -p binbook-core --test encoding`: 4 passed.
- `cargo test -p binbook-core --test strict_validation`: 4 passed.
- `cargo test -p binbook-core`: all tests passed.
- `cargo clippy -p binbook-core --all-targets -- -D warnings`: passed.
- RISC-V no-std check passed using the rustup compiler explicitly: `RUSTC="$(rustup which --toolchain stable rustc)" rustup run stable cargo check -p binbook-core --no-default-features --target riscv32imc-unknown-none-elf`.
- `cargo test --workspace`: passed.
- `cargo test -p binbook-fw --features diagnostic-console`: passed.
- `uv run pytest -q`: 100 passed, 26 skipped.

Task 3 RED:

- `cargo test -p binbook-compress --test packbits` failed because the new crate had no library or encoding API.

Task 3 GREEN:

- `cargo test -p binbook-compress`: 5 PackBits tests passed.
- `cargo clippy -p binbook-compress --all-targets -- -D warnings`: passed.
- Default and no-default-feature WASM checks passed using the rustup compiler explicitly.

Task 4 RED:

- `cargo test -p binbook-encode --test layout --test roundtrip` failed because `BookBuilder` and the writer model did not exist.

Task 4 GREEN:

- `cargo test -p binbook-encode`: deterministic layout and strict round-trip tests passed.
- `cargo clippy -p binbook-encode --all-targets -- -D warnings`: passed.
- `cargo check -p binbook-encode --target wasm32-unknown-unknown`: passed using the rustup compiler explicitly.
- `cargo test -p binbook-core`: passed.
- `cargo test --workspace`: passed.

Task 5 RED:

- Focused `gray2-render` and X4 validation tests failed on missing quantization, packing, image-plane, chunk, and logical-orientation APIs.

Task 5 GREEN:

- `cargo test -p gray2-render`: passed, including Python-matched GRAY1/GRAY2 diffusion and a 257×5 row-streaming case.
- `cargo test -p xteink-x4-display`: passed, including all four logical-corner mappings.
- `cargo clippy -p gray2-render -p xteink-x4-display --all-targets -- -D warnings`: passed.
- Both crates passed no-default-feature RISC-V checks using the rustup compiler explicitly.

Task 6 RED:

- Focused image tests failed because `binbook-image`, codec decoding, fitting, compilation, and book-page decoding APIs did not exist.

Task 6 GREEN:

- `cargo test -p binbook-image`: 8 tests passed across decode, fit, compile, orientation, compression, and PNG output.
- `cargo clippy -p binbook-image --all-targets -- -D warnings`: passed.
- `cargo check -p binbook-image --target wasm32-unknown-unknown`: passed using the rustup compiler explicitly.
- Dependency-tree scan found no Rayon, fontdb, fontconfig, or system-font dependencies.
- Python/Pillow regenerated the 7×5 Lanczos reference exactly; Rust test RMSE stays ≤3 and exact-size orientation pixels match exactly.
- `cargo test --workspace`: passed.

Task 7 RED:

- `cargo test -p binbook-document --test model` initially failed because the document crate and typed model APIs did not exist.
- `cargo test -p binbook-epub --test epub` initially failed because EPUB parsing, owned output types, and fixtures did not exist.

Task 7 GREEN:

- `cargo test -p binbook-document`: 2 model tests passed.
- `cargo test -p binbook-epub`: 2 synthetic EPUB integration tests passed, covering EPUB2/3 navigation, resource types, CSS cascade/inline precedence, missing-resource degradation, standard font obfuscation, and DRM rejection.
- `cargo clippy -p binbook-document -p binbook-epub --all-targets -- -D warnings`: passed.
- `RUSTC="$(rustup which --toolchain stable rustc)" rustup run stable cargo test -p binbook-epub --target wasm32-unknown-unknown --no-run`: passed and produced all WASM test executables.

Task 8 RED:

- `cargo test -p binbook-render --tests` initially failed because the renderer crate and its API did not exist.
- The first deterministic reflow fixture took more than 60 seconds because it rasterized too many 2× pages twice; the fixture was reduced while preserving wrap, forced-pagination, and repeatability coverage.
- The decoded-page golden test initially failed against a zero placeholder and reported the real stable GRAY2 digest before that digest was locked into the assertion.

Task 8 GREEN:

- `cargo test -p binbook-render`: 5 focused tests passed for reflow/structure, fonts/fallback, navigation, warnings/tables, and decoded-page golden output.
- Golden decoded packed-page SHA-256: `88a74bd02c30c66093bc0a8420f714dbc5f9c0916475879b03a75a48cd96f825`.
- `cargo clippy -p binbook-render -p binbook-image --all-targets -- -D warnings`: passed.
- `RUSTC="$(rustup which --toolchain stable rustc)" rustup run stable cargo test -p binbook-render --target wasm32-unknown-unknown --no-run`: passed and produced all WASM test executables.
- Dependency feature scan found no `fontconfig` or Rayon features.

Task 9 RED:

- `cargo test -p binbook-compiler --tests` initially failed because the compiler crate and locked public API did not exist.
- The first image E2E fixture failed with `CompileError::Image` because its hand-written PNG was malformed; the test now uses a valid in-memory SVG fixture and retains end-to-end image-source coverage.

Task 9 GREEN:

- `cargo test -p binbook-compiler`: 4 E2E tests passed for image compilation, EPUB compilation, warnings/progress, empty input, and injected output failure.
- Both outputs pass `binbook-core` strict validation and decode through `binbook-image`; EPUB output exposes the expected title and navigation count.
- `cargo clippy -p binbook-compiler --all-targets -- -D warnings`: passed.
- Stable-rustc WASM `cargo check` and `cargo test --no-run` passed for the crate and all integration tests.
- `cargo tree -p binbook-compiler` contains no fontconfig, Rayon, serial, firmware, ESP HAL, or Embassy dependencies.

Task 10 RED:

- New help/process tests initially failed because the renamed executable did not expose `encode`, `decode`, or `inspect`.
- Serial-feature Clippy exposed pre-existing oversized diagnostic functions after the library split; argument groups were converted to typed context structs and an unnecessary explicit lifetime was removed.

Task 10 GREEN:

- `cargo test -p binbook`: all compiler CLI, help, and 35 protocol tests passed.
- `cargo test -p binbook --features serial-device`: all tests passed; 10 hardware-orchestration tests and 11 serial-transport tests passed, with 4 live-device tests still intentionally ignored until Task 15.
- `cargo clippy -p binbook --all-targets --features serial-device -- -D warnings`: passed.
- Process tests cover image file, mixed directory skip warnings, EPUB, explicit mismatch, unsupported/all-skipped input, strict invalid inspection, JSON-only stdout, out-of-range decode, atomic cleanup, and preservation of an existing destination after an in-compiler failure.
- Manual native round trip passed: encoded `two-color.svg`, strict-inspected one valid page, and decoded `/tmp/binbook-task10.png` as an 800×480 grayscale PNG.

## Fixture evidence

Baseline fixture SHA-256 before Task 1:

`a8c2c7d935ce6ec6376139153e91a54111a59440dd85b62270fd072d8e47766d`

Current SHA-256 for all three copies:

`96fdfa2d8d9583e91c2f868c00c0c5863788e500dc264f77c73cbe5cd404f135`

The fixture remains 16 pages, 1,440 chunks, and 30 transitions. The latest hash includes the required empty section-35 entry and four-byte plane padding.

## Files changed through Task 2

- `BINBOOK_FORMAT_SPEC.md`
- `binbook/constants.py`, `reader.py`, `structs.py`, `writer.py`
- `crates/binbook-core/src/{error,font_resource,lib,section}.rs`
- `crates/binbook-core/tests/{font_resources,open}.rs`
- `tests/test_font_resources.py`
- Three canonical `nav_probe.binbook` fixture copies
- Active plan and this handoff
- `binbook/writer.py`, `tests/test_validation.py`
- `crates/binbook-core/src/{encode,index_encode,link_validation,record_validation,validate,validation_crc}.rs`
- Shared parser modules in `crates/binbook-core/src/`
- `crates/binbook-core/tests/{encoding,strict_validation}.rs`
- `crates/binbook-compress/{Cargo.toml,src/lib.rs,src/packbits.rs,tests/packbits.rs}`
- Root workspace manifest and lockfile
- `crates/binbook-encode/` model, policies, hashing, strings, indices, writer, and tests
- `crates/gray2-render/src/{quantize,pack,image}.rs` and focused tests
- `crates/xteink-x4-display/src/profile.rs`, reusing the established X4 mapping
- `crates/binbook-image/` codec, fit, compile, book decode, SVG fixture, and focused tests
- `crates/binbook-document/` typed document, node, style, resource, navigation/font, diagnostic model, and tests
- `crates/binbook-epub/` package parser, HTML/CSS/font handling, synthetic EPUB2/3 fixtures, and integration tests
- `crates/binbook-render/` document pagination, supplied-font loading, rich-text shaping, 2× rasterization, warnings, navigation mapping, and focused/golden tests
- `crates/binbook-image/src/{lib,compile}.rs` path-free decoded-image compilation entry point used by the renderer
- `crates/binbook-compiler/` locked public API, dispatch/composition, validation, bundled-font policy, E2E fixtures, callback tests, and failing-sink test
- `crates/binbook/` renamed native CLI, focused argument/input/encode/decode/inspect/atomic-output modules, preserved diagnostic modules, and process/help tests
- `AGENTS.md` and `firmware/scripts/run-x4-nav-burst-diagnostic.py` now invoke the renamed package/path

## Next exact action

Start Task 11 with RED Python help/viewer tests, then rename the Python entrypoint to `binbook-support`, remove compiler/decode/inspect commands and imports, and map every removed Python assertion to an existing named Rust test before deleting compiler-only modules.

## Hardware state

No hardware commands have run for this plan yet. Task 15 remains a mandatory completion gate: flash the Rust-generated fixture, capture at least 15 seconds of serial, independently query HELLO/STATUS/logs from a non-default page state, and inspect a fresh `/dev/video1` native capture plus the confirmed panel crop.
