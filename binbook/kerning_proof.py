from __future__ import annotations

from dataclasses import dataclass, replace
from datetime import datetime
import html
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import re
from statistics import mean
from typing import Any
from urllib.parse import unquote, urlparse

from PIL import Image, ImageChops, ImageDraw, ImageFont

from .fonts import FONT_KERNING_DIR, FontInfo, get_font, load_pair_kerning_table
from .render import TEXT_FEATURES, _character_spacing_px, _draw_text, _font, _wrap_text_to_width

DEFAULT_CANDIDATE_VALUES = (0, -40, -60, -80, -100, -120, -140, -160)
UPPER_TO_LOWER_PAIRS = ("To", "Th", "Ta", "Te", "Ty", "Yo", "Ye", "Ya", "Yu", "Wo", "Wa", "We", "Vo", "Va", "Ve")
UPPER_PAIRS = ("AV", "VA", "WA", "AW", "LT", "LA", "LY", "TA", "TY")
LOWER_PAIRS = ("yo", "oj", "ry", "ly", "vy", "wy", "fe", "rf", "ct")
PROOF_WORDS = ("You", "you", "Toast", "HAWAII", "Yale", "Yukon", "Water", "Av", "LT")
PAIR_CONTEXTS = {
    "Yo": (
        "Your young friend found a yellow book.",
    ),
    "yo": (
        "A young reader may enjoy your story.",
    ),
    "To": (
        "Today the town opened the tower to visitors.",
    ),
    "Th": (
        "The thick thread held the theorem together.",
    ),
    "AV": (
        "AV letters need to sit beside HAWAII and WATER.",
    ),
    "WA": (
        "Water washed away the warm sand.",
    ),
    "AW": (
        "A warm dawn awoke the whole town.",
    ),
    "LT": (
        "LT appears beside HALT, SALT, and WALT.",
    ),
}
FALLBACK_CONTEXTS = (
    "The quick reader studies every letter pair in context.",
)
HOLISTIC_CONTEXT = (
    "Today your young reader saw Yale, Yukon, water, Toast, HAWAII, "
    "a V-shaped valley, warm waves, clever type, and useful letters."
)
CONTROL_PAIRS = {
    "lowercase": ("nn", "oo", "no", "on"),
    "uppercase": ("HH", "HO", "OO"),
    "mixed_case": ("Ho", "He", "Ha"),
}


@dataclass(frozen=True)
class KerningProofResult:
    index_html: Path
    report_json: Path
    suggested_table: Path
    report: dict[str, Any]


@dataclass(frozen=True)
class KerningProofResponse:
    status: HTTPStatus
    headers: dict[str, str]
    body: bytes


def candidate_pairs(font_info: FontInfo) -> list[tuple[str, str]]:
    pairs: list[tuple[str, str]] = []
    for pair_text in UPPER_TO_LOWER_PAIRS + UPPER_PAIRS + LOWER_PAIRS:
        pairs.append((pair_text[0], pair_text[1]))
    for left, right in font_info.pair_kerning_milli_em:
        pairs.append((left, right))
    return _dedupe_pairs(pairs)


def candidate_values(current_value: int | None) -> list[int]:
    values = list(DEFAULT_CANDIDATE_VALUES)
    if current_value is not None and current_value not in values:
        values.append(current_value)
        values.sort(reverse=True)
    return values


def generate_kerning_proof(
    font_family: str,
    output_dir: Path,
    *,
    font_size: int = 72,
    static: bool = False,
    pair_kerning_milli_em: dict[tuple[str, str], int] | None = None,
) -> KerningProofResult:
    _log(f"Generating kerning proof for {font_family} at {output_dir}")
    font_info = get_font(font_family)
    if pair_kerning_milli_em is not None:
        font_info = replace(font_info, pair_kerning_milli_em=pair_kerning_milli_em)
    output_dir.mkdir(parents=True, exist_ok=True)
    assets_dir = output_dir / "assets"
    assets_dir.mkdir(exist_ok=True)

    font = _font(font_size, font_info)
    controls = _measure_controls(font, font_info)
    pairs = [
        _build_pair_report(pair, font, font_info, controls, assets_dir)
        for pair in candidate_pairs(font_info)
    ]
    holistic = _build_holistic_proof(font, font_info, assets_dir)
    report = {
        "font_family": font_info.family,
        "font_path": str(font_info.path),
        "font_size_px": font_size,
        "character_spacing_milli_em": font_info.default_character_spacing_milli_em,
        "existing_pair_kerning_milli_em": _serialize_pair_table(font_info.pair_kerning_milli_em),
        "proof_words": list(PROOF_WORDS),
        "source_notes": [
            "Typefacts / Bringhurst kerning-test text for broad Latin kerning coverage.",
            "Fontself control rhythms such as nn and HH for manual comparison.",
            "FontStruct proof words including HAWAII and Toast.",
            "FontLab common examples including Av, LT, and To.",
        ],
        "controls": controls,
        "holistic": holistic,
        "pairs": pairs,
    }

    report_json, suggested_table, index_html = _write_report_outputs(output_dir, font_info.family, report, static=static)
    _log(f"Generated kerning proof with {len(pairs)} pairs at {index_html}")
    return KerningProofResult(
        index_html=index_html,
        report_json=report_json,
        suggested_table=suggested_table,
        report=report,
    )


