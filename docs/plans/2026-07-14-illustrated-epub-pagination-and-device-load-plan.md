# Illustrated EPUB Pagination and X4 Device Replacement Plan

**Status:** Completed and verified on live Xteink X4 hardware on 2026-07-15.

> **For the implementing agent:** Start cold from the repository root and
> execute this plan sequentially in the current branch/worktree. Do not
> delegate to subagents. Use a todo tracker and update it as each task moves
> from pending to in progress to complete. Every implementation task starts
> with the smallest meaningful failing test and ends with its focused tests
> green. Do not delete anything from the X4 until the replacement BinBook has
> passed every host validation gate.

**Goal:** Fix the EPUB renderer's runaway allocation at a UTF-8 page boundary,
render the real artwork and captions from Project Gutenberg's improved
illustrated edition of *The Wind in the Willows*, compile it for the
`xteink-x4-portrait` profile without cropping any source pixels, then make the
X4 library contain exactly that one byte-verified BinBook.

**Why this plan exists:** Encoding Project Gutenberg eBook 289 reproduced a
real compiler defect. A 363 KiB EPUB drove resident memory from about 1.2 GiB
after one second to more than 15 GiB after twelve seconds before the process
was killed. Static inspection localized the defect to
`crates/binbook-render/src/pagination.rs::push_text`: if a page has fewer bytes
remaining than the next UTF-8 scalar requires, `floor_char_boundary` returns
zero, the loop appends an empty span, and no input is consumed. Substituting a
no-images EPUB is not an acceptable workaround. The user selected Gutenberg's
improved illustrated eBook 27805 and requires its artwork in the output.

**Scope boundary:** This is a host compiler/rendering correction plus a live
device content replacement. It is not the broader bounded-memory/on-device
compiler project described in `docs/ROADMAP.md`. `BookBuilder` may continue to
retain compiled pages for this task; the acceptance bound below proves that
the corrected host compiler handles this book safely. No firmware source or
wire protocol change is expected. If implementation unexpectedly touches
firmware, stop and add the mandatory flash, fifteen-second serial capture, and
full hardware gate from `AGENTS.md` before claiming completion.

**Primary references:**

- `AGENTS.md` and the optional host-only `AGENTS.local.md`
- `BINBOOK_FORMAT_SPEC.md`
- `README.md`
- `docs/reference/rust-development-standards.md`
- `docs/reference/rust-crate-architecture.md`
- `docs/reference/xteink-x4-agent-device-verification.md`
- `docs/ROADMAP.md`, section "Single streaming compiler core for host and
  device"; preserve this future-work entry

**Expected starting state:**

- The earlier e-reader work is committed on `main` at or after `245b65b`.
- `git status --short` is clean for tracked files.
- Generated evidence belongs below `target/diag-evidence/` and must remain
  untracked.
- The current firmware on the host-local `$PORT` has already advertised protocol 2 and
  capabilities `KEY,PAGE,STATUS,LOG,CRASH,DISPLAY_PROBE,STORAGE,REFRESH_MODE`.
- A read-only storage snapshot listed 29 `.binbook` entries. This number is
  only a starting observation; always query the device again immediately
  before deletion.
- Mounted `StoreList` currently reads the cached library index, which contains
  names and sizes but not page counts; its response therefore reports
  `pages=0`. Treat that field as a known non-gate in this task. Prove page count
  from strict host/readback inspection and live STATUS after opening instead.
- The host-local overlay identifies the working webcam and current crop. Read
  those values from `AGENTS.local.md`; do not copy them into tracked docs.

---

## 1. Locked Product and Rendering Decisions

Do not reopen these decisions during implementation unless repository evidence
proves one infeasible and the user explicitly approves a change.

### Source and output

- Use Project Gutenberg eBook **27805**, the improved illustrated edition:
  `https://www.gutenberg.org/ebooks/27805.epub3.images`.
- The source observed while writing this plan had SHA-256
  `0493439baf6f3a66ff2721850524fd9497cb10ecc4257c21d1e63b8498420e4d`.
  Gutenberg can update files in place. If a fresh download differs, stop,
  inspect its metadata/archive contents, record the new digest in `HANDOFF.md`,
  and ensure it is still eBook 27805 before encoding it.
- Compile the default `xteink-x4-portrait` `GRAY2_PACKED` rendition. Do not use
  `GRAY1_PACKED`, `--no-dither`, or a no-images EPUB for the device artifact.
- Name the device file `the-wind-in-the-willows.binbook`.

### Artwork extraction

