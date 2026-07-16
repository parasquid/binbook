# BinBook Roadmap

## X4 page-turn latency optimization targets

Status: evidence-gated.

Current serial cadence diagnostics split queueing from display activation. `serial_accept_after_bw_busy_asserted` is the firmware-only proxy for "the next user key was accepted after the display activation started." `display_dequeue_after_bw_busy_asserted` is only a queue-drain metric and can pass while user keys are still arriving too early.

Baseline before SPI-clock testing used 20 MHz display SPI. A live serial cadence run measured firmware-side activation at `750 ms`, reliable completion/gray-safe cadence at `925 ms`, and `dequeue_to_bw_start` at about `394-396 ms`. That pre-refresh segment is dominated by two full-screen BW plane writes in `crates/xteink-x4-display/src/native.rs::render_bw_differential_timed`: previous page fast-base plane to red RAM, then target page fast-base plane to black RAM. Each plane write in `crates/xteink-x4-display/src/plane_write.rs` took about `196 ms` at 20 MHz, with about `122 ms` attributed to SPI transfer and about `19 ms` to row fill/decode.

40 MHz display SPI was tested by changing `firmware/crates/binbook-fw/src/main.rs::DISPLAY_SPI_FREQUENCY_MHZ` from `20` to `40` and flashing diagnostic firmware. The live run improved activation to `725 ms`, reliable completion to `900 ms`, and `dequeue_to_bw_start` to about `373-376 ms`. Per-plane SPI-write time dropped only from about `122 ms` to about `110 ms`, so future agents should not assume the SPI clock change halves transfer time. Keep 40 MHz only if repeated live runs and webcam verification remain stable.

Partial-window BUSY was tested with diagnostic firmware. The probe measured full screen `800x480`, half height `800x240`, wide stripe `800x64`, one strip `800x16`, centered box `200x100`, and page-number box `240x80`. All cases reported `472 ms` BUSY for all three samples. With the current SSD1677 partial-refresh command sequence, BUSY duration does not scale with the RAM window.

Full-window plane streaming was implemented for non-cancellable full-plane writes. Absolute grayscale, BW differential page turns, and recovery seeding now set one full-screen RAM window per controller plane and stream all 480 rows in one RAM write command. Cancellable staged overlay and background base sync remain strip-based so they can still stop before activation. A live cadence run measured `dequeue_to_bw_start` at about `255-259 ms`, activation threshold at `525 ms`, reliable completion/gray-safe cadence at `775 ms`, and `BW_BUSY_READY` at about `501 ms`.

Remaining targets, in recommended order:

Before starting any target below, reproduce the current baseline with `binbook diag exercise page-turn-cadence`, `scripts/analyze_cadence.py`, and webcam verification where the target affects visible output. Keep accepted-key metrics separate from queued/dequeued metrics: `serial_accept_after_bw_busy_asserted` is the closest firmware-only proxy for "next key accepted after display activation started"; `display_dequeue_after_bw_busy_asserted` is only a queue-drain metric.

1. One-plane update investigation. Current BW turns write previous fast-base to one controller RAM plane and target fast-base to the other. Investigate whether the previous plane can be preserved or pre-seeded across normal sequential turns so only the target plane needs to be written. This could save about one full plane write, but it is high risk because stale controller RAM can produce wrong transitions or ghosting. Acceptance: explicit controller RAM state logs, alternating forward/backward turn tests, boundary turns, recovery after failed/cancelled renders, and webcam verification across at least 20 consecutive turns.

2. Dirty-region writes for SPI/decode reduction only. Compare the previous and target fast-base data and skip unchanged regions where both controller RAM targets would remain equivalent. The partial-window BUSY matrix shows this should not be expected to reduce the post-refresh BUSY section, and full-window streaming removed most strip command overhead, so this is only worthwhile if real books have large unchanged regions. Acceptance: fixture tests with unchanged, partially changed, and fully changed pages; hardware logs showing fewer `PLANE_SPI_WRITE_SUMMARY` bytes; visual verification for page sequences with large white margins and dense text.

3. Timestamp precision cleanup. `REQUEST_RECEIVE` currently samples `now_ms` before the select branch completes, so receipt-to-dequeue deltas can be slightly negative at high gaps. Move timestamp sampling closer to event emission or add a monotonic event-order correction in the analyzer. Acceptance: synthetic analyzer tests for signed deltas and live logs showing no unexplained negative dequeue deltas.