def canonical_kerning_path(font_family: str) -> Path:
    font_info = get_font(font_family)
    path = (FONT_KERNING_DIR / f"{font_info.family}.json").resolve()
    root = FONT_KERNING_DIR.resolve()
    if path.parent != root:
        raise ValueError(f"invalid font family for kerning table: {font_family}")
    return path


def save_canonical_kerning(
    font_family: str,
    pairs: dict[str, int | None],
    output_path: Path | None = None,
) -> dict[str, int]:
    get_font(font_family)
    canonical_pairs = _validate_canonical_pairs(pairs)
    path = output_path if output_path is not None else canonical_kerning_path(font_family)
    _log(f"Saving canonical kerning JSON for {font_family}: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(canonical_pairs, indent=2, sort_keys=True) + "\n")
    _log(f"Saved {len(canonical_pairs)} kerning pairs to {path}")
    return canonical_pairs


def serve_kerning_proof(
    font_family: str,
    output_dir: Path,
    *,
    font_size: int = 72,
    host: str = "127.0.0.1",
    port: int = 8765,
) -> None:
    proof = generate_kerning_proof(font_family, output_dir, font_size=font_size)
    handler_class = KerningProofRequestHandler.make_handler(font_family, output_dir, proof.report)
    try:
        server = ThreadingHTTPServer((host, port), handler_class)
    except OSError as exc:
        raise RuntimeError(f"could not start kerning proof server on {host}:{port}: {exc}") from exc
    print(f"Kerning proof server: http://{host}:{port}/")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


def _build_pair_report(
    pair: tuple[str, str],
    font: ImageFont.FreeTypeFont,
    font_info: FontInfo,
    controls: dict[str, Any],
    assets_dir: Path,
) -> dict[str, Any]:
    pair_text = "".join(pair)
    category = _pair_category(pair)
    target_gap = controls[category]["target_gap_px"]
    current_value = font_info.pair_kerning_milli_em.get(pair)
    values = candidate_values(current_value)
    candidates = [
        _build_candidate(pair, value, font, font_info, assets_dir)
        for value in values
    ]
    suggested = min(candidates, key=lambda item: abs(item["gap_px"] - target_gap))
    return {
        "pair": pair_text,
        "left": pair[0],
        "right": pair[1],
        "category": category,
        "current_value": current_value,
        "candidate_values": values,
        "target_gap_px": target_gap,
        "suggested_value": suggested["value"],
        "proof_words": [word for word in PROOF_WORDS if pair_text in word],
        "candidates": candidates,
    }


def _build_candidate(
    pair: tuple[str, str],
    value: int,
    font: ImageFont.FreeTypeFont,
    font_info: FontInfo,
    assets_dir: Path,
) -> dict[str, Any]:
    pair_text = "".join(pair)
    image = _render_pair_image(pair_text, font, font_info, {(pair[0], pair[1]): value})
    filename = f"{_pair_file_stem(pair_text)}_{value}.png"
    image.save(assets_dir / filename)
    contexts = _build_contexts(pair_text, value, font, font_info, assets_dir)
    gap = _measure_pair_gap(pair, font, font_info, value)
    return {
        "value": value,
        "gap_px": gap,
        "image": f"assets/{filename}",
        "contexts": contexts,
    }


def _measure_controls(font: ImageFont.FreeTypeFont, font_info: FontInfo) -> dict[str, Any]:
    controls: dict[str, Any] = {}
    for category, pairs in CONTROL_PAIRS.items():
        measurements = [
            {
                "pair": pair_text,
                "gap_px": _measure_pair_gap((pair_text[0], pair_text[1]), font, font_info, 0),
            }
            for pair_text in pairs
        ]
        controls[category] = {
            "pairs": list(pairs),
            "measurements": measurements,
            "target_gap_px": mean(item["gap_px"] for item in measurements),
        }
    return controls


def _measure_pair_gap(
    pair: tuple[str, str],
    font: ImageFont.FreeTypeFont,
    font_info: FontInfo,
    pair_value_milli_em: int,
) -> float:
    draw = ImageDraw.Draw(Image.new("L", (360, 180), 255))
    left_x = 80
    baseline_y = 44
    spacing_px = _character_spacing_px(font, font_info.default_character_spacing_milli_em)
    right_x = left_x + draw.textlength(pair[0], font=font, features=TEXT_FEATURES)
    right_x += spacing_px + _pair_value_px(font, pair_value_milli_em)

    left_bbox = _character_bbox(pair[0], font, left_x, baseline_y)
    right_bbox = _character_bbox(pair[1], font, right_x, baseline_y)
    if left_bbox is None or right_bbox is None:
        return 0.0
    return float(right_bbox[0] - left_bbox[2])


def _character_bbox(
    character: str,
    font: ImageFont.FreeTypeFont,
    x: float,
    y: int,
) -> tuple[int, int, int, int] | None:
    image = Image.new("L", (360, 180), 255)
    draw = ImageDraw.Draw(image)
    draw.text((x, y), character, fill=0, font=font, features=TEXT_FEATURES)
    return ImageChops.invert(image).getbbox()


def _render_pair_image(
    text: str,
    font: ImageFont.FreeTypeFont,
    font_info: FontInfo,
    pair_kerning_milli_em: dict[tuple[str, str], int],
) -> Image.Image:
    image = Image.new("L", (360, 150), 255)
    draw = ImageDraw.Draw(image)
    _draw_text(
        draw,
        (80, 38),
        text,
        font,
        font_info.default_character_spacing_milli_em,
        fill=0,
        pair_kerning_milli_em=pair_kerning_milli_em,
    )
    return image


def _build_contexts(
    pair_text: str,
    value: int,
    font: ImageFont.FreeTypeFont,
    font_info: FontInfo,
    assets_dir: Path,
) -> list[dict[str, str]]:
    contexts = []
    for index, text in enumerate(_context_texts(pair_text)):
        image = _render_context_image(text, pair_text, value, font, font_info)
        filename = f"{_pair_file_stem(pair_text)}_{value}_context_{index}.png"
        image.save(assets_dir / filename)
        contexts.append({"text": text, "image": f"assets/{filename}"})
    return contexts


def _build_holistic_proof(
    font: ImageFont.FreeTypeFont,
    font_info: FontInfo,
    assets_dir: Path,
    *,
    stale: bool = False,
    stale_pairs: list[str] | None = None,
) -> dict[str, Any]:
    image = _render_paragraph_image(HOLISTIC_CONTEXT, font, font_info, dict(font_info.pair_kerning_milli_em))
    filename = "holistic.png"
    image.save(assets_dir / filename)
    return {
        "text": HOLISTIC_CONTEXT,
        "image": f"assets/{filename}",
        "stale": stale,
        "stale_pairs": stale_pairs or [],
    }


def _context_texts(pair_text: str) -> tuple[str, ...]:
    if pair_text in PAIR_CONTEXTS:
        return PAIR_CONTEXTS[pair_text]
    proof_matches = tuple(word for word in PROOF_WORDS if pair_text in word)
    if proof_matches:
        proof_text = " ".join(proof_matches)
        return (
            f"{proof_text} appears alongside ordinary English words.",
        )
    return FALLBACK_CONTEXTS


def _render_context_image(
    text: str,
    pair_text: str,
    value: int,
    font: ImageFont.FreeTypeFont,
    font_info: FontInfo,
    base_pair_kerning_milli_em: dict[tuple[str, str], int] | None = None,
) -> Image.Image:
    pair_kerning = (
        dict(font_info.pair_kerning_milli_em)
        if base_pair_kerning_milli_em is None
        else dict(base_pair_kerning_milli_em)
    )
    pair_kerning[(pair_text[0], pair_text[1])] = value

    return _render_paragraph_image(text, font, font_info, pair_kerning)


def _render_paragraph_image(
    text: str,
    font: ImageFont.FreeTypeFont,
    font_info: FontInfo,
    pair_kerning_milli_em: dict[tuple[str, str], int],
) -> Image.Image:
    width = 760
    x = 24
    y = 26
    context_font = _font(_context_font_size(font), font_info)
    line_height = int(round(context_font.size * 1.35))
    measurement_image = Image.new("L", (width, 120), 255)
    measurement_draw = ImageDraw.Draw(measurement_image)
    lines = _wrap_text_to_width(
        text,
        measurement_draw,
        context_font,
        width - (x * 2),
        font_info.default_character_spacing_milli_em,
        pair_kerning_milli_em,
    ) or [""]

    image = Image.new("L", (width, y * 2 + line_height * len(lines)), 255)
    draw = ImageDraw.Draw(image)
    for index, line in enumerate(lines):
        _draw_text(
            draw,
            (x, y + line_height * index),
            line,
            context_font,
            font_info.default_character_spacing_milli_em,
            fill=0,
            pair_kerning_milli_em=pair_kerning_milli_em,
        )
    return image


def _context_font_size(font: ImageFont.FreeTypeFont) -> int:
    return max(28, min(40, int(round(font.size * 0.56))))


def _validate_canonical_pairs(pairs: dict[str, int | None]) -> dict[str, int]:
    canonical: dict[str, int] = {}
    for pair, value in pairs.items():
        if not isinstance(pair, str) or len(pair) != 2:
            raise ValueError("kerning pair keys must be two-character strings")
        if value is None or value == 0:
            continue
        if not isinstance(value, int):
            raise ValueError("kerning pair values must be integers")
        canonical[pair] = value
    return dict(sorted(canonical.items()))


def _json_bytes(payload: object) -> bytes:
    return (json.dumps(payload, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _response(status: HTTPStatus, body: object, content_type: str) -> KerningProofResponse:
    if isinstance(body, bytes):
        response_body = body
    elif isinstance(body, str):
        response_body = body.encode("utf-8")
    else:
        response_body = _json_bytes(body)
    return KerningProofResponse(
        status=status,
        headers={"Content-Type": content_type},
        body=response_body,
    )


def _write_report_outputs(
    output_dir: Path,
    font_family: str,
    report: dict[str, Any],
    *,
    static: bool = False,
) -> tuple[Path, Path, Path]:
    report_json = output_dir / "report.json"
    report_json.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    suggested_table = output_dir / "approved_table.py.txt"
    suggested_table.write_text(_table_text(font_family, report["pairs"], use_suggestions=True))
    index_html = output_dir / "index.html"
    index_html.write_text(_index_html(report, static=static))
    return report_json, suggested_table, index_html


def _changed_pair_keys(previous: dict[str, int], saved: dict[str, int]) -> list[str]:
    return sorted(pair for pair in set(previous) | set(saved) if previous.get(pair) != saved.get(pair))


def _pair_table_from_serialized(pair_table: object) -> dict[tuple[str, str], int]:
    table: dict[tuple[str, str], int] = {}
    for pair, value in dict(pair_table).items():
        if not isinstance(pair, str) or len(pair) != 2:
            raise ValueError("kerning pair keys must be two-character strings")
        if not isinstance(value, int):
            raise ValueError("kerning pair values must be integers")
        table[(pair[0], pair[1])] = value
    return table


class KerningProofRequestHandler(BaseHTTPRequestHandler):
    font_family: str
    output_dir: Path
    report: dict[str, Any]
    canonical_path: Path | None = None

    @classmethod
    def make_handler(
        cls,
        font_family: str,
        output_dir: Path,
        report: dict[str, Any],
        *,
        canonical_path: Path | None = None,
    ) -> type[KerningProofRequestHandler]:
        class ConfiguredKerningProofRequestHandler(cls):
            pass

        ConfiguredKerningProofRequestHandler.font_family = font_family
        ConfiguredKerningProofRequestHandler.output_dir = output_dir
        ConfiguredKerningProofRequestHandler.report = report
        ConfiguredKerningProofRequestHandler.canonical_path = canonical_path
        return ConfiguredKerningProofRequestHandler

    @classmethod
    def create_test_handler(
        cls,
        font_family: str,
        output_dir: Path,
        report: dict[str, Any],
        *,
        canonical_path: Path | None = None,
    ) -> type[KerningProofRequestHandler]:
        return cls.make_handler(
            font_family,
            output_dir,
            report,
            canonical_path=canonical_path,
        )

    def do_GET(self) -> None:
        self._send_response(self.handle_get(self.path))

    def do_POST(self) -> None:
        content_length = int(self.headers.get("Content-Length", "0"))
        self._send_response(self.handle_post(self.path, self.rfile.read(content_length)))

    @classmethod
    def handle_get(cls, raw_path: str) -> KerningProofResponse:
        path = urlparse(raw_path).path
        if path == "/":
            return _response(HTTPStatus.OK, _index_html(cls.report), "text/html; charset=utf-8")
        if path == "/report.json":
            return _response(HTTPStatus.OK, _json_bytes(cls.report), "application/json")
        if path == "/api/kerning":
            kerning_path = cls.canonical_path if cls.canonical_path is not None else canonical_kerning_path(cls.font_family)
            return _response(
                HTTPStatus.OK,
                _json_bytes(
                    {
                        "font_family": cls.font_family,
                        "pairs": _serialize_pair_table(load_pair_kerning_table(kerning_path)),
                    }
                ),
                "application/json",
            )
        if path.startswith("/assets/"):
            return cls._asset_response(path)
        return _response(HTTPStatus.NOT_FOUND, {"error": "not found"}, "application/json")

    @classmethod
    def handle_post(cls, raw_path: str, body: bytes) -> KerningProofResponse:
        path = urlparse(raw_path).path
        if path == "/api/holistic":
            return cls._handle_holistic_post(body)
        if path != "/api/kerning":
            return _response(HTTPStatus.NOT_FOUND, {"error": "not found"}, "application/json")
        try:
            payload = json.loads(body.decode("utf-8"))
            if not isinstance(payload, dict):
                raise ValueError("request body must be a JSON object")
            if payload.get("font_family") != cls.font_family:
                raise ValueError("font_family does not match this proof server")
            pairs = payload.get("pairs")
            if not isinstance(pairs, dict):
                raise ValueError("pairs must be an object")
            previous = dict(cls.report.get("existing_pair_kerning_milli_em", {}))
            saved = save_canonical_kerning(cls.font_family, pairs, cls.canonical_path)
            changed_pairs = _changed_pair_keys(previous, saved)
            if changed_pairs:
                _log(f"Regenerating {len(changed_pairs)} changed pair proofs: {', '.join(changed_pairs)}")
                cls._regenerate_pair_reports(saved, changed_pairs)
                cls.report["holistic"] = {
                    **cls.report["holistic"],
                    "stale": True,
                    "stale_pairs": changed_pairs,
                }
                _log(f"Holistic proof marked stale for {len(changed_pairs)} changed pairs")
            else:
                _log("No changed pair proofs to regenerate")
            _write_report_outputs(cls.output_dir, cls.font_family, cls.report)
        except (UnicodeDecodeError, json.JSONDecodeError, ValueError) as exc:
            return _response(HTTPStatus.BAD_REQUEST, {"error": str(exc)}, "application/json")
        return _response(
            HTTPStatus.OK,
            {
                "font_family": cls.font_family,
                "pairs": saved,
                "regenerated_pairs": changed_pairs,
                "holistic_stale": bool(changed_pairs),
                "report": cls.report,
            },
            "application/json",
        )

    @classmethod
    def _handle_holistic_post(cls, body: bytes) -> KerningProofResponse:
        try:
            payload = json.loads(body.decode("utf-8"))
            if not isinstance(payload, dict):
                raise ValueError("request body must be a JSON object")
            if payload.get("font_family") != cls.font_family:
                raise ValueError("font_family does not match this proof server")
            pair_table = _pair_table_from_serialized(cls.report.get("existing_pair_kerning_milli_em", {}))
            font_info = replace(get_font(cls.font_family), pair_kerning_milli_em=pair_table)
            font = _font(int(cls.report.get("font_size_px", 72)), font_info)
            assets_dir = cls.output_dir / "assets"
            _log(f"Regenerating holistic proof for {cls.font_family}")
            cls.report["holistic"] = _build_holistic_proof(font, font_info, assets_dir)
            _write_report_outputs(cls.output_dir, cls.font_family, cls.report)
            _log(f"Regenerated holistic proof for {cls.font_family}")
        except (UnicodeDecodeError, json.JSONDecodeError, ValueError) as exc:
            return _response(HTTPStatus.BAD_REQUEST, {"error": str(exc)}, "application/json")
        return _response(
            HTTPStatus.OK,
            {"font_family": cls.font_family, "regenerated": "holistic", "report": cls.report},
            "application/json",
        )

    @classmethod
    def _regenerate_pair_reports(cls, saved: dict[str, int], changed_pairs: list[str]) -> None:
        pair_table = _pair_table_from_serialized(saved)
        font_info = replace(get_font(cls.font_family), pair_kerning_milli_em=pair_table)
        font = _font(int(cls.report.get("font_size_px", 72)), font_info)
        assets_dir = cls.output_dir / "assets"
        existing_pairs = {entry["pair"]: entry for entry in cls.report["pairs"]}
        controls = cls.report["controls"]
        for pair_text in changed_pairs:
            existing_pairs[pair_text] = _build_pair_report((pair_text[0], pair_text[1]), font, font_info, controls, assets_dir)
        cls.report["pairs"] = [
            existing_pairs.get(entry["pair"], entry)
            for entry in cls.report["pairs"]
        ]
        existing_order = {entry["pair"] for entry in cls.report["pairs"]}
        cls.report["pairs"].extend(existing_pairs[pair_text] for pair_text in changed_pairs if pair_text not in existing_order)
        cls.report["existing_pair_kerning_milli_em"] = _serialize_pair_table(pair_table)

    @classmethod
    def _asset_response(cls, path: str) -> KerningProofResponse:
        asset_name = unquote(path.removeprefix("/assets/"))
        if "/" in asset_name or "\\" in asset_name or asset_name in {"", ".", ".."}:
            return _response(HTTPStatus.BAD_REQUEST, {"error": "invalid asset path"}, "application/json")
        asset_path = (cls.output_dir / "assets" / asset_name).resolve()
        asset_root = (cls.output_dir / "assets").resolve()
        if asset_path.parent != asset_root or not asset_path.exists():
            return _response(HTTPStatus.NOT_FOUND, {"error": "asset not found"}, "application/json")
        return KerningProofResponse(
            status=HTTPStatus.OK,
            headers={"Content-Type": "image/png"},
            body=asset_path.read_bytes(),
        )

    def _send_response(self, response: KerningProofResponse) -> None:
        self.send_response(response.status)
        for key, value in response.headers.items():
            self.send_header(key, value)
        self.send_header("Content-Length", str(len(response.body)))
        self.end_headers()
        self.wfile.write(response.body)

    def log_message(self, format: str, *args: object) -> None:
        return


def _index_html(report: dict[str, Any], *, static: bool = False) -> str:
    data = json.dumps(report, sort_keys=True).replace("</", "<\\/")
    save_button = "" if static else '<button id="save">Save Canonical JSON</button>'
    holistic_button = "" if static else '<button id="regenerate-holistic" hidden>Regenerate Holistic</button>'
    static_note = (
        '<span id="save-status">Static export: run without --static to save canonical JSON from the browser.</span>'
        if static
        else '<span id="save-status"></span>'
    )
    save_script = (
        ""
        if static
        else """
    document.getElementById('save').addEventListener('click', async () => {
      const status = document.getElementById('save-status');
      const save = document.getElementById('save');
      const activePair = active.pair;
      save.disabled = true;
      status.textContent = 'Saving and regenerating changed pair proofs...';
      try {
        const response = await fetch('/api/kerning', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ font_family: report.font_family, pairs: approvedPairs() })
        });
        const payload = await response.json();
        if (!response.ok) {
          status.textContent = `Save failed: ${payload.error ?? response.statusText}`;
          return;
        }
        report = payload.report;
        approvals = new Map(report.pairs.map(pair => [pair.pair, pair.current_value ?? pair.suggested_value]));
        active = report.pairs.find(pair => pair.pair === activePair) ?? report.pairs[0];
        assetVersion = Date.now();
        render();
        status.textContent = `Saved and regenerated proof. Regenerated ${{payload.regenerated_pairs.length}} changed pair proof(s). Holistic proof is stale.`;
      } finally {
        save.disabled = false;
      }
    });
    document.getElementById('regenerate-holistic').addEventListener('click', async () => {
      const status = document.getElementById('save-status');
      const button = document.getElementById('regenerate-holistic');
      button.disabled = true;
      status.textContent = 'Regenerating holistic proof...';
      try {
        const response = await fetch('/api/holistic', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ font_family: report.font_family })
        });
        const payload = await response.json();
        if (!response.ok) {
          status.textContent = `Holistic regeneration failed: ${payload.error ?? response.statusText}`;
          return;
        }
        report = payload.report;
        approvals = new Map(report.pairs.map(pair => [pair.pair, pair.current_value ?? pair.suggested_value]));
        active = HOLISTIC_VIEW;
        assetVersion = Date.now();
        render();
        status.textContent = 'Regenerated holistic proof.';
      } finally {
        button.disabled = false;
      }
    });"""
    )
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>BinBook Kerning Proof - {html.escape(report["font_family"])}</title>
  <style>
    :root {{ color-scheme: light; font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }}
    body {{ margin: 0; background: #f5f5f2; color: #1f2328; }}
    header {{ padding: 18px 24px 14px; background: #242622; color: #f8f8f2; }}
    header h1 {{ margin: 0 0 6px; font-size: 20px; font-weight: 650; }}
    header p {{ margin: 0; color: #d7d8d2; font-size: 13px; }}
    main {{ display: grid; grid-template-columns: 280px 1fr; gap: 0; min-height: calc(100vh - 75px); }}
    nav {{ border-right: 1px solid #d2d2ca; background: #fff; padding: 12px; overflow: auto; }}
    button, select {{ font: inherit; }}
    .pair-button {{ width: 100%; display: flex; justify-content: space-between; align-items: center; padding: 8px 10px; border: 1px solid transparent; background: transparent; border-radius: 6px; cursor: pointer; }}
    .pair-button[aria-current="true"] {{ border-color: #6a7a4f; background: #eef2e6; }}
    .pair-text {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 17px; }}
    .pair-meta {{ color: #677064; font-size: 12px; }}
    section {{ padding: 18px 22px 30px; overflow: auto; }}
    .toolbar {{ display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin: 12px 0 18px; }}
    .toolbar button {{ border: 1px solid #bfc4b7; background: #fff; padding: 7px 10px; border-radius: 6px; cursor: pointer; }}
    .candidate-grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 12px; }}
    .candidate {{ border: 1px solid #d1d1c7; border-radius: 8px; background: #fff; padding: 10px; cursor: pointer; }}
    .candidate.selected {{ outline: 2px solid #536c35; border-color: #536c35; }}
    .candidate img {{ display: block; width: 100%; height: auto; image-rendering: pixelated; border: 1px solid #eee; background: #fff; }}
    .candidate-info {{ display: flex; justify-content: space-between; margin-top: 8px; font-size: 13px; }}
    .candidate-contexts {{ margin-top: 18px; display: grid; gap: 12px; }}
    .context-card {{ border: 1px solid #d1d1c7; border-radius: 8px; background: #fff; padding: 10px; }}
    .context-card img {{ display: block; width: 100%; height: auto; image-rendering: pixelated; border: 1px solid #eee; background: #fff; }}
    .context-text {{ margin-top: 7px; color: #555d52; font-size: 13px; }}
    .context-heading {{ margin: 0 0 8px; font-size: 14px; color: #29311f; }}
    .badge {{ border-radius: 999px; background: #e7eadf; color: #3d4c2c; padding: 2px 7px; font-size: 11px; margin-left: 5px; }}
    .status-stale {{ color: #8a4b00; font-weight: 650; }}
    textarea {{ width: 100%; min-height: 220px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; }}
    .export-row {{ margin-top: 18px; }}
    @media (max-width: 760px) {{ main {{ grid-template-columns: 1fr; }} nav {{ border-right: 0; border-bottom: 1px solid #d2d2ca; max-height: 220px; }} }}
  </style>
</head>
<body>
  <header>
    <h1>BinBook Kerning Proof - {html.escape(report["font_family"])}</h1>
    <p>Suggestions compare raster gaps against control rhythms. Visual approval is authoritative.</p>
  </header>
  <main>
    <nav id="pair-list"></nav>
    <section>
      <h2 id="pair-title"></h2>
      <p id="pair-summary"></p>
      <div id="toolbar" class="toolbar">
        <button id="approve">Approve Suggested</button>
        <button id="none">No Override</button>
        <label>Approved value <select id="approved-value"></select></label>
        {save_button}
        {holistic_button}
        {static_note}
      </div>
      <div id="candidates" class="candidate-grid"></div>
      <div id="holistic-context" class="candidate-contexts"></div>
      <div id="candidate-contexts" class="candidate-contexts"></div>
    </section>
  </main>
  <script id="report-data" type="application/json">{data}</script>
  <script>
    const HOLISTIC_VIEW = '__holistic__';
    let report = JSON.parse(document.getElementById('report-data').textContent);
    let approvals = new Map(report.pairs.map(pair => [pair.pair, pair.current_value ?? pair.suggested_value]));
    let assetVersion = Date.now();
    let active = report.pairs[0];

    function assetSrc(path) {{
      return `${{path}}?v=${{assetVersion}}`;
    }}

    function renderList() {{
      const list = document.getElementById('pair-list');
      list.innerHTML = '';
      const holisticButton = document.createElement('button');
      holisticButton.className = 'pair-button';
      holisticButton.setAttribute('aria-current', active === HOLISTIC_VIEW ? 'true' : 'false');
      holisticButton.innerHTML = `<span class="pair-text">Holistic</span><span class="pair-meta">${{report.holistic.stale ? 'stale' : 'fresh'}}</span>`;
      holisticButton.addEventListener('click', () => {{ active = HOLISTIC_VIEW; render(); }});
      list.appendChild(holisticButton);
      for (const pair of report.pairs) {{
        const button = document.createElement('button');
        button.className = 'pair-button';
        button.setAttribute('aria-current', pair.pair === active.pair ? 'true' : 'false');
        button.innerHTML = `<span class="pair-text">${{pair.pair}}</span><span class="pair-meta">${{approvals.get(pair.pair) ?? 'none'}}</span>`;
        button.addEventListener('click', () => {{ active = pair; render(); }});
        list.appendChild(button);
      }}
    }}

    function renderCandidates() {{
      const container = document.getElementById('candidates');
      container.innerHTML = '';
      if (active === HOLISTIC_VIEW) return;
      for (const candidate of active.candidates) {{
        const card = document.createElement('button');
        card.className = 'candidate' + (approvals.get(active.pair) === candidate.value ? ' selected' : '');
        const badges = [
          candidate.value === active.suggested_value ? '<span class="badge">suggested</span>' : '',
          candidate.value === active.current_value ? '<span class="badge">current</span>' : ''
        ].join('');
        card.innerHTML = `<img src="${{assetSrc(candidate.image)}}" alt="${{active.pair}} at ${{candidate.value}} milli-em"><div class="candidate-info"><span>${{candidate.value}} milli-em ${{badges}}</span><span>gap ${{candidate.gap_px.toFixed(1)}}px</span></div>`;
        card.addEventListener('click', () => {{ approvals.set(active.pair, candidate.value); render(); }});
        container.appendChild(card);
      }}
    }}

    function renderContexts() {{
      const container = document.getElementById('candidate-contexts');
      const holistic = document.getElementById('holistic-context');
      container.innerHTML = '';
      holistic.innerHTML = '';
      if (active === HOLISTIC_VIEW) {{
        const card = document.createElement('div');
        card.className = 'context-card';
        const stale = report.holistic.stale ? `<p class="status-stale">Holistic proof is stale. Regenerate to review with latest saved kerning for ${{report.holistic.stale_pairs.join(', ')}}.</p>` : '';
        card.innerHTML = `<h3 class="context-heading">Holistic paragraph</h3>${{stale}}<img src="${{assetSrc(report.holistic.image)}}" alt="${{report.holistic.text}}"><div class="context-text">${{report.holistic.text}}</div>`;
        holistic.appendChild(card);
        return;
      }}
      const selectedValue = approvals.get(active.pair);
      const selected = active.candidates.find(candidate => candidate.value === selectedValue) ?? active.candidates[0];
      for (const context of selected.contexts) {{
        const card = document.createElement('div');
        card.className = 'context-card';
        card.innerHTML = `<img src="${{assetSrc(context.image)}}" alt="${{context.text}}"><div class="context-text">${{context.text}}</div>`;
        container.appendChild(card);
      }}
    }}

    function approvedPairs() {{
      const entries = {{}};
      for (const pair of report.pairs) {{
        const value = approvals.get(pair.pair);
        if (value === null || value === undefined || value === 0) continue;
        entries[pair.pair] = value;
      }}
      return entries;
    }}

    function renderSelect() {{
      const select = document.getElementById('approved-value');
      select.innerHTML = '<option value="">No override</option>';
      if (active === HOLISTIC_VIEW) return;
      for (const value of active.candidate_values) {{
        const option = document.createElement('option');
        option.value = String(value);
        option.textContent = `${{value}} milli-em`;
        select.appendChild(option);
      }}
      const value = approvals.get(active.pair);
      select.value = value === null || value === undefined ? '' : String(value);
    }}

    function render() {{
      renderList();
      const pairControls = [document.getElementById('approve'), document.getElementById('none'), document.getElementById('approved-value')];
      for (const control of pairControls) control.disabled = active === HOLISTIC_VIEW;
      const save = document.getElementById('save');
      if (save) save.hidden = active === HOLISTIC_VIEW;
      const regenerateHolistic = document.getElementById('regenerate-holistic');
      if (regenerateHolistic) regenerateHolistic.hidden = active !== HOLISTIC_VIEW || !report.holistic.stale;
      if (active === HOLISTIC_VIEW) {{
        document.getElementById('pair-title').textContent = 'Holistic proof';
        document.getElementById('pair-summary').textContent = report.holistic.stale ? `Holistic proof is stale after changes to ${{report.holistic.stale_pairs.join(', ')}}.` : 'Holistic proof reflects the current saved kerning table.';
      }} else {{
        document.getElementById('pair-title').textContent = `${{active.pair}}`;
        document.getElementById('pair-summary').textContent = `Control target ${{active.target_gap_px.toFixed(1)}}px, current ${{active.current_value ?? 'none'}}, suggested ${{active.suggested_value}}.`;
      }}
      renderSelect();
      renderCandidates();
      renderContexts();
    }}

    document.getElementById('approve').addEventListener('click', () => {{ approvals.set(active.pair, active.suggested_value); render(); }});
    document.getElementById('none').addEventListener('click', () => {{ approvals.set(active.pair, null); render(); }});
    document.getElementById('approved-value').addEventListener('change', event => {{
      approvals.set(active.pair, event.target.value === '' ? null : Number(event.target.value));
      render();
    }});
    {save_script}
    render();
  </script>
</body>
</html>
"""


def _table_text(font_family: str, pairs: list[dict[str, Any]], *, use_suggestions: bool) -> str:
    name = re.sub(r"[^A-Z0-9]+", "_", font_family.upper())
    lines = [f"# Generated by binbook kerning-proof for {font_family}", f"{name}_PAIR_KERNING_MILLI_EM = {{"]
    key = "suggested_value" if use_suggestions else "current_value"
    for pair in pairs:
        value = pair[key]
        if value is None or value == 0:
            continue
        lines.append(f"    ({pair['left']!r}, {pair['right']!r}): {value},")
    lines.append("}")
    return "\n".join(lines) + "\n"


def _log(message: str) -> None:
    timestamp = datetime.now().strftime("%H:%M:%S")
    print(f"[binbook kerning-proof {timestamp}] {message}", flush=True)


def _dedupe_pairs(pairs: list[tuple[str, str]]) -> list[tuple[str, str]]:
    seen: set[tuple[str, str]] = set()
    deduped = []
    for pair in pairs:
        if pair in seen:
            continue
        seen.add(pair)
        deduped.append(pair)
    return deduped


def _pair_category(pair: tuple[str, str]) -> str:
    if pair[0].isupper() and pair[1].isupper():
        return "uppercase"
    if pair[0].islower() and pair[1].islower():
        return "lowercase"
    return "mixed_case"


def _pair_value_px(font: ImageFont.FreeTypeFont, value: int) -> float:
    return getattr(font, "size", 24) * (value / 1000)


def _pair_file_stem(pair_text: str) -> str:
    return "_".join(f"u{ord(character):04x}" for character in pair_text)


def _serialize_pair_table(pair_table: object) -> dict[str, int]:
    return {
        "".join(pair): value
        for pair, value in dict(pair_table).items()
    }