- Render ordinary HTML `<img src="...">` resources.
- Render SVG cover wrappers that contain `<image href="...">` or
  `<image xlink:href="...">`. The Gutenberg cover uses this form.
- Associate an image with caption text from its containing `<figcaption>` or
  `.caption` element when it is inside `<figure>`, `role="figure"`, a
  `.figcenter` container, or the Gutenberg cover wrapper.
- Remove the associated caption node from normal text flow so the caption is
  not duplicated on the following text page.
- Do not display `alt` text as a normal caption. Retain it only for
  accessibility data and the deterministic unsupported/missing-image fallback.
- Unsupported or unreadable artwork produces a stable `UnsupportedImage`
  warning and a visible text fallback; it must not be silently omitted or abort
  the rest of an otherwise valid EPUB.

### Artwork pagination

- Decide orientation from the decoded pixel dimensions after any decoder-
  supplied orientation handling: `width <= height` is portrait/square;
  `width > height` is landscape.
- A portrait or square image produces one artwork page.
- A landscape image produces two consecutive portrait panels in left-to-right
  reading order.
- Split landscape images at the vertical midpoint. For odd source widths, the
  left panel owns the extra column. The two panels must partition the source:
  no column is omitted, repeated, overlapped, or center-cropped.
- Aspect-contain a complete portrait/square image or one landscape panel in its
  available page rectangle. Never distort aspect ratio. White letterboxing is
  allowed and is not cropping.
- Captionless artwork uses the complete logical 480x800 page rectangle.
- Captioned artwork uses the existing 24-logical-pixel outer margin, a
  centered 18-logical-pixel caption with 22-logical-pixel line height, and a
  bottom caption band of at most four wrapped lines. The artwork is
  aspect-contained in the remaining rectangle.
- Repeat the caption on both pages of a landscape image.
- If a caption exceeds four lines, emit a stable `UnsupportedContent` warning
  and truncate only at a UTF-8 character boundary with a visible ellipsis.
  Never reintroduce the zero-progress boundary bug in caption truncation.

### Device replacement

- Validate the local artifact completely before mutating the SD card.
- Close the currently open book and visibly/query-confirm the library before
  deleting files.
- Delete only names returned by diagnostic storage listing that end in
  `.binbook`; do not format the card and do not delete state or unrelated
  files.
- Require a discriminating empty-list result before upload.
- Final device state is exactly one valid library entry named
  `the-wind-in-the-willows.binbook`, with the expected byte size. Do not use
  mounted `StoreList.pages` as validity evidence because it is currently a
  documented zero placeholder.
- Upload acknowledgement alone is transport evidence. Completion additionally
  requires storage listing, complete readback, byte/hash comparison, strict
  inspection of the readback, opening the actual book, queried page state, and
  a fresh webcam image of the cover.

---

## 2. Current Code Path and Intended Boundaries

Read these files before editing:

- `crates/binbook-render/src/pagination.rs`
- `crates/binbook-render/src/document.rs`
- `crates/binbook-render/src/raster.rs`
- `crates/binbook-document/src/node.rs`
- `crates/binbook-epub/src/html.rs`
- `crates/binbook-image/src/decode.rs`
- `crates/binbook-image/src/compile.rs`
- `crates/binbook-compiler/src/compile.rs`
- `crates/binbook-compiler/src/api.rs`
- `crates/binbook/src/encode.rs`
- `crates/binbook/src/diag_cli.rs`

The intended responsibility split is:

| Crate | Responsibility in this change |
| --- | --- |
| `binbook-document` | Typed image node with resource, alt text, and optional real caption |
| `binbook-epub` | Resolve HTML/SVG image references and associate figure captions without rendering |
| `binbook-render` | Text pagination progress, image-page expansion, caption layout, anchor remapping, and 2x page composition |
| `binbook-image` | Existing path-free static image decoding and final X4 image compilation |
| `binbook-compiler` | Pass pixel/dither options into the renderer and assemble the resulting pages |
| `binbook` | Existing atomic filesystem output and existing diagnostic storage commands; no new command is required |

Locked API adjustments:

```rust
pub enum Node {
    // existing variants...
    Image {
        resource: ResourceId,
        alt: String,
        caption: Option<String>,
    },
}

impl Node {
    // Keep this source-compatible constructor.
    pub fn image(resource: ResourceId, alt: impl Into<String>) -> Self;

    // Add one explicit constructor; do not make callers construct fields ad hoc.
    pub fn captioned_image(
        resource: ResourceId,
        alt: impl Into<String>,
        caption: impl Into<String>,
    ) -> Self;
}
```

