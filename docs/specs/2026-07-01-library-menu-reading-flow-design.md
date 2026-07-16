# Library Menu and Reading Flow Design

## Status and authority

This document describes the current Xteink X4 e-reader foundation. The BinBook
wire format remains authoritative in `BINBOOK_FORMAT_SPEC.md`; this feature adds
firmware state files but no BinBook sections.

This is the durable technical-decision record for the reader UX foundation.
Contributors should use it for product behavior, storage/cache policy, memory
bounds, refresh policy, state durability, power behavior, and module ownership.
The executable task order and acceptance procedure live in
`docs/plans/2026-07-13-binbook-ereader-poc-plan.md`. Current machine/device
state and live evidence live only in the gitignored `HANDOFF.md`.

## Decision summary

| Concern | Decision | Reason |
| --- | --- | --- |
| Library authority | Persist a sorted binary index on the SD card and replace it only after a complete successful scan. | Menu entry must not wait for a fresh directory scan, while failed scans must retain a usable library. |
| Menu memory | Keep a validated file-backed cursor and at most 15 records in the RAM viewport. | The menu must support 200 books without materializing 200 filenames at entry. |
| Open acceleration | Prefetch at most 15 mount-generation-scoped FAT sessions for exact viewport names. | Selected-book open must avoid a directory rescan without paying for a 200-session RAM cache. |
| Refresh timing | Display the cached viewport first and update the index cooperatively in the background. | Directory freshness must not be on the menu/open/back interaction path. |
| Cache durability | Treat the cursor, viewport, and FAT sessions as volatile; reconstruct them after reset/deep sleep from the SD index. | ESP32-C3 deep sleep does not preserve the working RAM cache reliably enough to make it persistent state. |
| Reading progress | Persist per-filename and per-rendition progress in CRC-protected A/B binary state slots. | Books resume independently and replacement content cannot inherit an invalid page. |
| Visible transitions | Use the accepted fast preview/cleanup differential recipe for menu navigation and menu↔book, retain the balanced page-turn policy, and reserve full refresh for a POWER double-tap. | Phase-level hardware evidence separates panel activation from SD, plane staging, composition, and state persistence instead of attributing the whole request to e-ink. |
| Storage concurrency | Give one task exclusive ownership of mounted FAT and expose bounded typed requests. | Multiple FAT owners are unsafe, while long synchronous scans block the single-core executor. |

## Library behavior

- Normal firmware enumerates `.binbook` files in `/BOOKS` on the SD card. The
  embedded navigation probe is available only in diagnostic builds without SD
  storage and is never presented as a normal library entry.
- The library supports at most 200 files and filenames up to
  `binbook_storage::MAX_BINBOOK_NAME_BYTES` (121 UTF-8 bytes). Exceeding either
  bound produces an explicit screen rather than a partial list.
- Filenames are ordered by ASCII case-folded bytes with original UTF-8 bytes as
  the deterministic tie-breaker. Non-ASCII bytes are not case-folded.
- The five-row viewport wraps at the ends and preserves its selected filename
  when returning from a book. Rows show filenames; metadata and covers remain
  future UX work.
- `No SD card`, `SD card error`, `No BinBooks`, too-many-books, and unsupported
  filename states are visible and recoverable. Select retries a failed scan.
- The X4 has no SD card-detect input. `No SD card` is therefore the software
  classification of an SD protocol `CardNotFound` result, not a physical
  insertion signal. Other SD device and FAT failures classify as `SD card
  error`. Failed scans retain the last valid index and never prune progress.
  Physical removal is optional board qualification, not proof of this complete
  error-classification contract; hot removal while reading is out of scope.

## Library index, cursor, and cache architecture

The production design does not materialize all 200 filenames when the menu is
entered. It separates three concerns that must not be collapsed into one large
RAM cache:

1. The persistent, sorted `/BINBOOK/BINBOOK.A.INDEX` and
   `/BINBOOK/BINBOOK.B.INDEX` slots are the card-local source for menu ordering.
   Version 2 has a 20-byte CRC-protected header with a nonzero generation,
   followed by fixed 139-byte records. The version-1
   `/BINBOOK/BINBOOK.LIBRARY.INDEX` is a read-only migration fallback. Records
   contain the complete filename, size, and filename hash. Mounted FAT handles
   and other process-local lookup tokens are never persisted. See
   `docs/specs/library-index-v2.md`.
