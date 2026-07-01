# Handoff: Rust Multi-Format Compiler

Date: 2026-07-01
Active plan: `docs/plans/2026-07-01-rust-multiformat-compiler.md`
Current task: None — implementation plan complete

## Completed

Tasks 1 through 16 are complete.

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
- Renamed the Python console entrypoint to `binbook-support` and reduced its command surface to `view` and `kerning-proof`; Python no longer exposes encode, decode, or inspect commands.
- Removed Python EPUB/package/flow, compiler, writer, inspector, and string-builder modules after mapping their deleted test assertions to named Rust replacements in the active plan.
- Extended the transitional Python reader to parse and validate section-35 font records, including contiguous indices, reserved fields, SHA-256 size, and font family/source string references.
- Replaced the navigation-fixture Python compiler path with source-only PNG generation and an explicit `--compiler target/debug/binbook` invocation; the script copies one Rust-produced byte stream to all three fixture consumers.
- Regenerated the canonical 16-page fixture through Rust with required empty section 35, 1,440 chunks, 30 transitions, persistent orientation frames, unique labels/patterns, and all four grayscale levels.
- Rewrote current user/contributor documentation around the Rust `binbook` compiler and `binbook-support`, including auto/override input behavior, supported sources, EPUB degradation/font policy, warnings, atomic output, decode/inspect contracts, crate ownership, WASM restrictions, and section 35.
- Added an explicitly aspirational compiler roadmap for `binbook-wasm`, browser Blob/stream adapters, progress/warning bindings, browser UI, PDF, CBZ, and later source backends.
- Updated every current reference/runbook command from `binbook-cli` to `binbook` without modifying historical documentation.

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

Task 11 RED:

- `tests/test_support_cli.py` initially failed because `pyproject.toml` still registered `binbook` and Python help still exposed compiler commands.
- The Rust-generated viewer test initially exposed that `BinBookReader` had no parsed font-resource collection and rejected nonempty section 35.

Task 11 GREEN:

- `uv run binbook-support --help` exposes exactly `view` and `kerning-proof`.
- `tests/test_support_cli.py` compiles an EPUB through Rust, opens its nonempty section 35 with the Python reader, and renders page 0 through the transitional viewer at 480×800.
- `uv run pytest -q`: 58 passed, 26 skipped.
- `uv run pytest -q tests/test_kerning_proof.py --run-proof`: 26 passed.
- Source scan found no remaining Python imports or CLI parsers for removed encoder/decoder/inspector modules.

Task 12 RED:

- The new builder-contract test failed because `build-nav-probe-fixture.py` still imported the deleted Python page compiler and writer and had no `--compiler` argument.
- The first Rust fixture parser run exposed one stale Python-era expectation that optional book title metadata was `nav-probe`; the path-free image compiler correctly emits an empty optional title, and the fixture test now asserts that canonical value.

Task 12 GREEN:

- Exact regeneration command passed: `cargo build -p binbook` followed by `UV_CACHE_DIR=/tmp/binbook-uv-cache uv run --offline python firmware/scripts/build-nav-probe-fixture.py --compiler target/debug/binbook`.
- `tests/test_nav_probe_fixture.py`: 10 passed, including section 35, page/chunk/transition counts, copy identity, orientation frame, labels/patterns, four grayscale levels, and transition masks.
- `cargo test -p binbook-core`, `cargo test -p xteink-x4-display`, and `cargo test -p binbook-fw --features diagnostic-console`: passed.
- `uv run pytest -q`: 60 passed, 26 skipped.
- All three fixture SHA-256 values are `3c87fbde1e05c1bc127083511a4353b3d400c292df92672dc6710e9bc2f7f31d`.

Task 13 GREEN:

- The mandated stale-reference scan reports no current hits for `binbook-cli`, the removed PNG-folder alias, or the old Python encode invocation.
- `cargo test -p binbook --test help`: 2 passed after documentation/help alignment.
- `cargo fmt --all -- --check` and `git diff --check`: passed after applying the one pending fixture-test formatting normalization.
- `README.md`, `AGENTS.md`, `BINBOOK_FORMAT_SPEC.md`, all four specified current references, and `docs/reference/compiler-roadmap.md` describe current behavior and commands.

Task 14 GREEN:

