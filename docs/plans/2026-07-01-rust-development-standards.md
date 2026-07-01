# Rust Development Standards Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` and execute this plan sequentially in the main thread. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an operational Rust and firmware development guide that future contributors and coding agents can follow without prior refactor context.

**Architecture:** Keep crate ownership in `docs/reference/rust-crate-architecture.md` and device commands in the X4 verification runbook. Add one task-oriented reference that translates those contracts into implementation, testing, hardware-evidence, and completion rules, then link it from the two locations agents inspect first.

**Tech Stack:** Markdown, Cargo workspace commands, Rust 2021 `no_std`, embedded-hal 1.0, Xteink X4 hardware verification.

---

## Tasks

### Task 1: Write the operational standards reference

**Files:**

- Create: `docs/reference/rust-development-standards.md`
- Read: `AGENTS.md`
- Read: `docs/reference/rust-crate-architecture.md`
- Read: `docs/reference/xteink-x4-agent-device-verification.md`
- Read: `docs/specs/2026-07-01-rust-development-standards-design.md`

- [x] Write a scope section that names the format specification, architecture reference, and device runbook as authoritative sources.
- [x] Add a sequential change workflow from ownership selection through adversarial completion review.
- [x] Add a crate-placement decision table and dependency rules without duplicating the full architecture reference.
- [x] Document `no_std`, standard HAL traits, typed APIs, caller-owned buffers, bounded streaming, category-preserving errors, explicit state, and small-module requirements.
- [x] Document discriminating tests, feature-gated test commands, reusable-crate quality gates, pinned firmware builds, and Python compatibility checks.
- [x] Document mandatory sequential hardware verification, the difference between transport and outcome evidence, acceptance matrices, and `HANDOFF.md` requirements.
- [x] Add concise allowed/forbidden examples and a pre-completion checklist whose items have observable outcomes.

### Task 2: Make the guide discoverable

**Files:**

- Modify: `AGENTS.md`
- Modify: `docs/README.md`

- [x] Link `docs/reference/rust-development-standards.md` from the firmware modularity section in `AGENTS.md`.
- [x] Add the guide to the current-reference list in `docs/README.md` with descriptive link text.

### Task 3: Verify and commit the documentation

**Files:**

- Verify: `docs/reference/rust-development-standards.md`
- Verify: `AGENTS.md`
- Verify: `docs/README.md`
- Verify: `docs/plans/2026-07-01-rust-development-standards.md`

- [x] Run `git diff --check` and require exit status 0.
- [x] Run `rg -n "rust-development-standards|Rust development standards" AGENTS.md docs/README.md docs/reference docs/specs docs/plans` and confirm the guide, design, plan, and both discovery links are present.
- [x] Run the stale-boundary scan from the completed refactor plan and confirm any matches occur only in clearly historical or removal statements:

```bash
rg -n "binbook/rust|firmware/target|firmware/crates/ssd1677-driver|xteink[_-]hal|PageRef|decompress_page\(|\[u8; 8192\]" . --glob '!target/**' --glob '!.git/**'
```

- [x] Compare every documented command against `Cargo.toml`, crate feature declarations, `AGENTS.md`, and the current reference runbooks.
- [x] Review the final diff for placeholders, contradictory rules, vague completion claims, inaccessible heading order, and unrelated changes.
- [x] Stage only the four documentation files named in this plan and commit with `docs: add Rust development standards`.