2. A validated file-backed `LibraryIndexCursor` provides random record access.
   Firmware streams the complete index through CRC and structural validation
   before publishing it, then retains only a 15-entry `LibraryViewport`: five
   visible rows plus bounded look-ahead/look-behind prefetch. The measured
   viewport object is 2,304 bytes on the X4 target; menu RAM is therefore independent of the
   total book count.
3. A small bounded mounted-file-session prefetch/LRU accelerates opening the
   selected and nearby records. Sessions are scoped to the current mount and
   scan generation and are invalid after cancellation or replacement. Tokens
   remain reusable for that generation: when the active book changes, its file
   session returns to its indexed viewport slot instead of being discarded.
   They are an optimization only; persistent identity remains the filename and
   hashes in the index.

The persistent cache is therefore the transactional SD-card index, not a
200-book RAM data structure. Refresh uses a 15-entry external-sort workspace
and a non-authoritative scratch spool. The cursor, viewport, workspace, and
mounted sessions are bounded working RAM. Deep sleep/reset discards them; wake
remounts the card, selects and validates an exact slot, reconstructs the
viewport, and schedules a cooperative refresh.

The owner-path prototype proved the lookup optimization: retaining sessions for
all 200 books cost 17,312 bytes, and an earlier active-plus-pending version cost
33,312 bytes. A live 29-book run reduced selected-book descriptor lookup from
4,541 ms without a session to 205 ms with a cached session. Both large caches
have now been replaced by an explicit 15-name/15-session prefetch backend that
measures 4,560 bytes. Only names supplied from the sorted viewport receive
generation-scoped tokens; unrelated scan records remain indexable but do not
consume session RAM. The mounted refresh path submits the viewport names before
scanning. A physical A -> B -> A run measured metadata lookup at 206 ms, 205 ms,
and 206 ms respectively, with total opens of 1,139 ms, 1,140 ms, and 1,139 ms.
Before reusable session recycling, the final A metadata lookup fell back to
4,252 ms.

FAT provides no notification when a book is added or removed. The storage
owner therefore performs a cooperative scan at boot and opportunistically
after return-to-menu or during idle time. It processes at most eight raw
directory entries per step and yields between steps. A scan constructs a
replacement index and session generation off to the side; only a complete,
valid result may be published. Failure or cancellation retains the last valid
index and must not prune progress. Deterministic case-folded ordering must be
built with file-backed replacement records and bounded sort metadata/record
offsets, not a second full resident filename collection.

Select must never synchronously rescan `/BOOKS`. It normally opens the exact
selected record through an already-prefetched session. If that token is absent,
the owner performs a foreground exact-file lookup without running a directory
scan; the menu remains the persisted panel image until open succeeds or the
recoverable error screen is rendered. Book-to-menu likewise paints from the
validated index viewport immediately; any refresh scan continues afterward.

The cursor, viewport loader, mount-generation tokens, bounded session storage,
and full-session-cache performance probe are implemented and host-tested. The
normal `sd-storage` build has an interactive menu, bounded open/turn/back
path, owner-backed A/B reader-state load/save, per-rendition resume, visible
open-error recovery, and cancellable delayed grayscale refinement. Live
evidence covers cached and active-session opens, persisted nonzero-page resume,
page turns, Back, retry/Back from a bad book, and cancellation of an obsolete
gray refinement. Physical POWER hold, deep sleep, and wake restoration remain
mandatory acceptance gates; contributors must not describe the full POC as
complete until those outcomes are observed on the X4.

## Reading behavior

- Select opens the exact selected SD filename through the shared filesystem
  handle and renders its saved page or page 0. `Book::open` validates the
  structural records needed to construct the book; the X4 display path validates
  the profile and each rendered page.
- Left/Up move to the previous page and Right/Down move to the next page. Page
  boundaries clamp.
- Back or Select closes the active book and returns to the same library row.
- A page is committed only after the display engine reports a successful base
  render. Failed rendering does not advance in-memory or persisted progress.
- A failed open renders `CANNOT OPEN BOOK`, the selected filename, and retry/back
  guidance. The menu and serial diagnostic loop remain responsive.

## Graphics and refresh

