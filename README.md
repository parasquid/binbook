# BinBook

BinBook is a compiled raster-book format and Rust toolchain for low-RAM e-ink devices. The first target profile is `xteink-x4-portrait`: logical 480×800, physical 800×480, 270-degree clockwise mapping, staged `GRAY2_PACKED` by default, and explicit `GRAY1_PACKED` for faster lower-quality output.

`BINBOOK_FORMAT_SPEC.md` is the authoritative BinBook 0.1 candidate specification. The required `FONT_RESOURCE_INDEX` section records every font actually rasterized by reflow compilation; image-only books contain a valid empty section.

## Native CLI

Build the Rust executable from the workspace root:

```bash
cargo build -p binbook
```

Compile one PNG, JPEG, WebP, SVG, EPUB 2/3 file, or a non-recursive directory of mixed static images:

```bash
target/debug/binbook encode book.epub -o book.binbook
target/debug/binbook encode ./pages -o pages.binbook
target/debug/binbook encode cover.webp -o cover.binbook --pixel-format gray1 --no-dither
target/debug/binbook encode book.epub -o forced.binbook --font-family opendyslexic
```

`--input-format auto` uses path shape and file signatures. Explicit `image` and `epub` overrides reject mismatches. Directory names must be UTF-8, are sorted lexically, and unsupported or animated entries produce stderr warnings and are skipped; a single unsupported input or directory with no usable pages fails. Output is assembled and strictly validated in a sibling temporary file, then atomically renamed.

EPUB compilation supports package metadata, linear spine order, EPUB3 navigation, EPUB2 NCX fallback, fragments, common block/inline HTML, the documented CSS subset, and embedded TTF/OTF/WOFF/WOFF2 fonts. Referenced static artwork is rendered on standalone pages: portrait and square images use one page, landscape images use two uncropped left-to-right panels, and associated figure captions are retained. Unsupported CSS/content degrades deterministically with stable warnings. DRM-protected resources are rejected.

Inspect and decode through Rust:

```bash
target/debug/binbook inspect book.binbook --validate --strict
target/debug/binbook inspect book.binbook --validate --strict --json
target/debug/binbook decode book.binbook --page 0 -o page0.png
```

`inspect --json` writes JSON only to stdout. `decode` writes an 800×480 logical-content PNG for the stored X4 page and rejects out-of-range pages without leaving partial output. Device diagnostics remain under `binbook diag ...`; build them with `--features serial-device`.

Upload to an X4 with bounded streaming, acknowledged progress, CRC verification,
and library publication waiting:

```bash
target/debug/binbook upload --port "$PORT" --file book.binbook \
  --transfer-mode auto --window auto --progress auto \
  --transfer-report upload-report.json
```

Use `--progress always|auto|never`, `--json` for a machine-readable final
summary, and `--no-wait-library` when publication does not need to be observed.
Use `--transfer-mode legacy` for compatibility measurements; forced `bulk`
fails unless firmware advertises the capability. `--window auto|1|2|4|8`
requests a bound that firmware may clamp. Progress and report bytes advance
from validated durable acknowledgements, not serial writes.
On failure after an upload begins, the command attempts device-side abort and
reports whether cleanup was independently confirmed. Full verification can
stream the stored bytes back without buffering the book in host memory:

```bash
target/debug/binbook diag storage read --port "$PORT" \
  --path book.binbook --output device-readback.binbook \
  --transfer-mode auto --window auto --progress auto \
  --transfer-report readback-report.json
cmp book.binbook device-readback.binbook
```

## Python support tools

Python is retained only for the Pygame viewer and bundled-font kerning proof:

```bash
uv sync --dev
uv run binbook-support view book.binbook
uv run binbook-support kerning-proof --font-family opendyslexic --output-dir .tmp/kerning-proof
```

Viewer keys are right/down/space for next, left/up/backspace for previous, Home/End for first/last, and Esc or `q` to quit. Use `--static` for a shareable kerning proof; omit it to run the local approval server.

## Architecture and verification

The compiler graph is `binbook` → `binbook-compiler` → source (`binbook-epub`, `binbook-document`), rendering (`binbook-render`, `binbook-image`, `gray2-render`), and assembly (`binbook-encode`, `binbook-compress`, `binbook-core`) crates. Compiler libraries accept bytes and `Write + Seek`, contain no path/serial/firmware ownership, use supplied fonts only, and compile for `wasm32-unknown-unknown`.

Run the main gates:

```bash
cargo test --workspace
cargo test -p binbook --features serial-device
cargo test -p binbook-fw --features diagnostic-console
uv run pytest -q
uv run pytest -q tests/test_kerning_proof.py --run-proof
```

Firmware uses the pinned nightly command documented in `AGENTS.md`. Live-device verification follows `docs/reference/xteink-x4-agent-device-verification.md` and includes flash, serial state/log checks, and webcam inspection.

## Documentation

- [Format specification](BINBOOK_FORMAT_SPEC.md)
- [X4 e-reader library, cache, reading, refresh, state, and power design](docs/specs/2026-07-01-library-menu-reading-flow-design.md)
- [X4 e-reader POC implementation plan](docs/plans/2026-07-13-binbook-ereader-poc-plan.md)
- [Illustrated EPUB pagination and X4 device replacement plan](docs/plans/2026-07-14-illustrated-epub-pagination-and-device-load-plan.md)
- [EPUB artwork pagination design](docs/specs/2026-07-14-epub-artwork-pagination-design.md)
- [Rust crate architecture](docs/reference/rust-crate-architecture.md)
- [Compiler roadmap](docs/reference/compiler-roadmap.md)
- [Firmware flashing](docs/reference/xteink-x4-firmware-flashing.md)
- [Documentation index](docs/README.md)
