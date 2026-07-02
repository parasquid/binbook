# Handoff: Rust Modularity Refactor

Date: 2026-07-02
Branch: `refactor/rust-modularity-cleanup`
Current task: Rust codebase refactor for modularity, device-target seams, and readability

## Current Status

The refactor is implemented in the working tree. The main behavior is unchanged; this pass splits large modules, moves device-target conversion logic behind explicit target/profile helpers, reduces production coupling between host image compilation and the Xteink X4 display crate, and decomposes firmware diagnostic handling into focused command/runtime/state/serial/log modules.

## Implemented Behavior

| Area | Implementation path | Verification evidence |
| --- | --- | --- |
| Diagnostic protocol module split | `firmware/crates/binbook-diagnostic-protocol/src/{cobs,events,frame,key_page,probe,status_log,storage,types}.rs`, `firmware/crates/binbook-diagnostic-protocol/src/lib.rs` | `cargo test -p binbook-diagnostic-protocol --quiet`; `cargo clippy -p binbook-diagnostic-protocol --all-targets -- -D warnings` |
| Firmware diagnostic decomposition | `firmware/crates/binbook-fw/src/diag.rs`, `firmware/crates/binbook-fw/src/diag/{command,log_response,runtime,serial,state}.rs` | RED: `diag_command_module_exposes_dispatch_boundary` failed until `diag::command` existed; GREEN: `cargo test -p binbook-fw --features diagnostic-console --quiet`; live `diag hello` and `diag status` after flashing |
| Diagnostic storage error typing | `firmware/crates/binbook-fw/src/diag_storage.rs` | RED: `cargo clippy -p binbook-fw --features diagnostic-console --all-targets -- -D warnings` failed on `Result<_, ()>`; GREEN: same command passed |
| X4 geometry and GRAY2 conversion helpers | `crates/xteink-x4-display/src/profile.rs`, `crates/xteink-x4-display/src/ui_render.rs`, `crates/xteink-x4-display/src/probes.rs` | `cargo test -p xteink-x4-display --quiet`; `cargo clippy -p xteink-x4-display --all-targets -- -D warnings` |
| Host image target seam | `crates/binbook-image/src/target.rs`, `crates/binbook-image/src/compile.rs`, `crates/binbook-image/Cargo.toml` | `cargo test -p binbook-image --quiet`; `cargo tree -p binbook-image --edges normal --no-dedupe` shows no normal `xteink-x4-display` dependency |
| Menu rendering split | `firmware/crates/binbook-fw/src/menu.rs`, `firmware/crates/binbook-fw/src/menu_render.rs`, `firmware/crates/binbook-fw/src/lib.rs` | `cargo test -p binbook-fw --features diagnostic-console --quiet` |
| CLI diagnostic command split | `crates/binbook/src/main.rs`, `crates/binbook/src/diag_cli.rs`, `crates/binbook/src/lib.rs` | `cargo test -p binbook --features serial-device --quiet`; `cargo clippy -p binbook --all-targets --features serial-device -- -D warnings` |

## Verification Evidence

Host checks that passed:

```bash
RUSTUP_HOME=/var/home/tristan/.rustup CARGO_HOME=/var/home/tristan/.cargo rustup run stable cargo fmt --all -- --check
RUSTUP_HOME=/var/home/tristan/.rustup CARGO_HOME=/var/home/tristan/.cargo rustup run stable cargo test -p binbook-diagnostic-protocol --quiet
RUSTUP_HOME=/var/home/tristan/.rustup CARGO_HOME=/var/home/tristan/.cargo rustup run stable cargo test -p xteink-x4-display --quiet
RUSTUP_HOME=/var/home/tristan/.rustup CARGO_HOME=/var/home/tristan/.cargo rustup run stable cargo test -p binbook-image --quiet
RUSTUP_HOME=/var/home/tristan/.rustup CARGO_HOME=/var/home/tristan/.cargo rustup run stable cargo test -p binbook-fw --features diagnostic-console --quiet
RUSTUP_HOME=/var/home/tristan/.rustup CARGO_HOME=/var/home/tristan/.cargo rustup run stable cargo test -p binbook --features serial-device --quiet
RUSTUP_HOME=/var/home/tristan/.rustup CARGO_HOME=/var/home/tristan/.cargo rustup run stable cargo test --workspace --quiet
RUSTUP_HOME=/var/home/tristan/.rustup CARGO_HOME=/var/home/tristan/.cargo rustup run stable cargo clippy -p binbook-diagnostic-protocol --all-targets -- -D warnings
RUSTUP_HOME=/var/home/tristan/.rustup CARGO_HOME=/var/home/tristan/.cargo rustup run stable cargo clippy -p binbook-fw --features diagnostic-console --all-targets -- -D warnings
RUSTUP_HOME=/var/home/tristan/.rustup CARGO_HOME=/var/home/tristan/.cargo rustup run stable cargo clippy -p xteink-x4-display --all-targets -- -D warnings
RUSTUP_HOME=/var/home/tristan/.rustup CARGO_HOME=/var/home/tristan/.cargo rustup run stable cargo clippy -p binbook --all-targets --features serial-device -- -D warnings
git diff --check
```

