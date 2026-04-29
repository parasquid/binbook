from __future__ import annotations

import pytest


def pytest_addoption(parser: pytest.Parser) -> None:
    parser.addoption(
        "--run-proof",
        action="store_true",
        default=False,
        help="run slow kerning proof generation tests",
    )


def pytest_configure(config: pytest.Config) -> None:
    config.addinivalue_line("markers", "proof: slow kerning proof generation tests")


def pytest_collection_modifyitems(config: pytest.Config, items: list[pytest.Item]) -> None:
    if config.getoption("--run-proof"):
        return
    skip_proof = pytest.mark.skip(reason="need --run-proof option to run kerning proof tests")
    for item in items:
        if "proof" in item.keywords:
            item.add_marker(skip_proof)
