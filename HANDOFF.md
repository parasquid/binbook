# Handoff: Rust Multi-Format Compiler

Date: 2026-07-01
Active plan: `docs/plans/2026-07-01-rust-multiformat-compiler.md`
Current task: Task 4 — deterministic container writer

## Completed

Tasks 1 through 3 are complete.

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

## Next exact action

Start Task 4 with exact-layout and strict-validation RED tests for a `BookBuilder` that writes to `Write + Seek`. Keep the writer model independent of image/EPUB parsing and require byte-for-byte deterministic output plus a `binbook_core::validate_all` round trip.

## Hardware state

No hardware commands have run for this plan yet. Task 15 remains a mandatory completion gate: flash the Rust-generated fixture, capture at least 15 seconds of serial, independently query HELLO/STATUS/logs from a non-default page state, and inspect a fresh `/dev/video1` native capture plus the confirmed panel crop.
