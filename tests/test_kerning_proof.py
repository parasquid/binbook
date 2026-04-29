from __future__ import annotations

import json
from http import HTTPStatus
from pathlib import Path
from unittest import mock

import pytest
from PIL import Image, ImageChops

from binbook.cli import main
from binbook.fonts import get_font, load_pair_kerning_table
from binbook.kerning_proof import (
    DEFAULT_CANDIDATE_VALUES,
    KerningProofRequestHandler,
    candidate_pairs,
    candidate_values,
    canonical_kerning_path,
    generate_kerning_proof,
    _render_context_image,
    save_canonical_kerning,
)
from binbook.render import _font

pytestmark = pytest.mark.proof


def test_candidate_pairs_include_research_seeds_and_existing_overrides():
    pairs = candidate_pairs(get_font("opendyslexic"))

    assert ("T", "o") in pairs
    assert ("A", "V") in pairs
    assert ("T", "h") in pairs
    assert ("r", "y") in pairs
    assert ("Y", "o") in pairs
    assert ("y", "o") in pairs


def test_candidate_values_include_current_value_outside_default_set():
    values = candidate_values(-123)

    assert list(DEFAULT_CANDIDATE_VALUES) == [0, -40, -60, -80, -100, -120, -140, -160]
    assert -123 in values


def test_generate_kerning_proof_creates_html_json_export_and_assets(tmp_path):
    output_dir = tmp_path / "opendyslexic-proof"

    result = generate_kerning_proof("opendyslexic", output_dir)

    assert result.index_html == output_dir / "index.html"
    assert result.report_json == output_dir / "report.json"
    assert result.suggested_table == output_dir / "approved_table.py.txt"
    assert result.index_html.exists()
    assert result.report_json.exists()
    assert result.suggested_table.exists()
    assert any((output_dir / "assets").glob("*.png"))


def test_opendyslexic_report_includes_controls_candidates_and_existing_values(tmp_path):
    generate_kerning_proof("opendyslexic", tmp_path)

    report = json.loads((tmp_path / "report.json").read_text())
    pairs = {entry["pair"]: entry for entry in report["pairs"]}

    assert report["controls"]["mixed_case"]["target_gap_px"] is not None
    assert report["controls"]["lowercase"]["measurements"]
    assert pairs["Yo"]["current_value"] == -120
    assert pairs["yo"]["current_value"] == -60
    assert pairs["Yo"]["suggested_value"] in pairs["Yo"]["candidate_values"]
    assert pairs["Yo"]["candidates"][0]["gap_px"] is not None
    assert pairs["Yo"]["candidates"][0]["image"].startswith("assets/")


def test_report_includes_contextual_english_renders_for_candidates(tmp_path):
    generate_kerning_proof("opendyslexic", tmp_path)

    report = json.loads((tmp_path / "report.json").read_text())
    pairs = {entry["pair"]: entry for entry in report["pairs"]}
    yo_candidate = pairs["Yo"]["candidates"][0]

    assert "Your young friend" in yo_candidate["contexts"][0]["text"]
    assert "young" in yo_candidate["contexts"][0]["text"]
    assert yo_candidate["contexts"][0]["image"].startswith("assets/")
    assert (tmp_path / yo_candidate["contexts"][0]["image"]).exists()


def test_contextual_render_preserves_saved_non_active_pair_adjustments(tmp_path):
    generate_kerning_proof("opendyslexic", tmp_path)

    report = json.loads((tmp_path / "report.json").read_text())
    pairs = {entry["pair"]: entry for entry in report["pairs"]}
    candidate = next(item for item in pairs["Yo"]["candidates"] if item["value"] == 0)
    context = candidate["contexts"][0]
    actual = Image.open(tmp_path / context["image"])

    font_info = get_font("opendyslexic")
    font = _font(report["font_size_px"], font_info)
    with_saved_table = _render_context_image(context["text"], "Yo", 0, font, font_info)
    with_active_pair_only = _render_context_image(
        context["text"],
        "Yo",
        0,
        font,
        font_info,
        base_pair_kerning_milli_em={},
    )

    assert ImageChops.difference(actual, with_saved_table).getbbox() is None
    assert ImageChops.difference(actual, with_active_pair_only).getbbox() is not None


def test_holistic_context_image_wraps_without_right_edge_clipping(tmp_path):
    generate_kerning_proof("opendyslexic", tmp_path)

    report = json.loads((tmp_path / "report.json").read_text())
    pairs = {entry["pair"]: entry for entry in report["pairs"]}
    candidate = pairs["Yo"]["candidates"][0]
    image = Image.open(tmp_path / candidate["holistic_context"]["image"])

    assert image.height > 138
    right_edge = image.crop((image.width - 2, 0, image.width, image.height))
    assert ImageChops.invert(right_edge.convert("L")).getbbox() is None


def test_generated_html_displays_contextual_renders(tmp_path):
    generate_kerning_proof("opendyslexic", tmp_path)

    html = (tmp_path / "index.html").read_text()

    assert "candidate-contexts" in html
    assert "context.image" in html


