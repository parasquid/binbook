#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIRMWARE_DIR="${ROOT}/firmware"
ESPFLASH="${ESPFLASH:-${HOME}/.cargo/bin/espflash}"
ESPFLASH_PORT="${ESPFLASH_PORT:-/dev/ttyACM0}"
IMAGE="${IMAGE:-${ROOT}/target/riscv32imc-unknown-none-elf/release/binbook-fw}"

if [[ ! -x "${ESPFLASH}" ]]; then
  printf 'espflash not found or not executable: %s\n' "${ESPFLASH}" >&2
  printf 'Install it with: cargo install espflash\n' >&2
  exit 1
fi

if [[ ! -r "${ESPFLASH_PORT}" || ! -w "${ESPFLASH_PORT}" ]]; then
  printf 'ESP32-C3 serial port is not readable/writable: %s\n' "${ESPFLASH_PORT}" >&2
  printf 'Set ESPFLASH_PORT=/path/to/device if the Xteink X4 is on a different port.\n' >&2
  exit 1
fi

cd "${FIRMWARE_DIR}"

FW_FEATURES="${FW_FEATURES:-firmware-bin,debug-log}"

RUSTC="${RUSTC:-$(rustup which --toolchain nightly rustc)}" \
  rustup run nightly cargo \
  build -p binbook-fw --features "${FW_FEATURES}" --target riscv32imc-unknown-none-elf --release

"${ESPFLASH}" flash \
  --non-interactive \
  --chip esp32c3 \
  --port "${ESPFLASH_PORT}" \
  --flash-size 16mb \
  "${IMAGE}"
