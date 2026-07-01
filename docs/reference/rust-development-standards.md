# Rust and Firmware Development Standards

## Scope and authority

Use this guide for every Rust workspace or Xteink X4 firmware change. It defines
how to place, implement, test, and verify work. It is written for contributors
and coding agents that have no prior refactor context.

Read these sources before changing behavior:

1. [`BINBOOK_FORMAT_SPEC.md`](../../BINBOOK_FORMAT_SPEC.md) defines BinBook 0.1
   bytes, records, color values, plane slots, and chunk rules.
2. [Rust crate architecture](rust-crate-architecture.md) defines crate ownership,
   allowed dependencies, public integration contracts, and build commands.
3. [Xteink X4 agent device verification](xteink-x4-agent-device-verification.md)
   defines the mandatory live-device procedure.
4. [`AGENTS.md`](../../AGENTS.md) defines repository-wide execution, testing,
   hardware, Git, and documentation rules.

If this guide conflicts with one of those sources, follow the more specific
authoritative source and correct this guide in the same change.

## Change workflow

Follow these steps in order.

1. Read the applicable specification, architecture section, crate manifest,
   public API, tests, and callers.
2. Select the crate that owns the behavior. Do not start in `binbook-fw` merely
   because firmware uses the behavior.
3. Record acceptance criteria as observable results. For firmware, separate
   automated, serial, queried-state, and visible-display evidence.
4. Write the smallest discriminating failing test. Confirm that it fails for the
   intended reason before changing production code.
5. Implement the minimum behavior in the owning crate. Keep integration code at
   the boundary and preserve lower-layer error categories.
6. Run focused tests during development. Then run every gate required by the
   change classification in this guide.
7. Drive the artifact through its real surface. A parser must parse real bytes.
   A command must execute and change state. Firmware must run on the device.
8. Update current reference docs and `HANDOFF.md` when another agent needs the
   resulting state, evidence, commands, or known limitations.
9. Perform an adversarial review. Try to expose a no-op, stale state, ignored
   feature, boundary error, hidden allocation, or false completion claim.

Do not broaden a change into unrelated cleanup. Report important adjacent debt,
but keep the implementation and verification tied to the requested behavior.

## Choose the owning crate

Use the narrowest owner that has enough domain knowledge to implement the rule.

| Behavior | Owning crate | Does not belong in |
|---|---|---|
| BinBook records, bounds, typed indices, random-access reads | `binbook-core` | codecs, display, firmware |
| PackBits or LZ4 decoding | `binbook-decompress` | parser, display engine, firmware |
| Canonical GRAY2, staged planes, dithering, row conversion | `gray2-render` | controller driver, firmware |
| SSD1677 commands, windows, RAM writes, waits, controller power state | `ssd1677-driver` | X4 policy, firmware |
| X4 geometry, rotation, page streaming, refresh policy, cancellation, probes | `xteink-x4-display` | generic driver, firmware |
| Diagnostic frame types and codecs | `binbook-diagnostic-protocol` | display crates |
| ESP32-C3 wiring, Embassy tasks, storage/input adapters, application lifecycle | `binbook-fw` | reusable crates |
| Host commands, serial transport, human-readable output | `binbook-cli` | firmware and reusable crates |

Dependencies point toward lower-level owners. Reusable crates must not depend on
`binbook-fw`, the CLI, diagnostic protocol, Embassy, `esp-hal`, or repository
fixture paths. Do not bypass Cargo boundaries with `#[path]` or include files
from sibling crates.

When behavior spans layers, split it at a semantic interface. For example,
`xteink-x4-display` emits typed display events. Firmware maps those events to
diagnostic protocol numbers. The display crate must not emit diagnostic frames.

## API and dependency standards

### Keep reusable crates portable

- Reusable crates remain `#![no_std]` and independently compilable.
- Use `embedded-hal` 1.0, `embedded-hal-async`, and `embedded-storage` at hardware
  and storage boundaries.
- Do not introduce a repository-specific transport HAL.
- Keep board aliases, fixed GPIO assignments, ADC handling, partitions, Embassy
  channels, and ESP peripheral types in `binbook-fw`.
- Features must be additive and explicit. Code behind a feature requires tests
  that run with that feature enabled.

### Make invalid states harder to express

- Use newtypes or enums for page numbers, plane slots, chunk indices, byte
  lengths, offsets, refresh modes, and controller states.
- Validate raw wire values when converting into typed values. Do not pass an
  unvalidated `u32` through several layers.
- Make state transitions explicit. Commit the current page only after the
  required display operation succeeds.
- Keep semantic events independent of transport and numeric diagnostic codes.
- Avoid placeholder metadata, hard-coded status, ignored options, and public
  methods that imply behavior they do not implement.

### Keep modules focused

Prefer one clear responsibility per module. Split a new or substantially
rewritten production file before it exceeds 250 logical lines. Do not use broad
`allow` attributes to hide dead code, unused imports, warnings, or unsafe code.
Workspace lints deny warnings and unsafe code; any platform-required unsafe
startup code must be narrow and documented at its boundary.