Supporting work: row fill/decode optimization is not currently a primary target. Profile `PlaneDecoder::fill` and PackBits/LZ4 paths with realistic pages before changing buffer sizes or compression strategy. Acceptance requires before/after row-fill summaries from live logs and no increase in RAM footprint beyond documented constrained-device limits.

Do not optimize against `display_dequeue_after_bw_busy_asserted`; it measures queued work being processed later. Optimize against `serial_accept_after_bw_busy_asserted`, `dequeue_to_bw_start`, `PLANE_SPI_WRITE_SUMMARY`, `PLANE_ROW_FILL_SUMMARY`, `BW_BUSY_READY`, and webcam verification.

## Cursor-backed library catalog live qualification

Complete the evidence gate in
`docs/plans/2026-07-16-cursor-backed-library-catalog-ram-reduction.md` when the
X4 can acquire its SD card. Capture the exact starting catalog, navigate at
least 21 entries across multiple viewport boundaries, prove filename-hash
selection preservation across insertion and reboot, and verify that the
post-first-record fault image leaves the prior A/B slot authoritative. Restore
the exact starting catalog and leave the normal diagnostic image flashed.

## Strip-streamed display composition

Replace the persistent 96,000-byte `Gray2Framebuffer` with deterministic,
bounded strip composition where measurements prove that it preserves display
correctness and interaction latency. The X4 logical framebuffer is 480×800 at
two bits per pixel; it is convenient working state, not a requirement of the
SSD1677. A 8- or 16-row compositor should be able to render GRAY2 pixels,
derive the required controller-plane bytes, and stream them directly to panel
RAM using only a few KiB of caller-owned scratch.

Use SquidScript's current native Rust display work as a reference, while
distinguishing its two architectures. In `../SquidScript/platforms/esp/src/lib.rs`,
`CommandDisplaySink` retains a bounded source-order command snapshot and
`StreamingDisplayFlushTask`/`SnapshotPanelAdapter` generate SSD1677 rows without
retaining a full framebuffer. That ownership split is the relevant model:
application state produces a bounded immutable render description, and a
display-owned task repeatedly renders requested rows while streaming them to
the panel. SquidScript also retains `FramebufferDisplaySink`,
`FramebufferPanelAdapter`, and other `Gray2Framebuffer` paths; those demonstrate
a migration/reference implementation, not evidence that the framebuffer has
already been eliminated universally. Its own roadmap still calls for a fully
GRAY2-aware streaming compositor.

Do not copy CrossPoint's grayscale working-set strategy for this RAM-reduction
task. CrossPoint's proven waveform and plane mapping remain valuable references,
but its XTH path builds a full black/white base framebuffer and full grayscale
LSB/MSB planes across multiple passes. The BinBook target is row/strip plane
generation with bounded scratch, not several screen-sized buffers.

This is a display-pipeline redesign, not a mechanical buffer-size reduction:

1. Introduce a reusable `render_region(y_start, row_count, out)`-style contract
   for book content, menus, reader chrome, status overlays, orientation test
   pages, and recovery screens. Rendering the same region repeatedly from the
   same state must be byte-identical.
   Model source-order composition and flush ownership after SquidScript's
   bounded `CommandDisplaySink` snapshot rather than retaining pixels. Prove
   the command bound cannot silently drop required menu or reader operations.
2. Make BinBook plane decoding seek or advance incrementally without decoding a
   full page and without reparsing the entire page for every strip. Bound all
   decompression and row scratch explicitly.
3. Clip every UI primitive, glyph, highlight, ruler, and overlay to the
   requested strip. Add seam-sensitive fixtures whose glyphs and shapes cross
   strip boundaries.
4. Preserve differential-refresh correctness by providing both previous and
   target plane data. Evaluate deterministic re-rendering of previous state,
   incremental decoding of the previous page, and explicitly tracked SSD1677
   controller-RAM state; do not assume panel RAM contents remain valid after a
   cancelled write, reset, sleep, storage failure, or unknown-state recovery.
