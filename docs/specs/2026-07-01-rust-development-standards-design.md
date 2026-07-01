# Rust Development Standards Guide Design

## Purpose

Create one operational reference for contributors and coding agents changing the
Rust workspace or Xteink X4 firmware. The guide must turn the modular foundation
into concrete rules that are easy to apply during implementation and hard to
misread during verification.

The guide complements
[`../reference/rust-crate-architecture.md`](../reference/rust-crate-architecture.md).
The architecture reference remains the source for crate responsibilities and
dependency direction. The new guide explains how to make, test, and verify a
change without violating those boundaries.

## Audience

- Coding agents that may start without prior conversation context.
- Contributors who understand Rust but are unfamiliar with constrained embedded
  systems or this repository's evidence requirements.
- Reviewers deciding whether a change is correctly placed and adequately proven.

## Deliverables

1. Add `docs/reference/rust-development-standards.md`.
2. Link the guide from `AGENTS.md` near the firmware architecture rules.
3. Add the guide to the current-reference list in `docs/README.md`.

## Required content

The guide will use direct requirements, short rationale, and allowed/forbidden
examples. It will cover:

1. **Choose the owning crate**: place behavior by responsibility, preserve the
   downward dependency graph, and keep `binbook-fw` as the composition root.
2. **Design reusable APIs**: require `no_std`, standard embedded traits, typed
   identifiers, explicit state, caller-owned buffers, bounded streaming, and
   category-preserving errors.
3. **Protect constrained memory**: reject hidden full-page buffers, oversized
   scratch arrays, accidental by-value copies, and larger stacks used as fixes.
4. **Implement with evidence**: begin behavior changes with a discriminating
   failing test, verify feature-gated paths explicitly, and test payloads that
   cross scratch-buffer boundaries.
5. **Use the correct verification ladder**: run focused tests first, then
   workspace tests, reusable-crate Clippy and firmware-target checks, pinned
   firmware builds, and Python compatibility tests when relevant.
6. **Verify firmware on hardware**: require sequential flash, serial, diagnostic
   queries, and fresh webcam evidence for firmware changes; distinguish transport
   acknowledgements from observed state and visible output.
7. **Document completion honestly**: maintain an acceptance matrix and current
   `HANDOFF.md`; identify verified behavior, transport-only evidence, unverified
   visual behavior, known failures, and incomplete requirements.
8. **Finish with an adversarial review**: inspect every affected option, opcode,
   state transition, and feature gate; try to disprove completion with a
   non-default starting state or boundary-sized input.

## Structure

The reference guide will contain these sections:

1. Scope and authority
2. Change workflow
3. Crate placement decision table
4. API and dependency standards
5. Memory and streaming standards
6. Error and state standards
7. Test standards
8. Verification commands by change type
9. Hardware acceptance rules
10. Completion evidence and `HANDOFF.md`
11. Allowed and forbidden patterns
12. Pre-completion checklist

The guide will not repeat the full crate descriptions, wire-format rules, flash
procedure, or device-verification commands. It will link to the authoritative
architecture, format specification, and device runbook instead.

## Quality checks

- Every normative rule must agree with `AGENTS.md`, the modular-foundation plan,
  the implemented crate boundaries, and current build commands.
- Every command must be runnable from the stated directory and use the pinned
  toolchain where required.
- Examples must use real crate names and implemented patterns. Any illustrative
  value must be marked as illustrative.
- Headings must form a logical hierarchy, links must describe their target, and
  each checklist item must state an observable completion condition.
- A contradiction scan must cover `AGENTS.md`, `README.md`, `docs/`, workspace
  manifests, and the new guide.

## Non-goals

- Changing Rust APIs, crate boundaries, firmware behavior, or the BinBook format.
- Replacing the architecture reference or hardware-verification runbook.
- Adding generic Rust style advice already enforced by `rustfmt`, Clippy, or
  workspace lints.
- Documenting historical refactor chronology.