- Started with `cargo clean` (8.2 GiB and 28,495 files removed), then the complete formatting, workspace, focused, feature-gated, Clippy, RISC-V, WASM, Python, and diff gate passed. Current Python result: 60 passed, 26 skipped. Serial-device result: 10 hardware-orchestration tests passed, 11 transport tests passed, and 4 live-device tests remain intentionally ignored pending Task 15.
- The first all-features workspace Clippy run exposed host compilation of ESP-only optional dependencies. `binbook-fw` now places ESP/Embassy dependencies behind `cfg(target_arch = "riscv32")`, gates board firmware code to RISC-V, and supplies a host stub binary. The rerun passed with `-D warnings`.
- The exact pinned nightly firmware release build with `firmware-bin,diagnostic-console` passed for `riscv32imc-unknown-none-elf`.
- Native image-directory acceptance produced 2 valid pages. EPUB acceptance produced 1 valid page, 1 chapter, 1 navigation entry, title `Rust Compiler Acceptance`, author `BinBook QA`, and language `en`.
- Both decoded pages are 800×480 grayscale PNGs with exactly the canonical values 0, 85, 170, and 255. Full transcripts are `/tmp/binbook-compiler-acceptance/images-transcript.txt`, `/tmp/binbook-compiler-acceptance/epub-transcript.txt`, and `/tmp/binbook-compiler-acceptance/pixel-verification.txt`.
- `inspect --json` now exposes title, author, and language; the EPUB CLI integration test locks the title result.

Task 15 HARDWARE GREEN:

- Starting inputs: canonical fixture SHA-256 `3c87fbde1e05c1bc127083511a4353b3d400c292df92672dc6710e9bc2f7f31d`; release firmware SHA-256 `ff52cab7e1312f9db4ecbdd6a917898ac259486ba612f638cbeb61dff1080d6a`.
- Flash command: `FW_FEATURES="firmware-bin,diagnostic-console" firmware/scripts/flash-xteink-x4-nav-probe.sh`. Observed ESP32-C3 revision v0.4, 16 MB flash, application 1,122,768/16,384,000 bytes (6.85%), and `Flashing has completed!` on `/dev/ttyACM0`. Full output: `/tmp/binbook-compiler-acceptance/flash-transcript.txt`.
- The exact `AGENTS.md` pyserial reset/read command captured 15 seconds. It recorded ESP-IDF boot, 16 MB DIO flash, all application segments, `Loaded app from partition at offset 0x10000`, and no boot error. Binary diagnostic mode intentionally emits no textual application log. Full output: `/tmp/binbook-compiler-acceptance/boot-serial.txt`.
- `diag hello` returned `protocol=1 max_frame=512 capabilities=KEY,PAGE,STATUS,LOG,CRASH,DISPLAY_PROBE firmware=binbook-fw target=xteink-x4`.
- Initial STATUS returned `current_page=0 page_count=16 panel_mode=Grayscale dropped_log_count=0 protocol_error_count=0 last_error=0`.
- `diag page ... goto 3` returned `current_page=3`; an independent STATUS confirmed page 3 with 16 pages, Grayscale mode, zero drops/protocol errors, and `last_error=0`.
- `diag page ... goto 0` returned `current_page=0`; an independent STATUS confirmed the same clean state at page 0.
- Logs independently recorded `TURN_STARTED`, `PAGE_TURN arg0=0 arg1=3`, grayscale overlay activation/completion and base-sync completion for page 3, followed by `PAGE_TURN arg0=3 arg1=0` and the same complete render sequence for page 0. Cursor ended at 68 with zero dropped records. Full query outputs: `/tmp/binbook-compiler-acceptance/diag-*.txt`.
- Fresh `/dev/video1` capture: `/tmp/binbook-rust-compiler-webcam.jpg`, 1920×1080, 2026-07-01 17:48:17.383334134 +0800. Prescribed crop: `/tmp/binbook-rust-compiler-panel.jpg`, 440×770, 2026-07-01 17:48:17.425441832 +0800.
- Both files were inspected at original detail. The panel visibly shows PAGE 00 in portrait; TL triangle, TR circle, BL square, BR diamond; TOP/RIGHT/BOTTOM/LEFT labels; center crosshair and rulers; edge ticks; the asymmetric top-left triangle; complete unclipped border; and distinct black, dark-gray, light-gray, and white swatches. No stale page-3 region or unintended blank region is visible. The bezel was excluded from content assessment.
- Known hardware failures: none observed. The first `logs --since 0` response was page-limited at cursor 20; sequential cursor 20, 40, and 60 queries retrieved the complete navigation evidence rather than treating the first response as absence of events.