Python verification passed with Homebrew `uv` and a temporary `cargo` wrapper that pins stable, because this shell has no rustup default toolchain configured:

```bash
PATH="/tmp/binbook-test-bin:/home/linuxbrew/.linuxbrew/bin:$PATH" \
  RUSTUP_HOME=/var/home/tristan/.rustup \
  CARGO_HOME=/var/home/tristan/.cargo \
  uv run pytest -q
```

Result:

```text
67 passed, 26 skipped in 6.85s
```

Firmware target build passed:

```bash
cd firmware && \
  RUSTUP_HOME=/var/home/tristan/.rustup \
  CARGO_HOME=/var/home/tristan/.cargo \
  RUSTC="$(RUSTUP_HOME=/var/home/tristan/.rustup CARGO_HOME=/var/home/tristan/.cargo rustup which --toolchain nightly rustc)" \
  rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release
```

Result:

```text
Finished `release` profile [optimized] target(s) in 0.17s
```

## Live Hardware Evidence

Flashed diagnostic firmware:

```bash
PATH="/home/linuxbrew/.linuxbrew/bin:/var/home/tristan/.cargo/bin:$PATH" \
  HOME=/var/home/tristan \
  RUSTUP_HOME=/var/home/tristan/.rustup \
  CARGO_HOME=/var/home/tristan/.cargo \
  ESPFLASH=/var/home/tristan/.cargo/bin/espflash \
  ESPFLASH_PORT=/dev/ttyACM0 \
  FW_FEATURES="firmware-bin,diagnostic-console" \
  firmware/scripts/flash-xteink-x4-nav-probe.sh
```

Relevant output:

```text
Serial port: '/dev/ttyACM0'
Chip type:         esp32c3 (revision v0.4)
Crystal frequency: 40 MHz
Flash size:        16MB
Features:          WiFi, BLE
App/part. size:    1,140,176/16,384,000 bytes, 6.96%
Flashing has completed!
```

Captured 15 seconds of boot serial with the `AGENTS.md` pyserial command. Relevant output:

```text
ESP-IDF v5.5.1-838-gd66ebb86d2e 2nd stage bootloader
SPI Speed      : 40MHz
SPI Mode       : DIO
SPI Flash Size : 16MB
Loaded app from partition at offset 0x10000
Disabling RNG early entropy source...
```

Verified diagnostic transport after flashing:

```bash
RUSTUP_HOME=/var/home/tristan/.rustup CARGO_HOME=/var/home/tristan/.cargo \
  rustup run stable cargo run -p binbook --features serial-device -- diag hello --port /dev/ttyACM0
RUSTUP_HOME=/var/home/tristan/.rustup CARGO_HOME=/var/home/tristan/.cargo \
  rustup run stable cargo run -p binbook --features serial-device -- diag status --port /dev/ttyACM0
```

Output:

```text
protocol=2 max_frame=4126 capabilities=KEY,PAGE,STATUS,LOG,CRASH,DISPLAY_PROBE,STORAGE firmware=binbook-fw target=xteink-x4
current_page=0 page_count=16 panel_mode=Grayscale dropped_log_count=0 protocol_error_count=0 last_error=0
```

## Known Caveats

- This is a structural refactor. It does not introduce new target selection semantics beyond the existing X4 target behavior.
- `diag::command` is now a public module to make the dispatch boundary explicit; root-level `diag::*` re-exports remain for existing callers.
- Per-opcode command handlers are still grouped in `diag::command`; splitting each opcode into separate files was not necessary for this pass.
- Visual display output was not re-captured for this refactor because the changed firmware-visible paths were protocol/menu/geometry organization and the diagnostic transport was verified live.