5. Preserve the refresh policy: ordinary navigation must not fall back to a
   full refresh, and firmware must not schedule automatic cleanup full
   refreshes. Cancellation must stop before activation when still safe and
   leave controller-state metadata consistent with what was actually written.
6. Reuse one caller-owned strip workspace across mutually exclusive display
   phases. Do not introduce separate previous, target, GRAY2, black-plane, and
   red-plane strip allocations unless a measured lifetime analysis proves they
   overlap and the linked-RAM budget permits them.
   Keep the row-fill callback or equivalent synchronous with the SSD1677 write
   so it cannot retain a borrowed strip or materialize a hidden full-frame
   future. SquidScript's `SnapshotPanelAdapter` is the behavioral reference for
   generating each requested row from an immutable command snapshot.
7. Remove `Gray2Framebuffer` from the production runtime only after all display
   paths use the regional contract. It may remain in host fixtures as a
   reference renderer for byte-for-byte comparisons.

Begin with failing host tests that compare full-frame reference output against
strip sizes 1, 8, 16, and non-divisors of 800 for representative book pages,
PackBits/LZ4 boundaries, menus, long filenames, selection changes, overlays,
corner/orientation fixtures, and previous/target differential planes. Add
fault-injection tests for cancellation and failures before, between, and after
plane writes. Profile decode work to prove strip rendering does not repeatedly
decompress or parse the complete page.
Run the same command snapshots through a full-frame reference renderer and the
streaming renderer, then compare all 96,000 canonical GRAY2 bytes and both
derived controller planes. Include more commands than the proposed snapshot
capacity so overflow is an explicit typed error rather than clipped output.

Acceptance requires approximately 90 KiB of linked-RAM reduction, no hidden
full-frame temporary in an Embassy future or FFI boundary, no regression beyond
a separately documented latency threshold derived from current live timing,
and live X4 evidence. The hardware run must cover menu navigation, menu-to-book,
forward/backward and boundary page turns, overlays, sleep/wake, cancellation,
recovery after an injected failed write, at least 20 alternating turns, and the
required orientation/calibration frame. Record activation latency, plane-write
bytes and duration, strip seams, clipping, stale pixels, ghosting, starting and
ending controller state, and fresh webcam evidence before declaring completion.

## Host-native firmware behavioral simulator

Create a deterministic host-native simulator for BinBook firmware behavior.
Do not begin by emulating the ESP32-C3 CPU or attempting to run the linked
RISC-V ELF unchanged: reproducing ESP32-C3 interrupts, Embassy timing, USB,
SPI, SD, ADC, GPIO, power, and the SSD1677 in a QEMU-style machine would be a
large separate project. Compile the reusable application and state-machine
logic for the host and provide explicit simulated hardware-owner adapters.

The simulator should compose these boundaries:

1. Scripted button presses/releases, monotonic virtual time, wake/reset events,
   and deterministic executor stepping.
2. A host-directory or in-memory implementation of the storage traits, with
   mount/media lifecycle, persistence across simulated reboot, latency control,
   and fault injection at reads, writes, flushes, commits, and media removal.
3. An in-process diagnostic serial transport that runs the production frame
   codecs and command dispatch rather than bypassing the protocol boundary.
4. A simulated SSD1677 that retains black and secondary/red controller RAM,
   window/cursor state, refresh requests, sleep/reset state, and whether the
   controller contents are known. Model only commands used by the production
   driver, but reject unsupported or invalid sequences explicitly.
5. A renderer that converts simulated controller RAM into idealized PNG output
   and records every activation, written region, plane, byte count, refresh
   mode, and controller-state transition. Preserve raw plane dumps so tests can
   distinguish a wrong conversion from a wrong panel write.
6. A scriptable scenario runner with commands such as input, advance time,
   attach/remove media, inject failure, reboot, upload, and capture. Assertions
   should cover runtime mode, page/library state, protocol response, storage
   state, refresh policy, plane hashes, exact pixels, and generated images.

Keep hardware-independent orchestration out of the firmware binary crate where
necessary so the simulator exercises the same state transitions, display
requests, storage contracts, and diagnostic dispatch as production. Do not
fork a second menu/reader implementation for the simulator. SquidScript's
host/browser adapters are useful structural references, but a canvas-only
renderer is insufficient: BinBook must model SSD1677 plane/window state to
detect swapped polarity, rotation/address errors, stale regions, partial-write
clipping, cancellation after a plane write, and unexpected full refreshes.