Add `UnsupportedImage` to `binbook_render::WarningCode` and
`binbook_compiler::CompileWarningCode`, including a stable CLI warning message.
This is an additive public enum change; update exhaustive matches and tests.

Expand `binbook_render::RenderOptions` with the existing
`binbook_image::StorageFormat` and `dither: bool`, defaulting to GRAY2 and true.
`binbook-compiler` must derive these from `CompileOptions`. This ensures text,
portrait artwork, and landscape panels all honor the same selected rendition;
do not hard-code `CompileOptions::default()` inside rasterization.

Keep the public `render_document(...) -> RenderedDocument` entry point. Use a
private page model similar to:

```rust
enum PageContent {
    Text { spans: Vec<Span> },
    Artwork {
        resource: ResourceId,
        alt: String,
        caption: Option<String>,
    },
}
```

One private artwork input page may emit one or two compiled pages. Record the
compiled output start index for every private layout page, then remap anchors
through that table. Do not guess navigation offsets by counting images later.

For image composition, decode one resource once, create one 960x1600 white RGBA
canvas at a time, copy/resize the whole image or selected exact panel with
Lanczos containment, draw any caption using the same supplied `FontSystem`, and
immediately pass that canvas to `binbook_image::compile_decoded_image`. It is
acceptable for `binbook-render` to add the workspace `image` dependency because
page composition belongs to the host renderer; image decoding and X4 plane
construction remain in `binbook-image`. Drop the decoded image and canvas after
its one or two `CompiledPage` values are produced.

---

## 3. Task 0 - Establish a Safe Baseline

- [x] Read all authoritative references and code-path files listed above.
- [x] Start/update the todo tracker with Tasks 0 through 10 from this plan.
- [x] Confirm branch, commits, and tracked cleanliness:

  ```bash
  git branch --show-current
  git log -3 --oneline
  git status --short
  ```

- [x] Confirm the official source is present or download it only to ignored
  evidence storage:

  ```bash
  mkdir -p target/diag-evidence/wind-in-the-willows
  curl --fail --location \
    --output target/diag-evidence/wind-in-the-willows/pg27805.epub \
    https://www.gutenberg.org/ebooks/27805.epub3.images
  sha256sum target/diag-evidence/wind-in-the-willows/pg27805.epub
  unzip -l target/diag-evidence/wind-in-the-willows/pg27805.epub
  ```

- [x] Confirm the archive metadata says eBook 27805, title *The Wind in the
  Willows*, author Kenneth Grahame, and contains the 16 expected JPEG resources.
- [x] Do **not** rerun the known runaway encoder before Task 1. The recorded
  >15 GiB RSS trace is sufficient RED evidence and another run can destabilize
  the host.
- [x] Run focused pre-change tests to distinguish new regressions from existing
  state:

  ```bash
  rustup run nightly cargo test -p binbook-document
  rustup run nightly cargo test -p binbook-epub
  rustup run nightly cargo test -p binbook-render
  rustup run nightly cargo test -p binbook-compiler
  ```

**Gate:** Existing focused tests pass, the source is confirmed, and no device
mutation has occurred.

---

## 4. Task 1 - Guarantee Progress at UTF-8 Page Boundaries

**Primary file:** `crates/binbook-render/src/pagination.rs`

- [x] Add a private split-decision helper and its failing unit tests before
  changing `push_text`. The helper should return `None` when a nonempty string
  has no complete scalar that fits the remaining budget. Cover a 2-byte,
  3-byte, and 4-byte scalar with only 1, 2, and 3 bytes remaining.
- [x] Run the exact new tests and observe RED because the helper/behavior does
  not exist. Do not run a test that deliberately enters the old infinite loop.
- [x] Implement the smallest progress-safe behavior:
  - an empty input returns immediately;
  - a positive split appends a nonempty span and consumes exactly that prefix;
  - a zero/`None` split starts a new text page without appending an empty span;
  - every loop iteration therefore consumes input or increases usable page
    capacity.
- [x] Add the full regression test using `"a".repeat(1249)` followed by UTF-8
  punctuation and tail text. Assert:
  - two pages are produced;
  - every span is nonempty;
  - concatenated text is byte-identical to the input;
  - no page exceeds the 1,250-byte budget;
  - a second run is deterministic.
- [x] Include a case where the boundary text contains the curly quote/em dash
  class used by the Gutenberg source.
- [x] Run:

  ```bash
  rustup run nightly cargo test -p binbook-render
  rustup run nightly cargo test -p binbook-compiler
  ```

