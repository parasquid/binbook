# BinBook

Python reference implementation for the BinBook compiled raster-book format.

The current implementation supports:

- encoding a basic EPUB into `.binbook`
- encoding a folder of rendered PNG pages into `.binbook`
- decoding one `.binbook` page back to PNG
- inspecting the binary container structure
- viewing a `.binbook` in a Pygame desktop simulation viewer
- generating a local font kerning proof for bundled fonts

The first supported profile is `xteink-x4-portrait`, stored by default as canonical row-major `GRAY2_PACKED` pages at logical `480x800`, with physical panel metadata `800x480` and logical-to-physical rotation `90` degrees clockwise. `GRAY1_PACKED` output is available as an explicit fast/lower-quality option.
The default bundled reading font is Literata, licensed under the SIL Open Font License.

## Development

Use `uv` for local setup and test execution:

```bash
uv sync --dev
uv run pytest -q
```

Run the CLI through `uv`:

```bash
uv run binbook encode book.epub -o book.binbook --font-family sans-serif
uv run binbook encode-png-folder ./pages -o test.binbook
uv run binbook encode-png-folder ./pages -o test-gray1.binbook --pixel-format gray1
uv run binbook inspect test.binbook --validate
uv run binbook inspect test.binbook --validate --json
uv run binbook inspect test.binbook --validate --strict
uv run binbook decode test.binbook --page 0 -o page0.png
uv run binbook view test.binbook
```

The desktop viewer uses Pygame for its window backend. Keyboard controls are right/down/space for next page, left/up/backspace for previous page, Home/End for first/last page, and Esc or `q` to quit.

Bundled font families include `sans-serif`/`opendyslexic` and `literata`. The `sans-serif` family uses OpenDyslexic.

## Kerning Proof

Generate and open an interactive proof for a bundled font:

```bash
uv run binbook kerning-proof --font-family opendyslexic --output-dir .tmp/kerning-proof
```

The command writes `index.html`, `report.json`, `approved_table.py.txt`, and PNG proof assets under the output directory, then serves the proof at `http://127.0.0.1:8765/`. Use the browser UI to compare candidate pair values, approve overrides, and save canonical JSON back to `binbook/font_kerning/<font-family>.json`. After saving, the server regenerates only the changed pair proofs and refreshes the browser report so the saved table becomes the current baseline.

The holistic paragraph is a separate proof view. Saving pair changes marks the holistic proof stale, because the paragraph may no longer reflect the latest saved table. Use `Regenerate Holistic` in that view to rebuild it when you are ready for an end-to-end paragraph check.

Use `--static` when you only want a shareable HTML/asset export:

```bash
uv run binbook kerning-proof --static --font-family opendyslexic --output-dir .tmp/kerning-proof
```

Static exports do not run the save API, so approval choices in the browser are temporary. Run without `--static` when you want the browser to write the canonical kerning JSON.

Kerning proof generation is intentionally slower than the main test suite. Run its tests explicitly with:

```bash
uv run pytest -q tests/test_kerning_proof.py --run-proof
```