Develop test-first. Initial scenarios must cover boot with no card, empty and
multi-viewport libraries, menu navigation, open failure, first/open/resumed
page, forward/backward and boundary turns, menu-to-book/book-to-menu, state
persistence across reboot, upload publication, interrupted upload cleanup,
storage failure, sleep/wake, cancellation before activation, and recovery from
unknown controller state. Use the mandatory orientation/calibration frame and
navigation fixtures so rotation, mirroring, clipping, and stale pixels are
discriminating rather than inferred from a plausible screenshot.

For display conformance, retain a small set of reviewed golden PNGs, but prefer
structured plane/pixel assertions for most tests so harmless encoder metadata
does not create churn. Add differential tests that run the same render request
through the full-frame reference renderer and the strip-streamed renderer, and
compare complete controller planes and activation traces. Include mutation or
negative fixtures proving that swapped planes, one-pixel rotation errors,
skipped strips, forbidden full refreshes, and no-op commands fail the tests.

The simulator is a development and regression tool, not hardware evidence. It
cannot prove waveform quality, ghosting, physical contrast, BUSY timing, SPI
signal integrity, real USB behavior, ADC thresholds, power-loss behavior, or
visible activation latency. Firmware completion gates must continue to require
the authoritative live X4 runbook, serial capture, and fresh webcam inspection.
Acceptance for the simulator itself requires deterministic runs in CI, no
production behavior implemented only in simulator code, documented scenario
and artifact formats, representative end-to-end scenarios, and at least one
cross-check where simulator planes/screens are compared with a live device run.

## X4 Wi-Fi and BLE capability

Evaluate and integrate ESP32-C3 radio support behind explicit firmware feature gates. Start with Wi-Fi client capability behind a `wifi` feature, then add BLE behind a separate `ble` feature after Wi-Fi boot, connection, and idle-display behavior are stable. Do not enable either path in default firmware builds.

Use the current Espressif Rust radio stack that matches the pinned `esp-hal` generation, and keep radio ownership in `binbook-fw`; reusable BinBook, display, storage, and diagnostic protocol crates must not depend on Wi-Fi or BLE crates. If both Wi-Fi and BLE are needed at the same time, treat coexistence as a separate acceptance target rather than assuming single-radio evidence covers it.

Acceptance requires host tests for feature-gated command/configuration surfaces, firmware builds for no-radio, Wi-Fi-only, BLE-only, and coexistence configurations as applicable, live flash/serial evidence, measured RAM impact, measured idle and active power impact, and verification that display refresh, ADC input, SD/storage, and diagnostic USB behavior remain responsive while radio tasks are active.

## Single streaming compiler core for host and device

Refactor the existing host-oriented BinBook compiler into one streaming,
bounded-memory compiler core that is used unchanged for both host compilation
and future on-device EPUB-to-BinBook compilation. Do not create independent
"desktop" and "firmware" compilers. Platform-specific code may provide file,
SD, temporary-storage, scheduling, progress-reporting, and cancellation
adapters, but EPUB interpretation, layout, page rendering, compression, index
generation, policy/rendition hashes, and BinBook wire output must have one
implementation so the two targets cannot drift.

Current context for a cold-starting agent:

- `crates/binbook-compiler` coordinates EPUB ingestion, rendering, image
  processing, and encoding.
- `crates/binbook-epub` currently uses host-oriented EPUB/HTML/CSS dependencies,
  including `zip`, `rbook`, `html5ever`, and `markup5ever_rcdom`.
- `crates/binbook-render` currently uses `cosmic-text`, and
  `crates/binbook-image` currently uses host-oriented image and SVG libraries.
- `crates/binbook-encode/src/writer.rs::BookBuilder` retains every
  `CompiledPage` in a `Vec`, then `build_bytes()` assembles the complete output
  in another `Vec<u8>`.
- `crates/binbook-encode/src/indices.rs::build_pages` separately accumulates
  the complete page index, chunk index, transition index, and `PAGE_DATA` in
  memory. This architecture cannot be copied directly into the constrained
  ESP32-C3 firmware.