- [x] Commit only this independently correct bug fix and its tests:

  ```bash
  git add crates/binbook-render/src/pagination.rs
  git commit -m "fix(render): guarantee progress at UTF-8 page boundaries"
  ```

**Gate:** The regression terminates quickly, preserves every input byte, emits
no empty spans, and focused suites pass.

---

## 5. Task 2 - Represent Captions and Parse Gutenberg Artwork

**Primary files:**

- `crates/binbook-document/src/node.rs`
- `crates/binbook-document/tests/model.rs`
- `crates/binbook-epub/src/html.rs`
- `crates/binbook-epub/tests/epub.rs` and its synthetic fixture builder

- [x] First update model tests to require captionless and captioned image
  constructors to preserve resource ID, alt text, and optional caption.
- [x] Run `cargo test -p binbook-document` and observe RED.
- [x] Add the `caption` field and constructor shown in section 2; update all
  exhaustive `Node::Image` patterns with explicit fields or `..`.
- [x] Run `cargo test -p binbook-document` and require GREEN.
- [x] Extend the synthetic EPUB fixture before parser implementation with:
  - a normal `<img>` without a caption;
  - a Gutenberg-style `<div class="figcenter" role="figure">` containing one
    `<img>` and one `<span class="caption">`;
  - an SVG wrapper containing `<image xlink:href="...">`;
  - a missing image reference.
- [x] Add failing parser assertions that:
  - all three present resources become `Node::Image` nodes;
  - only the figure image receives its normalized real caption;
  - the associated caption is absent from ordinary text nodes;
  - the SVG reference resolves relative to its XHTML resource;
  - the missing resource remains a visible fallback plus a
    `MissingResource` diagnostic.
- [x] Implement figure extraction without leaking DOM/dependency types into
  public APIs. Only collapse a recognized figure/cover container when exactly
  one descendant image reference resolves; otherwise keep normal recursive
  conversion and diagnostics.
- [x] Attribute matching must use local names so `href` and `xlink:href` both
  work. Normalize caption whitespace in the same deterministic manner as body
  text.
- [x] Ensure the consumed caption subtree is not converted a second time.
- [x] Run:

  ```bash
  rustup run nightly cargo test -p binbook-document
  rustup run nightly cargo test -p binbook-epub
  rustup run nightly cargo clippy -p binbook-epub --all-targets -- -D warnings
  ```

**Gate:** Synthetic ordinary, captioned, SVG-wrapped, and missing images have
the locked typed representation and no duplicate caption text.

---

## 6. Task 3 - Render Portrait and Exact Two-Panel Landscape Artwork

**Primary files:**

- `crates/binbook-render/src/pagination.rs`
- `crates/binbook-render/src/document.rs`
- `crates/binbook-render/src/raster.rs`
- `crates/binbook-render/src/warning.rs`
- `crates/binbook-render/tests/reflow.rs`
- `crates/binbook-render/tests/navigation.rs`
- `crates/binbook-render/tests/golden.rs`
- `crates/binbook-render/Cargo.toml`

- [x] Add RED render tests using in-memory `Document` resources and tiny
  lossless images with unmistakable colors/patterns:
  1. portrait art between two text blocks produces text, one artwork page,
     then text;
  2. square art produces one page;
  3. landscape art produces left then right pages;
  4. an odd-width landscape source assigns the center column exactly once;
  5. reconstructed left+right source coverage contains every source column
     exactly once and no pixels outside each panel;
  6. both landscape pages contain the same caption band;
  7. captionless portrait art uses the full available viewport;
  8. aspect ratio is preserved and white letterboxing is allowed;
  9. unsupported image bytes produce a visible alt fallback and the new stable
     warning instead of aborting the document.
- [x] Run the exact new tests and observe RED.
- [x] Introduce private text/artwork page variants. On image insertion:
  - finish a nonempty current text page;
  - do not preserve redundant empty pages around the image;
  - append one private artwork input page;
  - resume text on a fresh page.
- [x] Add `UnsupportedImage` to render/compiler warning enums, mappings, CLI
  message, and exhaustive tests before using it.
- [x] Expand `RenderOptions` with storage format and dithering. Update every
  construction site and default without changing default output.
- [x] Implement 2x RGBA artwork composition:
  - decode a resource once;
  - partition landscape source rectangles exactly;
  - create only one 960x1600 canvas at a time;
  - aspect-contain with Lanczos filtering;
  - render caption text with supplied fonts and locked dimensions;
  - compile with the requested storage format/dither settings;
  - release temporary decoded/canvas buffers promptly.