## Memory and streaming standards

RAM is constrained by default.

- Callers own section-table, record, compressed, decoded-row, and output-plane
  buffers.
- APIs accept borrowed slices and report exact `required` and `provided` sizes
  when a buffer is too small.
- Stream records, chunks, rows, and controller writes. Retain only bounded decoder
  or controller state between calls.
- Compute expected output sizes from validated source metadata and format rules.
  Do not infer them from the destination buffer length.
- Test payloads larger than scratch buffers. A fixture that fits entirely in one
  scratch buffer does not prove streaming.
- Check whether struct moves, async state machines, FFI calls, or generic adapters
  create hidden stack copies before increasing stack or heap limits.

Do not add:

- full-page decode or render buffers to reusable paths;
- hidden fixed 8 KiB scratch arrays;
- allocation as a substitute for a caller-owned buffer;
- larger task stacks as the final fix for unexplained failures;
- aggregates that concatenate compressed planes before decoding.

LZ4 is the documented exception to strip-by-strip decoding: the current codec
requires complete compressed input and decoded output slices. Those slices remain
caller-owned and the `lz4` feature remains explicit.

## Error and state standards

Each layer owns its error vocabulary:

- source and format errors stay distinct in `binbook-core`;
- malformed input, exact-size mismatches, unsupported methods, and disabled LZ4
  stay distinct in `binbook-decompress`;
- geometry and output-buffer failures belong to `gray2-render`;
- SPI, pin, timeout, window, buffer, and controller-state failures belong to
  `ssd1677-driver`;
- source, decode, render, controller, cancellation, recovery, and X4 profile
  failures remain distinguishable in `xteink-x4-display`;
- board, queue, storage, transport, and lifecycle failures belong to `binbook-fw`.

Upper layers may translate an error into their own enum, but must preserve the
category. Do not turn every failure into `false`, `-1`, an empty payload, or a
generic transport error. Diagnostic numeric codes must include a readable name
when logged.

Cancellation and boundary no-ops are outcomes, not silent successes. Emit enough
semantic state for firmware and tests to distinguish completed, cancelled,
failed, and boundary-noop operations.

## Test standards

### Start with a discriminating failure

A test must fail if the implementation is absent or a branch is a no-op. Use a
starting state that exposes the behavior:

- navigate to page 0 from a nonzero page;
- clear a log that already contains known events;
- retrieve logs after generating known events;
- split a PackBits run across both input and output strips;
- use a payload larger than the scratch buffer;
- provide an output buffer exactly one byte too small;
- cancel during the specific staged-refresh phase under test.

Parser or command-decoder tests do not prove execution. Add behavior tests at the
owner and integration tests at each translated boundary.

### Preserve exact contracts where bytes matter

Use golden or exact-sequence assertions for format layouts, controller command
order, LUTs, voltage settings, protocol frames, plane hashes, and canonical pixel
values. Derive expected values independently from the specification or reference
fixtures. Do not copy expected values from the function under test.

Do not replace behavioral protection with source-text assertions. A source-shape
check may supplement a compile-time, API, or behavioral test, but cannot replace
one.

### Run feature-gated tests explicitly

Default workspace tests do not execute code excluded by `cfg(feature = ...)`.
Run the relevant features by name. Common gates include:

```bash
cargo test -p binbook-decompress --all-features
cargo test -p binbook-fw --features diagnostic-console
cargo test -p binbook-cli --features serial-device
```

Debug measurement and logging must use feature or compile-time guards and compile
out of normal release builds. Firmware logging uses the repository's `debug-log`
feature and `dbgprintln!` pattern; do not add unconditional `println!` calls.

## Verification by change type

Run commands from the repository root unless a command starts with `cd firmware`.

### Every Rust change

```bash
cargo fmt --all -- --check
cargo test -p <owning-package>
cargo test --workspace
git diff --check
```

`<owning-package>` is an explicit placeholder. Replace it with the changed Cargo
package name.

### Reusable crate changes

Run the package's focused test, Clippy, and firmware-target check. For
`binbook-decompress`, include all features in tests and Clippy.

```bash
cargo clippy -p <reusable-package> --all-targets --all-features -- -D warnings
cargo check -p <reusable-package> --no-default-features --target riscv32imc-unknown-none-elf
```

`<reusable-package>` is an explicit placeholder. For crates without features,
`--all-features` is harmless. Inspect `cargo metadata --no-deps --format-version
1` and `cargo tree -p <reusable-package>` when dependencies change.

### Firmware changes

```bash
cargo test -p binbook-fw
cargo test -p binbook-fw --features diagnostic-console
cargo clippy -p binbook-fw --all-targets --features diagnostic-console -- -D warnings
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
```

Use the pinned nightly `cargo` and `rustc`. Firmware builds use
`firmware/.cargo/config.toml` and write to the root `target/` directory.

### Format, rendering, or authoring compatibility changes

Run the focused Rust tests plus Python compatibility tests. Run the full Python
suite before completion:

```bash
uv run pytest -q
```

If the change touches kerning proof behavior, also run:

```bash
uv run pytest -q tests/test_kerning_proof.py --run-proof
```

Start final regression work from `cargo clean` when stale artifacts could mask a
failure. A clean build is required by the current implementation plan or hardware
runbook when either source says so.

## Hardware acceptance rules

Hardware verification is mandatory for firmware tasks unless the user or the
approved plan explicitly excludes it. Host tests and successful builds are
necessary but do not prove hardware behavior.

Follow the complete [Xteink X4 device verification runbook](xteink-x4-agent-device-verification.md).
At minimum, the evidence must include:

1. the exact pinned firmware build and flash command with relevant output;
2. at least 15 seconds of captured serial output;
3. the request payload and response opcode, sequence, status, and payload;
4. a known starting state and the expected ending state;
5. an independent STATUS or log query that confirms the result;
6. a fresh native-resolution webcam capture when display behavior is involved;
7. inspection of the actual webcam file, including orientation, clipping,
   stale-region, and grayscale checks where relevant.

Run flash, serial, diagnostic, and webcam commands sequentially. Only one process
may own `/dev/ttyACM0`. Never run hardware or serial commands in parallel.

### Evidence levels

Keep these claims separate:

| Evidence | What it proves | What it does not prove |
|---|---|---|
| Build passes | The selected source and features compile | Firmware booted or ran correctly |
| Flash command succeeds | An image was transferred | The application initialized correctly |
| Command acknowledgement | Transport and dispatch returned a response | The requested action occurred |
| STATUS or log query | Reported state and events match expectations | The panel visibly rendered correctly |
| Fresh inspected webcam capture | The panel's visible result | Internal state or protocol correctness |

For display changes, completion normally requires all five levels. A decoded
fixture, simulator image, or old capture is not live display evidence.

## Completion evidence and handoff

Before declaring a plan complete, create an acceptance matrix with one row per
requirement:

| Requirement | Implementation path | Automated test | Serial or queried state | Webcam result |
|---|---|---|---|---|
| Describe one observable requirement | Name the owning module or API | Name the exact test and result | Record exact evidence or `Not applicable` | Record exact evidence or `Not applicable` |

The row above is an illustrative template, not evidence. Do not use blank cells.
Use `Not applicable` only when that evidence type cannot prove the requirement.
An unverified required cell keeps the work incomplete.

Keep `HANDOFF.md` as a current-state snapshot. Replace stale sections instead of
appending a diary. It must distinguish:

- verified behavior;
- transport-only acknowledgements;
- unverified visual results;
- known failures and incomplete requirements;
- exact commands, relevant output, artifact paths, starting state, and ending
  state needed by the next agent.

Do not write `complete`, `passed`, or `all commands work` if a required result is
unobserved, placeholder data is returned, or source inspection contradicts the
claim.

## Allowed and forbidden patterns

| Concern | Allowed | Forbidden |
|---|---|---|
| Storage access | Implement `ReadAt` or an `embedded-storage` adapter in the integration layer | Read flash directly from `binbook-core` |
| Display transport | Supply `SpiDevice<u8>`, pins, and delays to the generic driver | Add a custom repository HAL or ESP types to `ssd1677-driver` |
| Temporary memory | Borrow caller-owned slices and return exact size errors | Hide a full-page or fixed 8 KiB scratch array |
| Page state | Commit the page after successful display completion | Update the current page when a request is merely queued |
| Errors | Translate while preserving source/decode/render/controller category | Collapse failures into `-1`, `false`, or an empty payload |
| Feature behavior | Run tests with the named feature enabled | Treat default workspace tests as feature coverage |
| Commands | Verify dispatch, state change, and an independent follow-up query | Treat an `Ok` acknowledgement as completion |
| Display QA | Inspect a fresh `/dev/video1` capture from the current run | Substitute a fixture decode or previous capture |
| Documentation | State current requirements and observed evidence | Record a chronological work diary or unsupported success claim |

## Pre-completion checklist

- [ ] Every changed behavior has one clear owning crate.
- [ ] Cargo metadata and dependency inspection show no forbidden edge.
- [ ] Reusable crates still compile with `no_std` for the firmware target.
- [ ] Temporary memory is caller-owned, bounded, and exercised beyond scratch
      boundaries.
- [ ] Errors preserve their category through each changed boundary.
- [ ] A discriminating test failed before implementation and now passes.
- [ ] Every affected feature gate was tested explicitly.
- [ ] Formatting, focused tests, workspace tests, Clippy, target checks, firmware
      builds, and Python tests required by the change type pass.
- [ ] The real user or device surface produced the requested observable result.
- [ ] Firmware evidence includes flash, serial, independent state queries, and a
      fresh inspected webcam capture where display output matters.
- [ ] `HANDOFF.md` and the acceptance matrix contain current evidence with no
      missing required cells.
- [ ] A final adversarial test from a non-default state did not expose a no-op,
      stale response, ignored option, boundary failure, or false completion claim.