- CrossPoint demonstrates that EPUB processing on the Xteink X4 is feasible
  when chapter/intermediate data is aggressively cached to SD. Treat that as
  evidence for an SD-backed incremental architecture, not as evidence that the
  existing host compiler or its dependency graph is firmware-ready.
- `BINBOOK_FORMAT_SPEC.md` remains authoritative. Its seekable header, section
  table, relative page-data offsets, and independent sections permit a writer
  to reserve/backpatch metadata and spool page data and indexes without holding
  the whole output in RAM.

Required architecture:

1. Define compiler-owned traits for sequential input, seekable output,
   temporary/spooled storage, caller-owned scratch buffers, progress events,
   cancellation, and resource limits. Host filesystem and firmware SD/FAT
   implementations must be adapters to these traits; reusable compiler crates
   must not depend on Embassy, ESP HAL types, or board GPIO details.
2. Replace whole-book DOM and output accumulation with bounded EPUB entry
   processing and bounded chapter/layout state. Explicitly cap ZIP entries,
   CSS/markup complexity, decoded image dimensions, font resources, and every
   scratch allocation. Oversized or unsupported content must return a typed,
   recoverable error rather than exhausting RAM.
3. Render and compress one page, or another explicitly measured bounded unit,
   at a time. Immediately spool compressed plane blobs and page/chunk records
   to temporary storage, then release the page buffers before continuing.
4. Build navigation, strings, page indexes, chunk indexes, transition indexes,
   and `PAGE_DATA` through bounded records or SD-backed spools. Reserve and
   backpatch the final header and section table only after final offsets,
   lengths, counts, CRCs, and hashes are known.
5. Preserve deterministic output. Given the same source bytes, compiler
   configuration, fonts, and target profile, host and device adapters must
   produce byte-identical `.binbook` files. A smaller device-supported EPUB
   feature profile is acceptable only as an explicit configuration of the same
   core; it must not fork parsing, layout, or encoding behavior.
6. Publish on-device results transactionally: keep the uploaded EPUB, compile
   under a temporary name/work directory, strictly validate the completed
   BinBook, then atomically publish it into `/BOOKS`. Cancellation, power loss,
   insufficient space, malformed EPUBs, and unsupported content must leave no
   library-visible partial BinBook and must preserve the last valid output.
7. Migrate incrementally. First make the host compiler use the streaming core
   without changing output bytes. Then add constrained-memory host tests and
   fault-injected spool tests. Only after those pass should firmware SD adapters
   and an on-device upload/compile UX be added.

Acceptance is evidence-gated:

- Existing host compiler fixtures remain byte-for-byte identical unless an
  intentional format/spec change is separately approved.
- Tests run the same corpus through host and device-style adapters and compare
  complete BinBook bytes, strict-validation results, hashes, indexes, page
  pixels, and errors.
- Peak live memory is measured with adversarial large books, chapters, CSS,
  images, fonts, and compressed payloads; memory use must remain within a
  documented bound independent of total book size.
- Tests inject failures at EPUB reads, every temporary-spool boundary, page
  rendering/compression, final copying, backpatching, flushing, validation, and
  publication, proving recovery and cleanup behavior.
- The firmware target builds without pulling host-only `std` dependencies into
  reusable device compiler crates.
- Completion requires a real Xteink X4 run that uploads an EPUB, compiles it on
  SD, validates and opens the resulting BinBook, reboots/resumes it, and proves
  via serial evidence and visible display inspection that no host-side compiler
  participated.

## Python authoring package modularization

Split binary format models/writing, EPUB ingestion, raster rendering, viewer, and kerning-proof server into independently testable modules. Define an explicit public package API, add basedpyright strict checking and Ruff gates, and preserve existing CLI output and BinBook bytes.

## Internal Flash LittleFS

Evaluate `littlefs2` on the existing 192 KB internal flash region for storing a small number of binbooks (tens of KB each). FAT is unsuitable for internal flash due to sector-erase wear, so this would use a wear-leveling filesystem (LittleFS) as a separate path.

- **Depends on:** completion of A (storage abstraction) — LittleFS would implement the same `Filesystem` trait from `binbook-storage`.
- **Hardware:** same internal flash already backing `FlashStorage` (0x40E90000, 192 KB).
- **Gate:** requires LittleFS on-device writes verified before adoption; read-only use of LittleFS is strictly out of scope.