- [x] For long captions, determine the last fitting UTF-8 boundary, append an
  ellipsis, and emit `UnsupportedContent`. Add a boundary test containing
  multibyte punctuation.
- [x] Record the compiled-output start page for each private input page. Remap
  every anchor through that table after rasterization.
- [x] Add a navigation test with anchors before and after a landscape image;
  the later target must shift by two artwork pages, not one.
- [x] Ensure warnings remain stably sorted and include resource/spine context.
- [x] Run:

  ```bash
  rustup run nightly cargo test -p binbook-render
  rustup run nightly cargo test -p binbook-compiler
  rustup run nightly cargo clippy -p binbook-render --all-targets -- -D warnings
  ```

**Gate:** All image orientation, exact-panel, caption, fallback, and navigation
tests pass; no test relies only on page count.

---

## 7. Task 4 - Prove End-to-End Compiler Behavior

**Primary files:**

- `crates/binbook-compiler/src/compile.rs`
- `crates/binbook-compiler/src/support.rs`
- `crates/binbook-compiler/tests/compile_epub.rs`
- `crates/binbook/tests/compiler_cli.rs`

- [x] Extend the compiler's synthetic EPUB fixture with one portrait image,
  one odd-width landscape image, captions, text, and a navigation target after
  the landscape image.
- [x] Add RED assertions for:
  - strict BinBook validation;
  - expected metadata and navigation;
  - one portrait plus two landscape pages in correct sequence;
  - correct decoded pixel patterns, not merely nonwhite output;
  - `GRAY1` and `--no-dither` propagation through EPUB artwork when explicitly
    selected;
  - `UnsupportedImage` warning propagation;
  - deterministic complete output bytes across two runs.
- [x] Pass `CompileOptions.pixel_format` and `dither` into `RenderOptions`.
  Do not change CLI flags or BinBook wire layout.
- [x] Add a process-level CLI regression containing the UTF-8 boundary and
  artwork. Require atomic output: success leaves one valid output; any injected
  render failure leaves no partial final file.
- [x] Run:

  ```bash
  rustup run nightly cargo test -p binbook-compiler
  rustup run nightly cargo test -p binbook --test compiler_cli
  rustup run nightly cargo clippy -p binbook-compiler --all-targets -- -D warnings
  rustup run nightly cargo check -p binbook-compiler --target wasm32-unknown-unknown
  rustup run nightly cargo test -p binbook-compiler --target wasm32-unknown-unknown --no-run
  ```

**Gate:** The path-free compiler produces deterministic, strictly valid output
with actual art, correct navigation, and requested rendition options.

---

## 8. Task 5 - Document Current Behavior and Run Full Host Gates

**Documentation:**

- [x] Add `docs/specs/2026-07-14-epub-artwork-pagination-design.md` as a
  current-state technical specification. Record image discovery, caption
  association, orientation heuristic, exact landscape partition, no-crop
  invariant, caption layout, fallback/warning behavior, anchor remapping, and
  bounded per-image temporaries.
- [x] Update `README.md` so its existing "static images" claim states the
  actual standalone-page behavior rather than an unqualified promise.
- [x] Update `docs/reference/rust-crate-architecture.md` only where the current
  crate responsibility description needs clarification.
- [x] Link the new spec from `docs/README.md`.
- [x] Grep `docs/ROADMAP.md` for `image`, `EPUB`, `memory`, `stream`, and
  `compiler`. Keep the future single-streaming-compiler work because this task
  does not complete it; remove or rewrite only claims directly contradicted by
  the implementation.
- [x] Do not add host-local paths, webcam coordinates, temporary filenames, or
  evidence paths to tracked docs.

**Full gates:**

```bash
cargo clean
rustup run nightly cargo test --workspace
rustup run nightly cargo test -p binbook --features serial-device
rustup run nightly cargo test -p binbook-fw --features diagnostic-console
RUSTUP_TOOLCHAIN=stable uv run pytest -q
rustup run nightly cargo clippy -p binbook-compiler --all-targets -- -D warnings
rustup run nightly cargo check -p binbook-compiler --target wasm32-unknown-unknown
rustup run nightly cargo test -p binbook-compiler --target wasm32-unknown-unknown --no-run
rustup run nightly cargo fmt --all --check
git diff --check
```

- [x] Distinguish any pre-existing failure from a regression with exact output.
  Do not waive a new failure.
