from __future__ import annotations

import argparse
from pathlib import Path
import sys

from .kerning_proof import generate_kerning_proof, serve_kerning_proof
from .viewer import launch_viewer


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="binbook-support")
    subparsers = parser.add_subparsers(dest="command", required=True)

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
        if args.command == "view":
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
                    static=True,
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
        print(f"binbook-support: error: {exc}", file=sys.stderr)
        return 1
    return 0
if __name__ == "__main__":
    raise SystemExit(main())
