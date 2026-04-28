# BinBook POC

Python proof of concept for the BinBook compiled raster-book format.

The current POC supports:

- encoding a basic EPUB into `.binbook`
- encoding a folder of rendered PNG pages into `.binbook`
- decoding one `.binbook` page back to PNG
- inspecting the binary container structure
- viewing a `.binbook` in a Pygame desktop simulation viewer

The first supported profile is `xteink-x4-portrait`, stored as canonical row-major `GRAY2_PACKED` pages at `480x800`.

## Development

Use `uv` for local setup and test execution:

```bash
uv sync --dev
uv run pytest -q
```

Run the CLI through `uv`:

```bash
uv run binbook encode book.epub -o book.binbook
uv run binbook encode-png-folder ./pages -o test.binbook
uv run binbook inspect test.binbook --validate
uv run binbook inspect test.binbook --validate --json
uv run binbook inspect test.binbook --validate --strict
uv run binbook decode test.binbook --page 0 -o page0.png
uv run binbook view test.binbook
```

The desktop viewer uses Pygame for its window backend. Keyboard controls are right/down/space for next page, left/up/backspace for previous page, Home/End for first/last page, and Esc or `q` to quit.
