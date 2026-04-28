from __future__ import annotations

import argparse
from pathlib import Path
import sys

from .inspect import inspect_book
from .reader import BinBookReader
from .structs import HEADER_SIZE, BinBookHeader
from .viewer import launch_viewer
from .writer import encode_png_folder


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="binbook")
    subparsers = parser.add_subparsers(dest="command", required=True)

    encode_png = subparsers.add_parser("encode-png-folder", help="encode a folder of page PNGs")
    encode_png.add_argument("input_dir", type=Path)
    encode_png.add_argument("-o", "--output", required=True, type=Path)
    encode_png.add_argument("--profile", default="xteink-x4-portrait")

    decode = subparsers.add_parser("decode", help="decode one page to PNG")
    decode.add_argument("input", type=Path)
    decode.add_argument("--page", required=True, type=int)
    decode.add_argument("-o", "--output", required=True, type=Path)

    inspect = subparsers.add_parser("inspect", help="inspect a BinBook file")
    inspect.add_argument("input", type=Path)
    inspect.add_argument("--validate", action="store_true")
    inspect.add_argument("--strict", action="store_true", help="report all validation errors detected by inspect")
    inspect.add_argument("--json", action="store_true", help="emit JSON inspection output")

    view = subparsers.add_parser(
        "view",
        help="simulate a BinBook file in a desktop viewer",
        description="simulate a BinBook file in a desktop viewer",
    )
    view.add_argument("input", type=Path)
    view.add_argument("--page", type=int, default=0)
    view.add_argument("--no-chrome", action="store_true")
    view.add_argument("--debug-content-box", action="store_true")

    args = parser.parse_args(argv)
    try:
        if args.command == "encode-png-folder":
            encode_png_folder(args.input_dir, args.output, args.profile)
        elif args.command == "decode":
            BinBookReader.open(args.input).decode_page_to_png(args.page, args.output)
        elif args.command == "inspect":
            reader = _open_for_inspect(args.input, strict=args.strict)
            result = inspect_book(reader, args.validate, json_output=args.json, strict=args.strict)
            print(result.json_text if args.json else result.text)
            if args.validate and not result.ok:
                return 1
        elif args.command == "view":
            launch_viewer(
                args.input,
                page=args.page,
                show_chrome=not args.no_chrome,
                debug_content_box=args.debug_content_box,
            )
        else:
            parser.error("unknown command")
    except Exception as exc:
        print(f"binbook: error: {exc}", file=sys.stderr)
        return 1
    return 0


def _open_for_inspect(path: Path, *, strict: bool) -> BinBookReader:
    if not strict:
        return BinBookReader.open(path)
    data = path.read_bytes()
    header = BinBookHeader.unpack(data[:HEADER_SIZE])
    from .reader import _read_pages, _read_sections

    sections = _read_sections(data, header)
    pages = _read_pages(data, sections)
    return BinBookReader(path, data, header, sections, pages)


if __name__ == "__main__":
    raise SystemExit(main())