- [x] Commit implementation and documentation with focused Conventional
  Commits. The intended remaining commits are:

  ```text
  feat(render): paginate EPUB illustrations
  docs(compiler): define EPUB artwork pagination
  ```

  Stage specific files only; never use `git add .` or `git add -A`.
- [x] Push the current branch to its configured upstream after all committed-
  tree checks pass. Do not amend, force-push, or change branches.

**Gate:** The committed tree passes all relevant host/WASM/Python checks and
tracked documentation describes current behavior accurately.

---

## 9. Task 6 - Compile and Inspect the Official Illustrated Book

Build the release CLI with device diagnostics enabled so one exact binary is
used for encoding and storage operations:

```bash
rustup run nightly cargo build \
  -p binbook --release --features serial-device
```

- [x] Remove only stale generated output from previous failed attempts; keep
  the verified source EPUB.
- [x] Record source size and SHA-256.
- [x] Resolve a GNU-compatible `time` executable, run the release encoder with
  verbose resource reporting, and preserve its full
  transcript under ignored evidence storage:

  ```bash
  TIME_BIN="$(command -v time)"
  "$TIME_BIN" -v target/release/binbook encode \
    target/diag-evidence/wind-in-the-willows/pg27805.epub \
    -o target/diag-evidence/wind-in-the-willows/the-wind-in-the-willows.binbook
  ```

- [x] Require normal exit, a nonempty final file, no partial sibling file, and
  maximum resident set size below **512 MiB**. If memory exceeds the bound, do
  not merely raise it: identify retained page/image buffers and add a targeted
  regression before proceeding.
- [x] Strictly inspect and record JSON:

  ```bash
  target/release/binbook inspect \
    target/diag-evidence/wind-in-the-willows/the-wind-in-the-willows.binbook \
    --validate --strict --json
  sha256sum \
    target/diag-evidence/wind-in-the-willows/the-wind-in-the-willows.binbook
  ```

- [x] Require title `The Wind in the Willows`, author `Kenneth Grahame`, a
  nonzero page count, default GRAY2 X4 rendition, and zero validation issues.
- [x] Decode page 0 and confirm it is the actual cover, not alt text or a blank
  page.
- [x] Decode pages 0 through 40 (or through the end if the book is shorter)
  into a numbered ignored directory. Build a thumbnail contact sheet with an
  available image tool, then record the exact full-resolution page numbers for
  the cover, flyleaf panels, and first story illustration. Do not guess page
  numbers from source chapter numbers.
- [x] Use those recorded page numbers to inspect at least:
  - the cover;
  - one portrait story illustration with its caption;
  - the left and right pages of the 630x423 landscape flyleaf, both with the
    same caption.
- [x] Inspect those decoded images directly. Verify correct orientation,
  readable captions, aspect preservation, exact left-to-right coverage, no
  discarded content, and only intentional white letterboxing.
- [x] Run strict inspection a second time after visual inspection to ensure no
  artifact was modified.

**Gate:** The official illustrated EPUB compiles within the memory bound into
a visually and structurally correct BinBook. Only now may device deletion
begin.

---

## 10. Task 7 - Establish Device Preconditions and Close the Active Book

Use portable variables in transcripts:

```bash
PORT="${PORT:?set PORT from the live host/device context}"
BIN=target/release/binbook
BOOK=target/diag-evidence/wind-in-the-willows/the-wind-in-the-willows.binbook
NAME=the-wind-in-the-willows.binbook
```

- [x] Run actual host-bound checks; never infer device state from sandboxed
  device visibility:

  ```bash
  "$BIN" diag hello --port "$PORT"
  "$BIN" diag status --port "$PORT"
  "$BIN" diag storage list --port "$PORT"
  ```

- [x] Require protocol 2 and `STORAGE`, `KEY`, `STATUS`, and `LOG`
  capabilities. Record the complete initial list.
- [x] Clear logs, inject `BACK`, retrieve all new logs, and independently
  verify the reader returned to the library. Do not infer library mode from an
  `ok` key acknowledgement.
- [x] Take a fresh webcam capture using `AGENTS.local.md`, show the capture path
  to the user, inspect that exact file, and verify the library is visible. If
  the device/camera moved, recrop before interpreting the display.
- [x] Query storage list once more immediately before deletion. This second
  result is the deletion source of truth.

**Gate:** The book is closed, the library is visibly active, capabilities are
correct, and the exact deletion set has been captured.

---

## 11. Task 8 - Delete Every Existing BinBook Safely

- [x] Extract every exact filename from the final pre-delete storage response
  whose name ends in `.binbook`. Preserve UTF-8 and spaces; pass each name as
  one quoted `--path` argument. Do not use unsafe whitespace-splitting `xargs`.
