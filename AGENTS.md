# Agent Instructions

## Project Context

- Treat [`BINBOOK_FORMAT_SPEC.md`](BINBOOK_FORMAT_SPEC.md) as the authoritative BinBook 0.1 candidate file-format specification.
- Treat files under [`docs/historical/`](docs/historical/) as historical POC context only.
- This repo is the Python reference implementation for BinBook 0.1, a compiled raster-book format for low-RAM e-ink/display devices.
- The first target profile is `xteink-x4-portrait`: logical `480x800`, physical `800x480`, `GRAY2_PACKED` by default, optional `GRAY1_PACKED` for explicit fast/lower-quality output, logical-to-physical rotation `270` degrees clockwise. This matches the verified SquidScript Xteink X4 target metadata.

### Firmware Context

- A lean `no_std` Rust firmware for the Xteink X4 is under development (or planned) in this repo.
- The firmware references the SquidScript project (`../SquidScript`) for hardware details (SSD1677 driver, SPI pins, button ADC ladder, power management) but has its own architecture.
- Reference doc: [`docs/reference/squidscript-and-xteink-reference.md`](docs/reference/squidscript-and-xteink-reference.md)

### Firmware Build Commands

- Run firmware host tests: `cd firmware && cargo test --workspace`
- Build firmware binary with rustup's pinned nightly `cargo` and `rustc`, not arbitrary tools from `PATH`:
  `cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release`
- Build CLI: `cd cli && cargo build`

### Modularity Constraint

**Firmware crates must be designed so other projects (e.g. SquidScript) can reimport them.**

The firmware is structured as independent crates with clear boundaries:

| Crate | Responsibility | Reusable by SquidScript? |
|-------|---------------|--------------------------|
| `binbook-core` | Format parsing, validation, page indexing | Yes — could replace SquidScript's C reader |
| `binbook-decompress` | RLE_PACKBITS, LZ4 decompression | Yes — currently inline in SquidScript display driver |
| `ssd1677-driver` | SPI command layer, init sequences | Yes — currently C code in SquidScript |
| `gray2-render` | GRAY2 plane decomposition, dithering | Yes — currently C code in SquidScript |
| `xteink-hal` | GPIO, SPI, ADC, power abstractions | Partially — SquidScript uses Zephyr HAL |
| `firmware` | Binary entry point, app logic | No — too specific |

Rules:
- No repo-level dependencies in library crates (no `#[path]` hacks, no sibling references).
- Each crate must be independently compilable and testable.
- Keep board-specific aliases, fixed GPIO mappings, and physical details in the firmware crate or `xteink-hal`, not in library crates.
- Prefer library-quality seams over one-off harness slots.

## Setup and Commands

- Use `uv` for dependency management and command execution (Python side).
- Use `cargo` for Rust firmware work.
- Do not trial-run commands in the sandbox when repo guidance or prior evidence shows they need host access. Run known host-bound commands with escalation up front, including `git add`, `git commit`, history rewrites, `git push`/`gh`, hardware flashing or serial access, and dependency/network fetches.
- Install/sync dependencies with:

```bash
uv sync --dev
```

- Run the full test suite with:

```bash
uv run pytest -q
```

- Encode an EPUB with:

```bash
uv run binbook encode path/to/book.epub -o book.binbook
```

- Encode PNG pages with:

```bash
uv run binbook encode-png-folder ./pages -o test.binbook
```

- Validate, decode, and view with:

```bash
uv run binbook inspect test.binbook --validate
uv run binbook decode test.binbook --page 0 -o page0.png
uv run binbook view test.binbook
```

## Git / Branching

- Do not create or switch branches unless the user explicitly asks.
- By default, make requested edits in the current branch/worktree.
- If branch isolation seems important, ask first and explain why.
- Never use `git add -A` or `git add .`. Always stage specific files by path.
- Never amend, force-push, or rewrite history without explicit user request.

## Implementation Guidelines