Task 16 ADVERSARIAL GREEN:

- Corrupted the section-35 `entry_size` byte from 80 to 79 in a `/tmp` fixture copy. `binbook inspect --validate --strict --json` rejected it with exit 1 and `error: invalid BinBook`.
- Temporarily changed the PackBits two-byte repeat threshold from 2 to 3. The exact golden boundary test failed (`left: 3`, `right: 2`, exit 101). The source was restored, all five PackBits tests passed, and `git diff --exit-code` proved no mutation remained.
- Added unsupported `float: left` CSS to a `/tmp` EPUB. Compilation emitted stable `warning[UnsupportedContent]`, strict inspection reported a valid one-page book with intact metadata/navigation, and page 0 decoded to an 800×480 grayscale PNG.
- Compiled a directory containing one SVG and one `.txt` file. The CLI emitted `warning: skipping unsupported input unsupported.txt`; strict inspection reported exactly one valid page.
- Repeated the discriminating page 3→0 sequence. Immediate STATUS correctly exposed the intermediate `panel_mode=Bw`, so acknowledgement was not treated as completion. A settled STATUS independently reported page 0, Grayscale, 16 pages, zero drops/protocol errors, and `last_error=0`; logs 85–97 proved overlay and base-sync completion.
- Webcam provenance recheck recorded the current file birth/mtime, 1920×1080 source and 440×770 crop, and `/dev/video1` as the USB UVC Insta360 One RS capture device. Evidence: `/tmp/binbook-compiler-acceptance/adversarial-webcam-provenance.txt`.
- Final pre-documentation `git status --short` and diff were empty. No historical docs changed. Python/pytest cache directories are ignored runtime artifacts and are not included in Git.
- Final verification after restoring all mutations: formatting check passed; all 5 PackBits tests passed; `binbook` serial-feature tests passed (61 automated, 4 intentionally ignored live-device tests); Python reported 60 passed and 26 skipped; all-features workspace Clippy passed with warnings denied; current-reference stale-alias scan and `git diff --check` passed.

## Acceptance matrix before hardware

| Requirement | Implementation path | Automated test/evidence | Serial/query applicability | Webcam applicability |
|---|---|---|---|---|
| BinBook 0.1 font records and strict validation | `binbook-core`, `binbook-encode` | workspace tests; strict validation; section-35 tests | Fixture parse/page count must remain valid on device | Not applicable |
| Deterministic PackBits compatible with firmware | `binbook-compress`, `binbook-decompress` | five PackBits tests including 9,217-byte input | Render/log success exercises device decode | Visible corruption or missing regions would fail |
| Image sequence compilation | `binbook-image`, `binbook-compiler`, `binbook encode` | 2-page native E2E transcript; strict inspect; decoded pixels | Device uses the Rust-compiled canonical image fixture | Page image/orientation is required evidence |
| EPUB parsing, reflow, metadata, navigation, and fonts | `binbook-epub`, `binbook-render`, `binbook-compiler` | compiler/render tests; EPUB E2E metadata and pixel transcript | Not applicable to canonical hardware fixture | Not applicable |
| Native inspect/decode and atomic output | `crates/binbook/src/{inspect,decode,atomic_output}.rs` | CLI process tests and both E2E transcripts | Diagnostic commands are separately verified below | Not applicable |
| Portable compiler and reusable firmware crates | crate feature/target boundaries and target-gated `binbook-fw` dependencies | all-features Clippy; RISC-V checks; WASM check/no-run; pinned release build | Firmware identity/protocol query required | Not applicable |
| Rust-generated canonical fixture | `firmware/scripts/build-nav-probe-fixture.py --compiler target/debug/binbook` | fixture tests; SHA-256 `3c87fbde1e05c1bc127083511a4353b3d400c292df92672dc6710e9bc2f7f31d` | Must report 16 pages and render page transitions | Must show orientation frame and four grays |
| Live diagnostic protocol and state transition | `binbook-fw` diagnostic console and `binbook diag` | host protocol/orchestration/transport tests | Observed protocol 1, 512-byte frame, correct identity, clean STATUS, page 3→0, complete logs | PAGE 00 independently visible after transition |
| Physical X4 output correctness | `xteink-x4-display`, `ssd1677-driver`, fixture orientation frame | host rendering/driver tests | Observed complete overlays/base sync for pages 3 and 0; no errors/drops | Observed all labels/shapes/rulers, unclipped border, no stale regions, and four grays |

