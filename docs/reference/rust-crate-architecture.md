# Rust Crate Architecture

## Workspace layout

The Rust implementation uses the repository root `Cargo.toml` and `Cargo.lock`. Build artifacts are written under the root `target/` directory.

| Crate | Responsibility | Allowed direct dependencies |
|---|---|---|
| `binbook-core` | BinBook parsing, validation, typed records, random-access reads | none beyond small `no_std` utilities |
| `binbook-decompress` | Exact PackBits and optional LZ4 decoding | `binbook-core`, optional `lz4_flex` |
| `gray2-render` | Pure canonical/staged GRAY2 and ordered-dither row conversion | none beyond small `no_std` utilities |
| `ssd1677-driver` | Generic SSD1677 command, window, RAM, refresh, reset, and wait operations | `embedded-hal`, `embedded-hal-async` |
| `xteink-x4-display` | X4 profile, streaming, refresh/cancellation policy, display engine, probes | the four reusable crates above |
| `binbook-diagnostic-protocol` | Diagnostic wire types and codecs | no firmware application dependency |
| `binbook-fw` | ESP32-C3/Embassy/storage/input/diagnostic wiring | reusable display crate, diagnostics, `embedded-storage`, Embassy, `esp-hal` |
| `binbook-cli` | Host inspection and diagnostic commands | core parser, diagnostic protocol, host-only dependencies |

Dependencies point down this table. Reusable crates do not depend on firmware, CLI, diagnostics, Embassy, ESP HALs, or repository fixture paths.

## Ownership boundaries

`binbook-core` owns binary structure, but does not decompress. `binbook-decompress` owns codecs, but does not select pages or planes. `gray2-render` owns pixel math, but does not issue controller commands. `ssd1677-driver` owns SSD1677 commands, but not X4 waveform policy. `xteink-x4-display` composes these pieces and owns X4 display policy. `binbook-fw` owns only platform integration and application coordination.

The old `rust/` package and `xteink-hal` custom transport crate are not part of the architecture. Hardware-facing reusable code uses `embedded-hal` 1.0, `embedded-hal-async`, and `embedded-storage` traits.

## Buffer ownership

- The caller supplies section-table and fixed-record scratch to `binbook-core`.
- The caller supplies compressed and decoded slices to decompression and X4 rendering.
- Row conversion writes into caller-owned output slices.
- Driver writes use caller-provided rows or windows.
- Reusable crates do not allocate full-page buffers or embed generic 8 KiB scratch arrays.

Buffer errors report exact required and provided sizes where the distinction is meaningful. Streaming APIs retain only bounded decoder or controller state between calls.

## Error boundaries

- `binbook-core`: source, format, bounds, typed-index, and buffer-size failures.
- `binbook-decompress`: malformed runs, exact-size mismatches, unsupported methods, and disabled/failed LZ4.
- `gray2-render`: invalid geometry or undersized row buffers.
- `ssd1677-driver`: SPI, pin, timeout, window, buffer, and controller-state failures.
- `xteink-x4-display`: source/decode/render/controller failures plus cancellation, recovery, and profile validation.
- `binbook-fw`: board initialization, storage adapter, queue, diagnostic transport, and application lifecycle failures.

Lower layers return typed errors; upper layers translate them without discarding their category.

## Public integration model

An external consumer opens a source with `binbook_core::Book`, reads an explicit plane/chunk descriptor into its own compressed buffer, decodes into its own row buffer with `binbook_decompress`, and either converts rows directly with `gray2_render` or delegates X4 page policy to `xteink_x4_display`. A board adapter supplies `embedded-hal` devices to `ssd1677_driver` and storage through `ReadAt` or `embedded-storage` adapters.

The `xteink-x4-display` crate is the only reusable owner of X4 dimensions, rotation, staged waveform policy, chunk geometry, page-turn semantics, and refresh state.

## Build and test commands

Run all host tests from the repository root:

```bash
cargo test --workspace
```

Run reusable crate gates independently:

```bash
cargo test -p binbook-core
cargo test -p binbook-decompress --all-features
cargo test -p gray2-render
cargo test -p ssd1677-driver
cargo test -p xteink-x4-display
cargo clippy -p binbook-core --all-targets -- -D warnings
cargo clippy -p binbook-decompress --all-targets --all-features -- -D warnings
cargo clippy -p gray2-render --all-targets -- -D warnings
cargo clippy -p ssd1677-driver --all-targets -- -D warnings
cargo clippy -p xteink-x4-display --all-targets -- -D warnings
```

Check reusable crates for the firmware target without default features:

```bash
cargo check -p binbook-core --no-default-features --target riscv32imc-unknown-none-elf
cargo check -p binbook-decompress --no-default-features --target riscv32imc-unknown-none-elf
cargo check -p gray2-render --no-default-features --target riscv32imc-unknown-none-elf
cargo check -p ssd1677-driver --no-default-features --target riscv32imc-unknown-none-elf
cargo check -p xteink-x4-display --no-default-features --target riscv32imc-unknown-none-elf
```

Build firmware with the pinned nightly toolchain:

```bash
cd firmware
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release
```

Run Python compatibility tests separately with `uv run pytest -q`. Hardware verification follows `docs/reference/xteink-x4-agent-device-verification.md` and is sequential because only one process may own the serial device.