- [x] For each captured name, run:

  ```bash
  "$BIN" diag storage delete --port "$PORT" --path "$EXACT_NAME"
  ```

- [x] After each acknowledgement, continue only if response opcode/status are
  correct. Acknowledgement proves that request only; it does not prove the
  complete directory state.
- [x] Poll/re-list with bounded retries after the complete deletion pass,
  because each successful delete schedules an asynchronous library refresh:

  ```bash
  "$BIN" diag storage list --port "$PORT"
  ```

- [x] Require `entry_count=0`. If anything remains, compare against the
  original exact set and retry only the specific remaining `.binbook` entries.
  Do not upload while the list is nonempty.
- [x] Query logs/status and require zero new protocol errors and no unexplained
  storage error.

**Gate:** A fresh independent list proves the library contains zero BinBooks.

---

## 12. Task 9 - Upload, Read Back, Open, and Visually Verify

- [x] Record the local file byte length, CRC32 reported by the uploader, and
  SHA-256.
- [x] Upload the verified artifact:

  ```bash
  "$BIN" diag storage upload \
    --port "$PORT" \
    --path "$NAME" \
    --file "$BOOK"
  ```

- [x] Require upload begin/write/commit success and matching whole-file CRC32.
  If upload fails, use diagnostic abort/delete for only `$NAME`, independently
  re-list, then retry from the known local artifact. Do not regenerate the book
  during a transport retry.
- [x] Poll `storage list` with bounded retries until background refresh settles.
  Require exactly one entry named `$NAME` with the local byte size. Record but
  do not accept `pages=0` as page-count evidence; this is the current mounted
  list placeholder described in the starting-state section.
- [x] Read the entire stored file back:

  ```bash
  "$BIN" diag storage read \
    --port "$PORT" \
    --path "$NAME" \
    --output target/diag-evidence/wind-in-the-willows/device-roundtrip.binbook
  cmp "$BOOK" \
    target/diag-evidence/wind-in-the-willows/device-roundtrip.binbook
  sha256sum "$BOOK" \
    target/diag-evidence/wind-in-the-willows/device-roundtrip.binbook
  target/release/binbook inspect \
    target/diag-evidence/wind-in-the-willows/device-roundtrip.binbook \
    --validate --strict --json
  ```

- [x] Require `cmp` success, equal SHA-256 values, and zero strict-validation
  issues. This is the independent storage-integrity proof.
- [x] Ensure the library is displayed. With exactly one entry, inject `SELECT`.
  Retrieve logs and require the selected filename/rendition was opened and page
  0 displayed; do not infer this from the key acknowledgement.
- [x] Query STATUS independently. Require:
  - `current_page=0`;
  - `page_count` equals the host-inspected count;
  - `dropped_log_count=0`;
  - `protocol_error_count=0`;
  - `last_error=0`.
- [x] Take a new webcam capture, reveal its path to the user, inspect the same
  image, and require the real illustrated cover in correct X4 orientation.
  A locally decoded page is not live-device evidence.
- [x] Return to the library once and require the single filename is visible;
  reopen it and reconfirm page 0 to prove the post-refresh library index is
  usable, not merely the still-open upload path.
- [x] Finish with one final storage list and STATUS query.

**Gate:** The device contains exactly one byte-identical, strictly valid book;
the library opens it; queried state matches; and the webcam shows its cover.

---

## 13. Task 10 - Evidence, Adversarial Review, and Handoff

- [x] Update gitignored `HANDOFF.md` as a current-state snapshot, not a diary.
  Include:
  - source URL, source digest, and archive identity;
  - output size, page count, CRC32, and SHA-256;
  - peak RSS and elapsed compile time;
  - strict inspection output;
  - pre-delete, empty, post-upload, and final storage lists;
  - the known `StoreList.pages=0` limitation and the independent page-count
    evidence used instead;
  - upload commit output;
  - readback `cmp` and SHA-256 evidence;
  - open/log/STATUS evidence;
  - exact fresh webcam capture path and what is actually visible;
  - any remaining limitation or failure.
- [x] Keep `HANDOFF.md`, source EPUBs, BinBooks, decoded pages, contact sheets,
  and transcripts untracked.
- [x] Build an acceptance matrix matching every requirement below to its code
  path, automated test, and observed evidence.