## Final Must Ship matrix

| Must Ship requirement | Implementation path | Automated test | Observed evidence |
|---|---|---|---|
| Rust package/library/executable named `binbook` | `crates/binbook`, workspace manifest | help and CLI process tests | Native E2E and hardware diagnostics invoked `target/debug/binbook` |
| Stable encode/decode/inspect/diag commands | `crates/binbook/src/args.rs` and command modules | help, protocol, compiler CLI tests | Image/EPUB E2E plus live HELLO/STATUS/page/log commands |
| Static PNG/JPEG/WebP/SVG and non-recursive image directory | `binbook-image`, native input discovery | codec, directory, CLI tests | 2-page SVG directory and mixed-directory acceptance |
| EPUB 2/3 metadata, spine, nav/NCX, reflow, fonts, images, warnings | `binbook-epub`, `binbook-document`, `binbook-render` | EPUB2/3 and render/font/navigation/golden tests | Metadata/nav E2E and unsupported-CSS degradation audit |
| GRAY1 and staged GRAY2 output | `gray2-render`, `binbook-image` | quantization, plane, orientation, compiler tests | Decodes contain exact 0/85/170/255 levels; X4 visibly shows four swatches |
| Strict Rust validation and logical PNG decode | `binbook-core`, native inspect/decode | strict validation and CLI error tests | Both E2E books strict-valid; corrupted section 35 rejected; 800×480 decoded pages |
| Required `FONT_RESOURCE_INDEX` migration | spec, `binbook-core`, `binbook-encode`, Python support reader | Rust/Python section-35 and fixture tests | Canonical fixture strict-valid; corrupt entry size rejected |
| Compiler crates support WASM target | path-free compiler crate graph | WASM check and test no-run | Task 14 commands produced all compiler test WASM executables |
| Rust canonical fixture proven on X4 | Rust fixture builder, firmware release, diagnostic/display crates | fixture, firmware, protocol/display tests | Exact hash flashed; 16 pages queried; page 3→0 logs; fresh webcam proof |

## Fixture evidence

Baseline fixture SHA-256 before Task 1:

`a8c2c7d935ce6ec6376139153e91a54111a59440dd85b62270fd072d8e47766d`

Current Rust-generated SHA-256 for all three copies:

`3c87fbde1e05c1bc127083511a4353b3d400c292df92672dc6710e9bc2f7f31d`

The fixture remains 16 pages, 1,440 chunks, and 30 transitions. The latest hash is compiled by Rust and includes the required empty section-35 entry and four-byte plane padding.

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
- `pyproject.toml`, `binbook/cli.py`, `reader.py`, and `sections.py` now define the support-only Python surface and section-35 viewer compatibility
- `tests/test_support_cli.py` and the active-plan migration table preserve the cutover evidence; obsolete compiler-only Python modules/tests are removed
- `firmware/scripts/build-nav-probe-fixture.py`, the three canonical fixture copies, and their parser/display/firmware/Python tests now use the Rust compiler as source of truth
- `README.md`, `AGENTS.md`, `BINBOOK_FORMAT_SPEC.md`, current Rust/firmware references, and `docs/reference/compiler-roadmap.md` now document the shipped compiler and aspirational backends

## Next exact action

No implementation task remains. Preserve `/tmp/binbook-compiler-acceptance/` and the two webcam JPEGs while their local evidence is useful; they are intentionally not tracked.

## Hardware state

Hardware verification is complete with the exact commands and observed evidence above. Device ended on page 0 in Grayscale mode with 16 pages, zero dropped logs, zero protocol errors, and `last_error=0`. The current webcam files prove the final visible page and physical rendering criteria.