The menu is drawn with `embedded-graphics` and `profont` into the reusable
480x800 packed-GRAY2 framebuffer in `xteink-x4-display`. Reader pages remain
streamed from BinBook data and do not use that framebuffer.

Routine menu navigation, menu-to-book, book-to-menu, and page turns use bounded
partial or differential updates whenever the previous visible panel state is
known. They must not invoke the roughly four-second full waveform merely to
change modes. There is no automatic periodic full-refresh cleanup. Two short
POWER taps, with releases at most 500 ms apart, explicitly request a full
cleanup refresh; a two-second continuous hold requests sleep. Fault recovery
must use a verified non-full-refresh path or retain the persisted image until
the user requests cleanup; unknown panel state does not authorize an automatic
full waveform. Book pages otherwise use the staged black/white base, grayscale
overlay, and background base-synchronization pipeline. Diagnostic KEY events
and physical buttons enter the same request queue.

Phase-level live measurements are the refresh-latency authority. Menu
navigation measures approximately 652 ms end to end: about 189 ms for each
full UI plane, 90 ms composition, and 184 ms accepted preview plus cleanup. A
physical press adds 40 ms debounce and up to one 20 ms polling interval.
Menu-to-book measures approximately 1,139 ms: 205–207 ms cached session/book
metadata, 5–6 ms page metadata, 189–194 ms previous UI plane, 549–553 ms target
book plane, and 184 ms activation. The target-book stage is dominated by SD
reads (about 360 ms), not decompression (about 27 ms). Book-to-menu uses the
CrossPoint-style Clean transition. The current live run completed the whole
Back display request in 2,169 ms; no storage write occurred inside that
request. Durable state persistence is scheduled after interaction has been
idle for two seconds.

Dirty-window and incremental-row menu prototypes were rejected. The rotated
window reduced each plane by only about 45 ms and produced ambiguous retained
highlight bands; incremental row composition was slower than byte-clearing and
redrawing the complete five-row framebuffer. Neither is part of the current
panel-state contract.

## Persistent reader state

State is card-local and stored transactionally in `/BINBOOK/BINBOOK.A.STATE`
and `/BINBOOK/BINBOOK.B.STATE`. The binary `BBST` version-1 format contains:

- a wrapping generation number;
- saved mode (`Menu` or `Reading`);
- selected and active filename hashes;
- the active 32-byte rendition hash; and
- up to 200 `{filename hash, rendition hash, page}` progress records.

The firmware selects the newest valid CRC-protected slot and falls back to the
other slot after a torn or corrupt write. Progress is independent per filename
and rendition. Replacing content under the same filename with a different
rendition starts at page 0. A successful library scan prunes deleted filenames;
a missing or unreadable card never prunes state.

Slot readback verification compares progress records semantically, independent
of insertion order. Encoding sorts records canonically, so decoded readback may
have a different vector order while representing identical state. Once verified,
the decoded canonical state replaces the RAM copy.

Page turns, selection changes, book open, and book close update the bounded
authoritative `ReaderState` in RAM and mark it dirty. A successful foreground
interaction postpones ordinary persistence until two seconds after the last
interaction. A failed idle write leaves RAM dirty and schedules a retry after
five seconds. Deep sleep is a durability barrier: it still requires a
synchronous, verified save and is cancelled if that save fails. Unexpected
power loss may lose changes since the last successful idle flush, but the
CRC-protected A/B slots preserve the prior committed generation.

After a state-slot upload is flushed and its exact size is checked, the mounted
owner converts that same write session into the read session used for verified
readback. It does not reopen the metadata directory. This preserves decode,
semantic comparison, canonical RAM replacement, and A/B generation ordering
while reducing live readback from 1,521 ms to 5 ms. In the current staged run,
the complete verified idle save took 581 ms, including a 424 ms initial phase,
40 ms write, 101 ms commit, and 5 ms readback. FAT create/open and flush remain
synchronous once an idle flush starts, so input that arrives during that
bounded operation queues until it finishes. Foreground input wins before an
idle flush starts and postpones it; this is idle scheduling, not preemptible FAT
I/O.

## Power lifecycle

GPIO3 is an active-low power input. A continuous two-second hold requests sleep;
one short press has no effect, while two short taps request explicit display
cleanup. After the display task finishes its active operation,
the firmware saves state, sends SSD1677 deep-sleep command `0x10`, waits for the
initiating button hold to be released, and enters ESP32-C3 deep sleep with GPIO3
low as the wake source.