- Keep required runtime metadata binary; do not add JSON/CBOR/protobuf sections to `.binbook`.
- Preserve canonical BinBook GRAY2 storage for default `xteink-x4-portrait` output: `0=black`, `1=dark gray`, `2=light gray`, `3=white`, packed row-major MSB-first.
- Allow `GRAY1_PACKED` for `xteink-x4-portrait` only when explicitly configured. Do not emit `GRAY4_PACKED` for this profile.
- Page blobs store book content pixels only; reader/viewer chrome is rendered separately.
- Prefer small, focused modules with tests for binary layout, validation, rendering, and CLI behavior.
- Add or update tests before implementation changes when practical.
- Run `uv run pytest -q` before claiming implementation work is complete.

### Test Discipline

- Default to TDD for implementation work: write or update the smallest meaningful failing test first, then implement.
- Keep tests honest. Do not add assertions for unsupported behavior or fake firmware paths.
- For firmware work, separate host-testable logic from hardware-bound code so behavior can be driven by unit tests before flashing.
- Treat failing tests as active project risk. Distinguish pre-existing failures from regressions.
- When tests fail after a format change, determine whether the tests are stale (written for old format) or whether the code is wrong before choosing a fix.
- Test any path that reads, copies, streams, or handles a payload larger than obvious scratch buffers — fixtures that fit the buffer are no-op tests.
- Keep `cargo clean && cargo test` as the reliable baseline check; stale builds mask real failures.

### Documentation Discipline

- Include documentation work explicitly in implementation plans.
- Create new docs when needed, update related existing docs in the same change, remove or revise obsolete docs.
- Update `HANDOFF.md` after completing a task when the next agent would benefit from current status, verification evidence, blockers, commands, or remaining work. Keep it ready for another agent to pick up without relying on chat context.
- Write reference docs as current-state facts, not chronological diaries. Omit "we observed", "first this failed", etc.
- Describe what code currently does in comments, never what it used to do.
- When a commit deletes or replaces a file, grep for stale references across docs, README, and commit messages. Fix in the same commit.
- Specs belong in docs/specs and plans belong in docs/plans DO NOT use any other folders despite what another skill might tell you.

## Constrained Device RAM Discipline

- Treat RAM as a constrained resource by default in firmware and firmware-facing Rust.
- Prefer caller-owned buffers, streaming/file-backed staging, borrowed views, and in-place construction over fixed temporary arrays or by-value transfers.
- Keep fixed buffers only when they represent intentional persistent state or bounded hardware contracts.
- When diagnosing failures, test whether a buffer or FFI boundary materializes hidden temporaries before increasing stack/heap. Larger stacks are diagnostic data, not the default fix.

## Debug Instrumentation

- Wrap debug timing, measurement, and diagnostic output behind feature flags or compile-time guards so they compile out in release builds.
- Use named constants for all diagnostic thresholds, not magic numbers.
- When emitting error/trace codes, pair the number with its name for legibility (e.g., `error=-12 (ENOMEM)` not `error=-12`).

## Lessons Learned

- Prefer empirical evidence over code analysis when debugging. Add diagnostics and run the code; tracing source files generates hypotheses, running confirms them.
- When porting struct layouts between languages, verify the packed byte layout matches the encoder exactly. Off-by-N layout mismatches produce plausible-looking but wrong values.
- Compute expected output sizes from source metadata (pixel format, dimensions) rather than relying on output buffer length.
- Read the docs before experimenting. Check existing tests, specs, and reference docs first; only build new fixtures when documented coverage is genuinely missing.

## Behavioral Preferences

- Treat user questions as requests for explanation by default.
- Do not implement changes in response to a question unless the user explicitly asks to implement, fix, add, commit, or change code.
- If the user asks "can we", "is there", "how do I", "what about", or similar, answer the question directly instead of starting implementation.
- If an answer suggests a possible code change, explain the option and ask before editing.
- When unsure whether the user wants action or explanation, ask before editing files.
- Keep responses concise and factual.
