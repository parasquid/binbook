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
- Keep the repository `.python-version` pinned to Python 3.13 unless the
  Python dependency set is verified on a newer interpreter. `pygame==2.6.1`
  has a locked Linux wheel for CPython 3.13 but may build from source on newer
  interpreters; if `uv` tries to compile pygame and reports a missing compiler
  such as `gcc-13`, first verify `uv run python --version` and the
  `.python-version` pin instead of installing random system compiler packages.
- On this atomic Linux development host, use Homebrew for host tool installs.
  Do not use `dnf`, `rpm-ostree`, or other base-OS package manager changes
  unless the user explicitly asks.
- Use `cargo` for Rust firmware work.
- Do not trial-run commands in the sandbox when repo guidance or prior evidence shows they need host access. Run known host-bound commands with escalation up front, including `git add`, `git commit`, history rewrites, `git push`/`gh`, hardware flashing or serial access, and dependency/network fetches.
- For hardware, USB, serial, flashing, monitor, SD-card, block-device, or mounted-media work, never treat sandboxed `/dev`, `/run/media`, mount, or port visibility as evidence. Do not run a sandboxed "quick check" first. Use a single escalated command up front, or clearly state that host/device access was not checked.
- If a hardware or serial command is part of the requested verification, run the actual host-bound command with escalation instead of substituting a sandboxed existence check. Only report "not visible", "not connected", or "blocked" after an escalated host check fails.
- Never skip a verification step by preemptively assuming it will fail in the sandbox. Run the command and let the escalation mechanism handle access. The only valid reason to skip a step is if the plan or user explicitly says it is out of scope.
- Install/sync dependencies with:

```bash
uv sync --dev
```

- Run the full test suite with:

```bash
uv run pytest -q
```

- Run firmware host tests separately with:

```bash
cd firmware && cargo test --workspace
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

## Firmware Serial Monitor

To capture serial output from the Xteink X4 device:

```bash
uv run --with pyserial --no-project python3 -c "
import serial, time, sys
ser = serial.Serial('/dev/ttyACM0', 115200, timeout=1)
ser.dtr = False; ser.rts = False; time.sleep(0.05)
ser.rts = True; time.sleep(0.05); ser.rts = False; time.sleep(0.1)
start = time.time()
while time.time() - start < 15:
    data = ser.read(ser.in_waiting or 1)
    if data:
        sys.stdout.buffer.write(data)
        sys.stdout.flush()
ser.close()
"
```

Note: `pyserial` is not a project dependency. Use `uv run --with pyserial --no-project` to get it
ad-hoc. Do not add it to `pyproject.toml` — it's only needed for hardware serial monitoring.
`espflash monitor` does not work headless (fails with `Failed to initialize input reader`).

## Git / Branching

- Do not create or switch branches unless the user explicitly asks.
- By default, make requested edits in the current branch/worktree.
- If branch isolation seems important, ask first and explain why.
- Never use `git add -A` or `git add .`. Always stage specific files by path.
- Never amend, force-push, or rewrite history without explicit user request.
- Use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) for all commit messages. Format: `<type>[optional scope]: <description>` where type is one of `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, or `revert`.
- When implementing a written plan, always use a todo tracker and keep it current as tasks move from pending to in progress to complete.
- Only add `Co-authored-by` trailers when the agent can truthfully claim authorship of the changed files. Do not include misleading co-author footers on commits where the agent merely organized or staged existing work.

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

### Plan Writing Conventions

- Write plans assuming the executing agent starts cold with no prior conversation context. Include all necessary file paths, commands, and constraints explicitly.
- Emphasize test-driven development: each task should begin with a failing test, followed by the minimal implementation to pass it.
- Require relevant tests to pass after each task before moving to the next. If a task leaves tests failing, the plan is incomplete.
- Make hardware verification an explicit completion gate for firmware tasks. Include the exact flashing/serial commands needed to confirm behavior on real hardware, and do not mark firmware work complete without evidence from a live device run.

### Documentation Discipline

- Include documentation work explicitly in implementation plans.
- Create new docs when needed, update related existing docs in the same change, remove or revise obsolete docs.
- Update `HANDOFF.md` after completing a task when the next agent would benefit from current status, verification evidence, blockers, commands, or remaining work. Keep it ready for another agent to pick up without relying on chat context.
- `HANDOFF.md` is a current-state snapshot, not a diary. Overwrite the relevant sections with up-to-date information rather than appending new entries. Remove or revise stale content.
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
- Firmware debug logging uses the `debug-log` Cargo feature, which gates `esp-println`. Define a `dbgprintln!` macro in `main.rs` that expands to `esp_println::println!` when `debug-log` is enabled and is a no-op otherwise. Replace all `println!` calls with `dbgprintln!` so debug output compiles out entirely in non-debug builds.
- To build with debug logging: `--features firmware-bin,debug-log`. Without: `--features firmware-bin` (no esp-println dependency).
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
- Execute implementation plans directly without delegating to subagents. Work sequentially, marking todo items as you go.
