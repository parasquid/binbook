# Diagnostic Custom LUT Probes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use inline sequential execution for this repo. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add diagnostic-only SSD1677 custom LUT probes that measure whether shorter staged grayscale waveform candidates reduce BUSY time without changing normal page-turn behavior.

**Architecture:** Keep normal reading pinned to the existing `STAGED_GRAY_LUT`. Add fixed diagnostic probe IDs for two firmware-owned candidate LUTs, route them through the existing `DisplayProbe` protocol, and execute them in `xteink-x4-display` with existing BUSY timing observers. The probe writes a visible full-screen transition pattern, loads the selected candidate waveform, triggers staged grayscale activation, and reports timing through the existing diagnostic log path.

**Tech Stack:** Rust workspace crates `binbook-diagnostic-protocol`, `binbook`, `binbook-fw`, `xteink-x4-display`, and `ssd1677-driver`; tests via `cargo test`; live verification via diagnostic firmware flash, serial diagnostic CLI, and webcam capture.

---

### Task 1: Protocol and CLI Probe IDs

**Files:**
- Modify: `firmware/crates/binbook-diagnostic-protocol/src/types.rs`
- Modify: `firmware/crates/binbook-diagnostic-protocol/tests/codec.rs`
- Modify: `crates/binbook/src/diag_protocol.rs`
- Modify: `crates/binbook/src/args.rs`
- Modify: `crates/binbook/src/diag_cli.rs`
- Modify: `crates/binbook/tests/protocol.rs`

- [ ] Add failing tests that `ProbeCode::CustomLutFastA = 0x05` and `ProbeCode::CustomLutFastB = 0x06` encode/decode correctly and that CLI probe requests carry the expected payload bytes.
- [ ] Run `RUSTUP_TOOLCHAIN=stable cargo test -p binbook-diagnostic-protocol display_probe_payload_accepts_custom_lut_candidates -- --nocapture` and confirm it fails because the probe codes do not exist.
- [ ] Add the two probe variants to protocol and CLI mapping.
- [ ] Run `RUSTUP_TOOLCHAIN=stable cargo test -p binbook-diagnostic-protocol -p binbook protocol --features serial-device`.

### Task 2: Firmware Dispatch and Runtime Mapping

**Files:**
- Modify: `firmware/crates/binbook-fw/src/diag/command.rs`
- Modify: `firmware/crates/binbook-fw/src/async_refresh.rs`
- Modify: `firmware/crates/binbook-fw/src/runtime/display_task.rs`
- Modify: `firmware/crates/binbook-fw/src/runtime/diagnostic_console.rs`
- Modify: `firmware/crates/binbook-fw/src/runtime_engine.rs`
- Modify: `firmware/crates/binbook-fw/tests/diagnostic_acceptance.rs`
- Modify: `firmware/crates/binbook-fw/tests/diagnostic_dispatch.rs`

- [ ] Add failing tests that diagnostic dispatch maps probe bytes `0x05` and `0x06` to custom LUT probe kinds.
- [ ] Run `RUSTUP_TOOLCHAIN=stable cargo test -p binbook-fw --features diagnostic-console diagnostic_display_probe_acceptance -- --nocapture` and confirm it fails on the missing probe kinds.
- [ ] Add custom LUT variants through firmware diagnostic enums and runtime mapping.
- [ ] Run `RUSTUP_TOOLCHAIN=stable cargo test -p binbook-fw --features diagnostic-console diagnostic_display_probe_acceptance diagnostic_dispatch -- --nocapture`.

### Task 3: Display-Crate Candidate Waveforms and Probe Execution

**Files:**
- Modify: `crates/xteink-x4-display/src/panel.rs`
- Modify: `crates/xteink-x4-display/src/probes.rs`
- Modify: `crates/xteink-x4-display/tests/probes.rs`
- Modify: `crates/ssd1677-driver/tests/commands.rs`

- [ ] Add failing tests that candidate waveform metadata is valid, distinct from the pinned staged LUT, and preserves voltage bytes.
- [ ] Add failing tests that the custom LUT probe writes both controller RAM planes, loads a waveform, triggers staged grayscale activation, and waits on BUSY through the observer.
- [ ] Run `RUSTUP_TOOLCHAIN=stable cargo test -p xteink-x4-display custom_lut -- --nocapture` and confirm the tests fail on missing implementation.
- [ ] Add `CustomLutCandidate`, fixed candidate A/B byte arrays, and `load_custom_lut_candidate`.
- [ ] Add `custom_lut_probe_observed` that writes a visible transition pattern, loads the candidate LUT, triggers `RefreshMode::StagedGrayscale`, and waits for ready.
- [ ] Run `RUSTUP_TOOLCHAIN=stable cargo test -p xteink-x4-display`.

### Task 4: CLI Exercise and Evidence Output

**Files:**
- Modify: `crates/binbook/src/args.rs`
- Modify: `crates/binbook/src/diag_cli.rs`
- Create: `crates/binbook/src/custom_lut_probe.rs`
- Modify: `crates/binbook/src/lib.rs`
- Add or modify tests under `crates/binbook/tests/`

- [ ] Add a failing test or CLI protocol test for `binbook diag probe custom-lut-fast-a --port $PORT` and `custom-lut-fast-b`.
- [ ] Implement direct probe command routing using existing `DisplayProbe` response handling.
- [ ] Run `RUSTUP_TOOLCHAIN=stable cargo test -p binbook --features serial-device`.

### Task 5: Documentation, Host Verification, and Hardware Gate

**Files:**
- Modify: `docs/ROADMAP.md`
- Modify: `docs/reference/xteink-x4-firmware-flashing.md`
- Modify: `HANDOFF.md` if present

- [ ] Document the diagnostic-only custom LUT probes, candidate IDs, commands, and acceptance caveats.
- [ ] Run `RUSTUP_TOOLCHAIN=stable cargo test --workspace`.
- [ ] Run `RUSTUP_TOOLCHAIN=stable cargo test -p binbook-fw --features diagnostic-console`.
- [ ] Build firmware with `cd firmware && RUSTC="$(rustup which --toolchain nightly rustc)" rustup run nightly cargo build -p binbook-fw --features firmware-bin,diagnostic-console --target riscv32imc-unknown-none-elf --release`.
- [ ] Flash diagnostic firmware with `FW_FEATURES="firmware-bin,diagnostic-console" firmware/scripts/flash-xteink-x4-nav-probe.sh`.
- [ ] Run `target/debug/binbook diag probe --port "$PORT" custom-lut-fast-a`, collect logs, and capture a fresh webcam image.
- [ ] Run `target/debug/binbook diag probe --port "$PORT" custom-lut-fast-b`, collect logs, and capture a fresh webcam image.
- [ ] Update `HANDOFF.md` with exact observed commands, BUSY timings, output paths, and visual status. Do not claim success for any candidate without serial and webcam evidence.
