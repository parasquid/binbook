# BinBook Documentation

The authoritative BinBook 0.1 candidate file-format specification lives at
[`../BINBOOK_FORMAT_SPEC.md`](../BINBOOK_FORMAT_SPEC.md).

## Current Reference

Use this ownership map when changing the reader. Record durable product and
architecture decisions in the reader design, executable cold-start task order
and acceptance criteria in the POC plan, reusable Rust constraints in the Rust
standards, and live machine/device state only in the gitignored `HANDOFF.md`.
Do not use `HANDOFF.md` or a dated precursor plan as the sole record of a
technical decision.

- [`specs/2026-07-01-library-menu-reading-flow-design.md`](specs/2026-07-01-library-menu-reading-flow-design.md) - current X4 e-reader decisions for library indexing, bounded caching/cursors, background refresh, reading, progress, display refresh, sleep/wake, and module ownership.
- [`specs/2026-07-14-epub-artwork-pagination-design.md`](specs/2026-07-14-epub-artwork-pagination-design.md) - current EPUB image discovery, caption association, portrait/two-panel pagination, no-crop composition, warning, memory, and anchor-remapping behavior.
- [`plans/2026-07-13-binbook-ereader-poc-plan.md`](plans/2026-07-13-binbook-ereader-poc-plan.md) - cold-start implementation order and evidence gates for completing that reader foundation.
- [`plans/2026-07-14-illustrated-epub-pagination-and-device-load-plan.md`](plans/2026-07-14-illustrated-epub-pagination-and-device-load-plan.md) - cold-start TDD and evidence plan for fixing UTF-8 pagination runaway allocation, rendering uncropped captioned EPUB artwork, and replacing the X4 library with the verified illustrated *Wind in the Willows* BinBook.
- [`reference/rust-development-standards.md`](reference/rust-development-standards.md) - operational Rust and firmware standards for crate placement, APIs, constrained memory, testing, hardware evidence, and completion review.
- [`reference/rust-crate-architecture.md`](reference/rust-crate-architecture.md) - implemented Rust workspace boundaries, external-consumer dependencies, buffer contracts, and quality gates.
- [`reference/xteink-x4-firmware-flashing.md`](reference/xteink-x4-firmware-flashing.md) - pinned X4 firmware build and flashing procedure.
- [`reference/xteink-x4-agent-device-verification.md`](reference/xteink-x4-agent-device-verification.md) - mandatory sequential serial and webcam verification runbook.
- [`reference/xteink-x4-grayscale-research.md`](reference/xteink-x4-grayscale-research.md) - supporting research for the Xteink X4 grayscale display profile.
- [`reference/xteink-x4-custom-lut-tuning.md`](reference/xteink-x4-custom-lut-tuning.md) - X4 Balanced/Fast refresh mode notes, custom SSD1677 LUT tuning, tail-byte terminology, metrics, and update guidance.
- [`ROADMAP.md`](ROADMAP.md) - evidence-gated implementation work.

## Dated Plans And Specs

Unless explicitly listed under Current Reference above, files under
[`plans/`](plans/) and dated files under [`specs/`](specs/) are implementation
records. They can mention old crate paths, old protocol versions, temporary
artifact paths, or tasks that have since completed. Prefer the documents named
under Current Reference, the root format spec, and [`ROADMAP.md`](ROADMAP.md)
for present-state behavior.

## Historical POC Material

Files under [`historical/`](historical/) describe the original proof-of-concept direction. They are preserved for context, but they are not authoritative for BinBook 0.1.

- [`historical/codex-prompt-binbook-poc.md`](historical/codex-prompt-binbook-poc.md)
- [`historical/binbook-poc-spec.md`](historical/binbook-poc-spec.md)
- [`historical/binbook-poc-docs-and-roadmap.md`](historical/binbook-poc-docs-and-roadmap.md)