- [x] Perform the mandatory adversarial completion review:
  - try the UTF-8 boundary with multiple scalar widths;
  - inspect image parsing for both `src` and namespaced `href`;
  - inspect panel arithmetic for odd width and no lost column;
  - inspect caption truncation for another zero-progress path;
  - verify anchor remapping after a two-page artwork expansion;
  - prove the device readback came from storage by comparing complete bytes;
  - prove the final list is fresh and contains exactly one entry;
  - prove the webcam file is a fresh capture from the configured live camera, not a decoded
    fixture.
- [x] Run final repository checks:

  ```bash
  rustup run nightly cargo fmt --all --check
  git diff --check
  git status --short
  git log -4 --oneline
  ```

- [x] If implementation created any uncommitted tracked change, test it and
  commit/push it intentionally before completion. Never hide work in a dirty
  tree.

---

## 14. Acceptance Matrix

No row may be marked complete without all three evidence columns.

| Requirement | Implementation path | Automated proof | Required observed proof |
| --- | --- | --- | --- |
| UTF-8 pagination always progresses | `binbook-render::pagination` | 2/3/4-byte boundary and full-content tests | Official EPUB completes without runaway memory |
| Peak compile memory is bounded for this book | page-local raster temporaries | focused allocation-shape tests where practical | GNU `time -v` RSS below 512 MiB |
| Real EPUB images are parsed | `binbook-epub` HTML/SVG conversion | normal img, figure, SVG wrapper tests | Cover and story art visible in decoded official output |
| Captions are real and not duplicated | typed image caption + figure collapse | caption association/text-absence tests | Portrait and landscape decoded pages show one intended caption per page |
| Portrait/square art uses one page | private artwork page renderer | decoded pixel-pattern test | Official portrait illustration inspected |
| Landscape art uses exact two-page partition | panel arithmetic/raster | odd-width left/right reconstruction test | Official flyleaf panels inspected left then right |
| No artwork is cropped | aspect containment and exact panel union | source-column coverage/aspect tests | Decoded official pages retain all visible source content |
| Navigation survives page expansion | layout-to-output page map | anchors before/after landscape test | Strict official output and usable navigation metadata |
| Unsupported image recovery is honest | `UnsupportedImage` fallback | warning + visible fallback test | No unsupported warning for the valid Gutenberg JPEG set |
| BinBook is valid X4 GRAY2 | compiler option propagation/encoder | compiler E2E strict tests | `inspect --validate --strict --json` succeeds |
| Device starts from empty BinBook set | diagnostic delete per listed name | storage-owner tests already in workspace | fresh `entry_count=0` response |
| Upload is complete | diagnostic upload/commit | existing protocol/storage tests | CRC-success commit plus one-entry name/size list |
| Stored bytes equal local artifact | diagnostic paginated read | existing read codec/transport tests | complete `cmp` and matching SHA-256 |
| Library opens the uploaded file | background refresh + reader open | existing reader/library tests | logs name the book; STATUS page 0/count matches |
| Final cover is visible on hardware | existing display pipeline | decoded local cover is only a precheck | fresh webcam capture of actual X4 cover |
| Final library contains exactly one book | storage list/background index | existing storage/list tests | final list contains only `the-wind-in-the-willows.binbook` |

## Completion Definition

This plan is complete only when:

1. the UTF-8 non-progress bug has a discriminating regression and is fixed;
2. the illustrated EPUB's cover and story art are real raster pages with the
   locked caption and no-crop policy;
3. all focused, workspace, serial-feature, Python, lint, format, and WASM gates
   pass;
4. the official illustrated eBook compiles within the 512 MiB RSS bound and
   strictly validates;
5. code/docs are committed and pushed with focused Conventional Commits;
6. a fresh device list proves all old BinBooks were removed;
7. complete device readback is byte-identical to the local artifact;
8. logs and STATUS prove the only library item opens at page 0 with the correct
   page count;
9. a fresh webcam capture visibly shows the real illustrated cover; and
10. `HANDOFF.md` and the acceptance matrix truthfully distinguish every
    verified result from any remaining limitation.

Observed completion evidence: the official 1,093,651-byte EPUB matched the
locked SHA-256 and contained 16 JPEG resources. Release compilation completed
in 17.96 seconds with 123,636 KiB maximum RSS. The strictly valid GRAY2 artifact
contains 323 pages and is 24,671,670 bytes. The sole device entry has that exact
name and size; complete optimized readback passed `cmp` and SHA-256. After a
Back-to-library cycle, the one-entry refreshed index reopened page 0/323,
`BOOK_OPENED` was logged, STATUS retained zero dropped logs/protocol errors and
`last_error=0`, and a fresh live webcam capture showed the illustrated cover.
