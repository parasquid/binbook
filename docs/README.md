# BinBook Documentation

The authoritative BinBook 0.1 candidate file-format specification lives at
[`../BINBOOK_FORMAT_SPEC.md`](../BINBOOK_FORMAT_SPEC.md).

## Current Reference

- [`reference/rust-development-standards.md`](reference/rust-development-standards.md) - operational Rust and firmware standards for crate placement, APIs, constrained memory, testing, hardware evidence, and completion review.
- [`reference/rust-crate-architecture.md`](reference/rust-crate-architecture.md) - implemented Rust workspace boundaries, external-consumer dependencies, buffer contracts, and quality gates.
- [`reference/xteink-x4-firmware-flashing.md`](reference/xteink-x4-firmware-flashing.md) - pinned X4 firmware build and flashing procedure.
- [`reference/xteink-x4-agent-device-verification.md`](reference/xteink-x4-agent-device-verification.md) - mandatory sequential serial and webcam verification runbook.
- [`reference/xteink-x4-grayscale-research.md`](reference/xteink-x4-grayscale-research.md) - supporting research for the Xteink X4 grayscale display profile.
- [`ROADMAP.md`](ROADMAP.md) - evidence-gated implementation work.

## Dated Plans And Specs

Files under [`plans/`](plans/) and dated files under [`specs/`](specs/) are implementation records. They can mention old crate paths, old protocol versions, temporary artifact paths, or tasks that have since completed. Prefer the current reference docs, the root format spec, and [`ROADMAP.md`](ROADMAP.md) for present-state behavior.

## Historical POC Material

Files under [`historical/`](historical/) describe the original proof-of-concept direction. They are preserved for context, but they are not authoritative for BinBook 0.1.

- [`historical/codex-prompt-binbook-poc.md`](historical/codex-prompt-binbook-poc.md)
- [`historical/binbook-poc-spec.md`](historical/binbook-poc-spec.md)
- [`historical/binbook-poc-docs-and-roadmap.md`](historical/binbook-poc-docs-and-roadmap.md)
