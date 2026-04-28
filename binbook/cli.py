from __future__ import annotations

import argparse
from pathlib import Path
import sys

from .inspect import inspect_book
from .reader import BinBookReader
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

    args = parser.parse_args(argv)
    try:
        if args.command == "encode-png-folder":
            encode_png_folder(args.input_dir, args.output, args.profile)
        elif args.command == "decode":
            BinBookReader.open(args.input).decode_page_to_png(args.page, args.output)
        elif args.command == "inspect":
            print(inspect_book(BinBookReader.open(args.input), args.validate))
        else:
            parser.error("unknown command")
    except Exception as exc:
        print(f"binbook: error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
