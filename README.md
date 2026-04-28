# BinBook POC

Python proof of concept for the BinBook compiled raster-book format.

Milestone 1 supports:

- encoding a folder of rendered PNG pages into `.binbook`
- decoding one `.binbook` page back to PNG
- inspecting the binary container structure

The first supported profile is `xteink-x4-portrait`, stored as canonical row-major `GRAY2_PACKED` pages at `480x800`.

## Development

Use `uv` for local setup and test execution:

```bash
uv sync --dev
uv run pytest -q
```

Run the CLI through `uv`:

```bash
uv run binbook encode-png-folder ./pages -o test.binbook
uv run binbook inspect test.binbook --validate
uv run binbook decode test.binbook --page 0 -o page0.png
```
