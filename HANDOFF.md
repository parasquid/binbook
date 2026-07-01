# Handoff: Rust Multi-Format Compiler

Date: 2026-07-01
Active plan: `docs/plans/2026-07-01-rust-multiformat-compiler.md`
Current task: Task 2 — shared wire encoders and strict validation

## Completed

Task 1 is complete.

- Added required `FONT_RESOURCE_INDEX` section ID 35 and its 80-byte record contract to `BINBOOK_FORMAT_SPEC.md`.
- Added no-allocation Rust parsing with typed source/style enums and validation of indices, flags, reserved bytes, and string references.
- Added Python record packing/unpacking plus an empty required writer section so transitional Python fixtures and viewer remain compatible.
- Regenerated all three canonical `nav_probe.binbook` copies; they are byte-identical.
- Updated the exact section-table scratch requirement from 720 to 760 bytes.

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

## Fixture evidence

Baseline fixture SHA-256 before Task 1:

`a8c2c7d935ce6ec6376139153e91a54111a59440dd85b62270fd072d8e47766d`

Current SHA-256 for all three copies:

`81524593bf36135562a22fd4e39b9e55c326859c6846ecf4907d0805f405a0f3`

The fixture remains 16 pages, 1,440 chunks, and 30 transitions. The hash change is the required empty section-35 table entry.

## Files changed through Task 1

- `BINBOOK_FORMAT_SPEC.md`
- `binbook/constants.py`, `reader.py`, `structs.py`, `writer.py`
- `crates/binbook-core/src/{error,font_resource,lib,section}.rs`
- `crates/binbook-core/tests/{font_resources,open}.rs`
- `tests/test_font_resources.py`
- Three canonical `nav_probe.binbook` fixture copies
- Active plan and this handoff

## Next exact action

Start Task 2 with RED tests in `crates/binbook-core/tests/encoding.rs` and `strict_validation.rs`. First cover exact little-endian encoding and exact undersized-buffer errors for the existing header, section, page, chunk, transition, navigation, chapter, and font records. Then add corruption cases one validation category at a time; do not implement production encoders or the validator before observing each intended failure.

## Hardware state

No hardware commands have run for this plan yet. Task 15 remains a mandatory completion gate: flash the Rust-generated fixture, capture at least 15 seconds of serial, independently query HELLO/STATUS/logs from a non-default page state, and inspect a fresh `/dev/video1` native capture plus the confirmed panel crop.