The shared sleep-preparation coordinator permits panel sleep only after a
verified state save and permits CPU deep sleep only after panel sleep succeeds.
State-save failure returns a failed acknowledgement and renders `CANNOT SAVE /
SLEEP CANCELLED`; panel-sleep failure also returns a failed acknowledgement but
does not claim a recoverable panel state.

Every power request consumes exactly one display-task acknowledgement. A full
refresh acknowledgement never authorizes CPU sleep; only the acknowledgement
paired with the current successful sleep-preparation request does so.

Wake is reset-like: mount the SD card, validate the persistent index, schedule
a cooperative refresh scan, initialize the panel, and restore the saved menu
selection or active book/page. Missing or replaced books fall back to the
library.

## Module boundaries

- `binbook-core`: format parsing and rendition identity.
- `binbook-storage` and `embedded-sd-storage`: named random access and FAT I/O.
- `ssd1677-driver`: controller commands, including deep sleep.
- `xteink-x4-display`: framebuffer, panel wrapper, streaming page renderer, and
  display engine.
- `binbook-fw`: shared SD ownership, library/menu state, reader-state slots,
  physical input, application lifecycle, and diagnostics.

Contributor ownership within `binbook-fw` and the SD adapter is intentionally
more specific:

- `src/library.rs`: persistent index layout, deterministic records, cursor,
  viewport, filename identity, and ephemeral mount tokens.
- `src/library_index_load.rs`: bounded index validation/loading, including the
  viewport loader; its legacy full-library loader is a migration seam, not the
  target menu authority.
- `src/library_index_save.rs`: transactional index publication.
- `src/storage_owner.rs`: the single-owner request protocol, cooperative scan
  records, generation-scoped sessions/tokens, and active-file promotion.
- `src/owner_book.rs`: selected-book metadata/page clients over the owner.
- `src/owner_reader.rs`: owner-backed menu/open/render/turn lifecycle and the
  commit-after-success invariant; it contains no board or panel I/O.
- `src/runtime/mounted_reader_io.rs`: bounded owner request exchange and direct
  prepared-row streaming into SSD1677 RAM planes plus owner-state slot
  exchanges. It must not materialize a complete page or publish reader state
  before the panel operation succeeds.
- `src/runtime/mounted_display_task.rs`: board/runtime wiring for the bounded
  viewport, cached/active-session open, display commit, deferred-gray
  cancellation, visible error recovery, state persistence, explicit cleanup,
  and power preparation.
- `src/runtime/library_refresh_task.rs`: legacy probe wiring retained outside
  the normal mounted reader path.
- `embedded-sd-storage/src/sd_filesystem.rs`: mounted FAT scan and exact file
  sessions; it must remain independent of menu policy.

The reusable crates contain no board pin ownership or Embassy application
state. There must be exactly one FAT/block-device owner shared by reader, state,
index, and diagnostic operations. The mounted normal reader uses that owner
task for cursor loads, exact selected-file sessions, state slots, and bounded
page streaming. Typed requests transfer control and bounded records rather than
complete pages or the full library through channels.

An active upload is scheduler-affine to the client that began it until commit
or abort. Upload requests from other clients remain queued; they must not be
executed between chunks and rejected as `Busy`. This permits reader-state saves
to overlap background index publication without turning a successful Back or
sleep transition into an error.

## Verification requirements

Completion requires all of the following:

- host tests for 200-entry bounds, sorting, viewport behavior, state wire
  layout/CRC/generation/fault injection, rendition matching, exact SD reads,
  display commit semantics, power hold/release gating, and SSD1677 deep sleep;
- workspace and feature-gated firmware tests;
- pinned-nightly builds for base, SD, and diagnostic+SD firmware variants;
- a live X4 flash and at least 15 seconds of serial boot capture;
- diagnostic and webcam evidence for library navigation, exact selected-book
  open, page turns, independent per-book resume, recoverable open failure,
  physical sleep, and wake restoration.

Acknowledgements or successful command transport are not visual or state
evidence. Hardware results and any remaining unverified criteria belong in the
host-local `HANDOFF.md` acceptance matrix.
