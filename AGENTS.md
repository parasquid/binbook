# Agent Instructions

## Project Context

- Treat `BINBOOK_FORMAT_SPEC.md` as the authoritative BinBook 0.1 candidate file-format specification.
- Treat `CODEX_PROMPT_BINBOOK_POC.md.md`, `BINBOOK_POC_SPEC.md.md`, and `BINBOOK_DOCS_AND_ROADMAP.md.md` as historical POC context only.
- This repo is the Python reference implementation for BinBook 0.1, a compiled raster-book format for low-RAM e-ink/display devices.
- The first target profile is `xteink-x4-portrait`: logical `480x800`, physical `800x480`, `GRAY2_PACKED` by default, optional `GRAY1_PACKED` for explicit fast/lower-quality output, logical-to-physical rotation `90` degrees clockwise.

## Setup and Commands

- Use `uv` for dependency management and command execution.
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

## Implementation Guidelines

- Keep required runtime metadata binary; do not add JSON/CBOR/protobuf sections to `.binbook`.
- Preserve canonical BinBook GRAY2 storage for default `xteink-x4-portrait` output: `0=black`, `1=dark gray`, `2=light gray`, `3=white`, packed row-major MSB-first.
- Allow `GRAY1_PACKED` for `xteink-x4-portrait` only when explicitly configured. Do not emit `GRAY4_PACKED` for this profile.
- Page blobs store book content pixels only; reader/viewer chrome is rendered separately.
- Prefer small, focused modules with tests for binary layout, validation, rendering, and CLI behavior.
- Add or update tests before implementation changes when practical.
- Run `uv run pytest -q` before claiming implementation work is complete.

## Behavioral Preferences

- Treat user questions as requests for explanation by default.
- Do not implement changes in response to a question unless the user explicitly asks to implement, fix, add, commit, or change code.
- If the user asks "can we", "is there", "how do I", "what about", or similar, answer the question directly instead of starting implementation.
- If an answer suggests a possible code change, explain the option and ask before editing.
- When unsure whether the user wants action or explanation, ask before editing files.
- Keep responses concise and factual.