def test_report_includes_holistic_paragraph_renders_for_candidates(tmp_path):
    generate_kerning_proof("opendyslexic", tmp_path)

    report = json.loads((tmp_path / "report.json").read_text())
    pairs = {entry["pair"]: entry for entry in report["pairs"]}
    yo_candidate = pairs["Yo"]["candidates"][0]

    assert "holistic_context" in yo_candidate
    assert "Today" in yo_candidate["holistic_context"]["text"]
    assert "your young" in yo_candidate["holistic_context"]["text"]
    assert yo_candidate["holistic_context"]["image"].startswith("assets/")
    assert (tmp_path / yo_candidate["holistic_context"]["image"]).exists()


def test_literata_proof_generation_works_with_empty_pair_table(tmp_path):
    generate_kerning_proof("literata", tmp_path)

    report = json.loads((tmp_path / "report.json").read_text())

    assert report["font_family"] == "literata"
    assert report["existing_pair_kerning_milli_em"] == {}
    assert "AV" in {entry["pair"] for entry in report["pairs"]}


def test_load_pair_kerning_table_reads_json_pairs(tmp_path):
    kerning_file = tmp_path / "opendyslexic.json"
    kerning_file.write_text('{"Yo": -120, "yo": -60}\n')

    table = load_pair_kerning_table(kerning_file)

    assert table == {("Y", "o"): -120, ("y", "o"): -60}


def test_load_pair_kerning_table_rejects_malformed_json(tmp_path):
    kerning_file = tmp_path / "bad.json"
    kerning_file.write_text('{"Y": -120}\n')

    try:
        load_pair_kerning_table(kerning_file)
    except ValueError as exc:
        assert "two-character" in str(exc)
    else:
        raise AssertionError("malformed kerning JSON should be rejected")


def test_save_canonical_kerning_writes_sorted_json_and_removes_zero_values(tmp_path):
    output = tmp_path / "opendyslexic.json"

    save_canonical_kerning("opendyslexic", {"yo": -60, "Yo": -120, "AV": 0, "LT": None}, output)

    assert output.read_text() == '{\n  "Yo": -120,\n  "yo": -60\n}\n'


def test_save_canonical_kerning_rejects_invalid_payload(tmp_path):
    try:
        save_canonical_kerning("opendyslexic", {"Y": -120}, tmp_path / "opendyslexic.json")
    except ValueError as exc:
        assert "two-character" in str(exc)
    else:
        raise AssertionError("invalid pair key should be rejected")


def test_server_routes_report_assets_and_kerning_api(tmp_path):
    proof = generate_kerning_proof("opendyslexic", tmp_path)
    handler = KerningProofRequestHandler.create_test_handler("opendyslexic", tmp_path, proof.report)

    index = handler.handle_get("/")
    report = handler.handle_get("/report.json")
    api = handler.handle_get("/api/kerning")
    asset_name = proof.report["pairs"][0]["candidates"][0]["image"]
    asset = handler.handle_get(f"/{asset_name}")

    assert index.status == HTTPStatus.OK
    assert b"BinBook Kerning Proof" in index.body
    assert report.status == HTTPStatus.OK
    assert json.loads(report.body)["font_family"] == "opendyslexic"
    assert api.status == HTTPStatus.OK
    assert json.loads(api.body)["pairs"] == {"Yo": -120, "yo": -60}
    assert asset.status == HTTPStatus.OK
    assert asset.headers["Content-Type"] == "image/png"


def test_server_save_api_writes_canonical_table(tmp_path):
    proof = generate_kerning_proof("opendyslexic", tmp_path)
    target = tmp_path / "opendyslexic.json"
    handler = KerningProofRequestHandler.create_test_handler(
        "opendyslexic",
        tmp_path,
        proof.report,
        canonical_path=target,
    )

    response = handler.handle_post(
        "/api/kerning",
        json.dumps({"font_family": "opendyslexic", "pairs": {"Yo": -140, "yo": 0}}).encode(),
    )

    assert response.status == HTTPStatus.OK
    assert target.read_text() == '{\n  "Yo": -140\n}\n'


def test_server_save_api_rejects_path_traversal_font(tmp_path):
    proof = generate_kerning_proof("opendyslexic", tmp_path)
    handler = KerningProofRequestHandler.create_test_handler("opendyslexic", tmp_path, proof.report)

    response = handler.handle_post(
        "/api/kerning",
        json.dumps({"font_family": "../opendyslexic", "pairs": {"Yo": -120}}).encode(),
    )

    assert response.status == HTTPStatus.BAD_REQUEST


def test_cli_kerning_proof_rejects_unknown_font_family(tmp_path):
    exit_code = main(["kerning-proof", "--font-family", "missing-font", "--output-dir", str(tmp_path)])

    assert exit_code == 1


def test_cli_kerning_proof_static_creates_report(tmp_path):
    exit_code = main(["kerning-proof", "--static", "--font-family", "opendyslexic", "--output-dir", str(tmp_path)])

    assert exit_code == 0
    assert (tmp_path / "index.html").exists()
    assert (tmp_path / "report.json").exists()


def test_cli_kerning_proof_starts_server_by_default(tmp_path):
    with mock.patch("binbook.cli.serve_kerning_proof") as serve:
        serve.return_value = None

        exit_code = main(["kerning-proof", "--font-family", "opendyslexic", "--output-dir", str(tmp_path)])

    assert exit_code == 0
    serve.assert_called_once()


def test_canonical_kerning_path_stays_inside_package_data():
    path = canonical_kerning_path("opendyslexic")

    assert path.name == "opendyslexic.json"
    assert path.parent.name == "font_kerning"
    assert path.parent.parent.name == "binbook"
