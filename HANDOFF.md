# Handoff: Xteink X4 Async Deferred-Grayscale Refresh

Date: 2026-06-28

## Current State

Task 6 is complete on host verification and ready to commit. Tasks 1 through 5 are complete and verified on host tests and target builds.

Implemented so far:

- `firmware/crates/xteink-hal/src/lib.rs`
  - Added `AsyncDelay` for executor-driven firmware timing.
- `firmware/crates/ssd1677-driver/src/lib.rs`
  - Added `Ssd1677Driver::is_busy`.
  - Added `Ssd1677Driver::trigger_refresh`.
  - Added async wrappers: `wait_ready_async`, `init_async`, `init_grayscale_async`, and `refresh_async`.
  - Split the init command sequences into private helpers so blocking and async paths share the same command layout.
  - Changed `refresh_with_delay` to compose `trigger_refresh` plus `wait_ready_with_delay`.
- `firmware/crates/binbook-fw/src/lib.rs`
  - Re-exported the new `async_refresh` module.
- `firmware/crates/binbook-fw/src/async_refresh.rs`
  - Added the coordinator constants, enums, request/completion types, and the minimal state machine needed for startup, BW refresh completion, gray delay, reseed, and recovery transitions.
- `firmware/crates/binbook-fw/src/display.rs`
  - Added async strip-streaming helpers for grayscale rendering, BW reseeding, BW differential, and recovery.
  - The async helpers keep the PackBits decoder alive across 16-row strips and yield between strips as required.
- `firmware/crates/binbook-fw/src/runtime.rs`
  - Added an Embassy task runtime module that owns bounded request/completion channels and spawns async input, display, and diagnostic tasks behind `firmware-bin`.
  - Input presses queue while gray refresh is deferred, and the display task yields between strips and during gray delay checks.
  - The diagnostic task keeps STATUS and LOG_GET responsive while render completions are pending and returns queue-full errors without evicting older requests.
- `firmware/crates/binbook-fw/src/main.rs`
  - Added the `firmware-bin` async entrypoint hook and kept the old blocking entrypoint behind `not(feature = "firmware-bin")`.
  - Added the approved 20 MHz SPI constant string required by the runtime regression test.
- `firmware/crates/binbook-fw/src/diag.rs`
  - KEY requests now preserve `PageTurn` through `PendingAction::RenderTurn` instead of resolving to a target page too early.
  - Added `DiagnosticSnapshot` and `DiagnosticPendingQueue` as the host-testable seams for committed STATUS reads and bounded deferred command queuing.
- `firmware/crates/binbook-fw/src/diag_log.rs`
  - Re-exported the new deferred-gray event codes for turns, reseed, refresh phase, and display recovery.
- `firmware/crates/binbook-diagnostic-protocol/src/lib.rs`
  - Added stable nonzero event codes for deferred-gray diagnostics.
- `cli/src/lib.rs`
  - Added CLI event-name formatting for the new deferred-gray diagnostic events.
  - Added `diag exercise deferred-gray` parsing and the deferred-gray exercise runner that keeps one serial session open, validates page/status/log responses, and uses batched key transport for the queued turns.
- `cli/src/main.rs`
  - Routed `diag exercise deferred-gray` to the new exercise runner.
- `cli/tests/protocol.rs`
  - Added parser coverage for the deferred-gray exercise subcommand.
- `cli/tests/hardware_diagnostic.rs`
  - Added a scripted fake-serial exercise harness and an ignored live-hardware wrapper for the deferred-gray flow.

Host verification:

- `cargo test --offline -p binbook-fw --test firmware_logic firmware_runtime_uses_approved_async_configuration -- --exact`
- `cargo test --offline -p binbook-fw --test async_refresh display_request_channel_is_fifo_and_rejects_the_seventeenth_request -- --exact`
- `CARGO_HOME=/tmp/binbook-cargo-home RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo check --offline -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf`
- `CARGO_HOME=/tmp/binbook-cargo-home RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin --target riscv32imc-unknown-none-elf --release`
- `CARGO_HOME=/tmp/binbook-cargo-home RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,debug-log --target riscv32imc-unknown-none-elf --release`
- `cargo test -p binbook-diagnostic-protocol deferred_gray_event_codes_are_stable_and_nonzero -- --exact`
- `cargo test -p binbook-cli cli_logs_formats_event_names_and_sequences -- --exact`
- `cargo test --offline -p binbook-fw --features diagnostic-console --test firmware_logic batched_key_presses_are_resolved_when_dequeued -- --exact`
- `cargo test --offline -p binbook-fw --features diagnostic-console --test firmware_logic diagnostic_pending_queue_rejects_the_seventeenth_command_without_evicting_old_requests -- --exact`
- `cargo test -p binbook-diagnostic-protocol`
- `cargo test -p binbook-fw --features diagnostic-console --test firmware_logic`
- `CARGO_HOME=/tmp/binbook-cargo-home RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo check --offline -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf`
- `CARGO_HOME=/tmp/binbook-cargo-home RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release`
- `CARGO_HOME=/tmp/binbook-cargo-home RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console,debug-log --target riscv32imc-unknown-none-elf --release`
- `CARGO_HOME=/tmp/binbook-cargo-home cargo test --features serial-device deferred_gray_exercise -- --nocapture`
- `CARGO_HOME=/tmp/binbook-cargo-home cargo test --features serial-device --test hardware_diagnostic -- --list`
- `CARGO_HOME=/tmp/binbook-cargo-home cargo test`
- `CARGO_HOME=/tmp/binbook-cargo-home cargo test --features serial-device`

The temp `CARGO_HOME` was required so Cargo could unpack cached crates into a writable registry src directory. All verification commands passed.

No hardware flashing, serial capture, or webcam verification has been run yet. That work is still reserved for Task 9.

## Next Work

Continue with Task 7: add compile-time probe selection and complete host verification, then proceed through docs and hardware verification.
