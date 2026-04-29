from __future__ import annotations

import argparse
from pathlib import Path
import sys

from .inspect import inspect_book
from .kerning_proof import generate_kerning_proof, serve_kerning_proof
from .reader import BinBookReader
from .render import encode_epub
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
    encode_png.add_argument("--pixel-format", choices=("gray2", "gray1"), default=None)

    encode = subparsers.add_parser("encode", help="encode an EPUB into a BinBook file")
    encode.add_argument("input_epub", type=Path)
    encode.add_argument("-o", "--output", required=True, type=Path)
    encode.add_argument("--profile", default="xteink-x4-portrait")
    encode.add_argument("--font-family", default="literata")
    encode.add_argument("--pixel-format", choices=("gray2", "gray1"), default=None)

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

    kerning_proof = subparsers.add_parser(
        "kerning-proof",
        help="generate an interactive font kerning proof",
        description="generate an interactive static HTML proof for tuning font-specific pair kerning",
    )
    kerning_proof.add_argument("--font-family", default="opendyslexic")
    kerning_proof.add_argument("--output-dir", required=True, type=Path)
    kerning_proof.add_argument("--font-size", type=int, default=72)
    kerning_proof.add_argument("--port", type=int, default=8765)
    kerning_proof.add_argument("--static", action="store_true")

    args = parser.parse_args(argv)
    try:
        if args.command == "encode-png-folder":
            encode_png_folder(args.input_dir, args.output, args.profile, args.pixel_format)
        elif args.command == "encode":
            encode_epub(args.input_epub, args.output, args.profile, args.font_family, args.pixel_format)
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
        elif args.command == "kerning-proof":
            if args.static:
                result = generate_kerning_proof(
                    args.font_family,
                    args.output_dir,
                    font_size=args.font_size,
                )
                print(f"Wrote kerning proof: {result.index_html}")
            else:
                serve_kerning_proof(
                    args.font_family,
                    args.output_dir,
                    font_size=args.font_size,
                    port=args.port,
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
