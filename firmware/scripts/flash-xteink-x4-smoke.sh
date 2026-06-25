#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

printf 'flash-xteink-x4-smoke.sh is retained for compatibility; flashing current GRAY2 render probe instead.\n' >&2
exec "${SCRIPT_DIR}/flash-xteink-x4-gray2-probe.sh"
