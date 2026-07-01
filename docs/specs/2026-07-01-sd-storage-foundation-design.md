# SD Storage Foundation Design

> **Location note:** This spec is authored under `.omo/specs/` because the
> planning agent is sandboxed to `.omo/*.md`. It belongs at
> `docs/specs/2026-07-01-sd-storage-foundation-design.md`. Move it before
> implementation.

## Status and authority

This is an approved design for Sub-project **A** of the SD-card library feature
(the first of three: **A** SD storage foundation → **B** diagnostic storage
extension → **C** library menu + reading flow). It covers the reusable crate
layer that mounts a FAT-formatted SD card over the Xteink X4 SPI bus and exposes
BinBook files as a `binbook-core` `ReadAt` source.

`BINBOOK_FORMAT_SPEC.md` remains authoritative for every BinBook wire-format
field. `docs/reference/squidscript-and-xteink-reference.md` is authoritative for
X4 pinout and bus facts. This design adds no BinBook format sections and changes
no format bytes.

Build order is **A → B → C**. A ships the storage crate; B ships the serial
commands that exercise it and produces the first hardware evidence; C ships the
visible menu.

## Hardware facts (from the reference doc)

The SD card shares **SPI2** with the display:

| Signal | GPIO | Notes |
|--------|------|-------|
| SCK    | GPIO8  | shared clock |
| MOSI   | GPIO10 | shared data out |
| MISO   | GPIO7  | SD card only (display is write-only) |
| SD CS  | GPIO12 | active-low |

SPI mode 0 (CPOL=0, CPHA=0), MSB-first. SD card clock starts at 400 kHz for
initialization and may rise after init. The display task currently owns SPI2 and
its staged refresh is timing-critical (SSD1677 staged streaming cannot be
interrupted mid-frame). This shared-bus contention is the top technical risk and
is owned by the board HAL, not by the storage crate.

## Goals

- Mount a FAT16/32-formatted SD card over SPI and read BinBook files from it,
  PC-mountable (drag `.binbook` files onto the card like a USB stick).
- Expose BinBook files as a `binbook-core` `ReadAt` so `Book<R: ReadAt>` reads
  pages straight off the card with zero whole-file buffering.
- Provide the layer as reusable, `no_std`, `embedded-hal`-based library crates
  that SquidScript and other firmware can consume (modularity rule).
- Keep all board-specific pin assignments and bus arbitration in `xteink-hal` /
  `binbook-fw`; library crates depend only on `embedded-hal` traits.
- Make all FAT/enumeration/ReadAt logic unit-testable on host without hardware,
  via a mock block device and a mock `Filesystem` trait.

## Non-goals (deferred)

- Writing to the card, upload/list/delete over serial, and any host CLI — these
  are Sub-project **B**. A delivers read + enumerate only.
- The library menu, reading-flow integration, and resume state — Sub-project **C**.
- A second storage backend on internal flash (LittleFS) — roadmap item, not built
  here. The backend-agnostic trait below anticipates it.
- "No card inserted" / "empty card" *display* behavior — Sub-project **C**.

## Crate architecture

Four layers. Only the first two are new; the latter two exist and are extended.

```text
embedded-sd-storage   (NEW, generic, non-binbook)
├── embedded-hal / embedded-hal-async   (SPI bus + OutputPin CS + Delay)
└── embedded-sdmmc                      (SPI-mode SD controller + FAT16/32)

binbook-storage        (NEW, binbook-specific, backend-agnostic)
├── binbook-core                        (ReadAt, header parsing)
└── storage trait defined here          (Filesystem/File abstraction)

xteink-hal             (exists)  + shared SPI2 bus mutex with display-gating
binbook-fw             (exists)  wires xteink-hal -> embedded-sd-storage
                                  -> binbook-storage -> binbook-core
```

### Layer 1 — `embedded-sd-storage` (new, generic)

A generic SPI-mode SD + FAT16/32 storage crate over `embedded-hal` traits. It
wraps `embedded-sdmmc` (the de-facto `no_std` SPI-mode SD + FAT crate; the
implementation plan must confirm it is current and `esp-hal`-compatible and pin a
version). It knows nothing about X4 pins or BinBook.

This crate is a standalone generic SD+FAT engine. It does **not** depend on
`binbook-storage` or know about the backend-agnostic trait (Layer 2); it only
provides raw operations that `binbook-fw` adapts.

Public surface (final signatures decided in the plan):

- `mount` / `init` over a borrowed SPI bus + CS pin + `Delay`.
- Open a file for read, enumerate directory entries incrementally, read bytes at
  an offset.

Naming follows the existing generic-crate convention (`ssd1677-driver`,
`gray2-render`): lowercase, no `binbook` prefix.

### Layer 2 — `binbook-storage` (new, binbook-specific, backend-agnostic)

The only BinBook-coupled storage layer. Depends on `binbook-core` and **defines**
a `Filesystem` trait here (open/read-at-offset/enumerate-directory), **not** a
dependency on `embedded-sd-storage` — so it works over any backend (SD FAT today;
internal-flash LittleFS later; future USB). `binbook-fw` writes the adapter
`impl Filesystem for ...` wrapping `embedded-sd-storage` (Layer 4). This is
exactly the seam SquidScript's `content.binbook.list` / `open` API wants, so it
can reuse this crate verbatim (writing its own adapter for its backend).

Public surface:

- `enumerate_binbooks(fs)` — list `.binbook` files in a directory, reading each
  candidate's BinBook **header only** to validate it is a real BinBook before
  listing. Returns an iterable/paginated result (the menu keeps a bounded name
  cache; the full sort/pagination policy is a C concern).
- `open(fs, name)` — open a BinBook file and return an SD-backed `ReadAt`.
- `list_entry` carries `{ name, file_size, page_count }` (header-only, cheap).
  Full title/author (`BookMetadata`) is a deeper read and is **not** returned by
  `enumerate_binbooks`; fetch on demand if a caller wants it.

### Layer 3 — `xteink-hal` (exists, extended)

Owns SPI2 and all GPIO pin assignments. Gains the **shared SPI2 bus mutex with
display-gating**: an arbiter that hands SPI2 to either the display
(`ssd1677-driver`) or the SD storage, guaranteeing SD access is only granted when
the display is not mid-refresh. The storage crate receives the bus + CS pin via
`embedded-hal` traits; the arbitration policy and pin wiring live here.

### Layer 4 — `binbook-fw` (exists, wiring)

Wires `xteink-hal` (shared bus) → `embedded-sd-storage` → `binbook-storage` →
`binbook-core`. The diagnostic console (Sub-project B) and the menu/reader
(Sub-project C) reach SD only through this wiring and the shared-bus arbiter.

## Design decisions (locked)

- **Filesystem:** FAT16/32 (PC-mountable). Books reach the card two ways:
  drag-and-drop on a computer, and (via B) serial diagnostic upload.
- **SPI bus strategy:** shared-bus mutex with display-gating (option chosen over
  funneling all SPI through the display task). The arbiter must be proven never
  to break an SSD1677 refresh.
- **Crate split:** generic `embedded-sd-storage` (SD+FAT) separated from
  binbook-specific `binbook-storage` (enumeration + `ReadAt`), per the explicit
  requirement that storage be reusable by any firmware code, not BinBook-coupled.
- **`nav_probe.binbook`** remains embedded via `include_bytes!` as a fallback for
  now; whether to drop it is decided in Sub-project C (the embedded-fallback menu
  entry).

## Data flow (reading a page off SD)

1. `binbook-fw` asks `binbook-storage::enumerate_binbooks(fs)` to list `.binbook`
   files. `binbook-storage` reads the FAT root directory via the `Filesystem`
   trait (provided by `embedded-sd-storage`) and validates each candidate header.
2. A caller opens a book: `binbook-storage::open(fs, name)` returns an SD-backed
   `ReadAt`.
3. `binbook-core`'s `Book<R: ReadAt>` reads the page index and compressed page
   blobs on demand.
4. Each blob read goes through `embedded-sd-storage` → SD block read → over the
   `xteink-hal`-arbitrated shared SPI2 bus.

## RAM discipline

Constrained-RAM rule applies (AGENTS.md). No whole-file buffering; FAT directory
entries read incrementally; bounded buffers only; every scratch buffer
caller-owned and explicitly sized. `embedded-sdmmc`'s internal sector cache is
the only persistent read buffer; its size is documented and bounded.

## Error handling

- No card inserted / card not FAT → `mount` returns a clear error. What the device
  *shows* is C's concern; A only reports the error.
- Corrupt BinBook header during enumeration → skip and flag; never crash the list.
- Read error mid-page → propagate as `FirmwareError::Storage` (already exists).
- Bus busy → the `xteink-hal` arbiter defers SD access until the display is idle
  (async `WouldBlock`/await semantics decided in the plan).

## Testing and verification

- **Host unit tests (no hardware):**
  - `embedded-sd-storage`: FAT open/read/enumerate against a **mock block device**
    seeded with a FAT image containing known `.binbook` files.
  - `binbook-storage`: enumeration + `ReadAt` logic against a **mock
    `Filesystem`** trait (no SD dependency).
- **Hardware evidence gate (lands at the A→B boundary):** A alone cannot produce
  hardware evidence without serial commands. Once B adds `storage upload/list/
  read`, the combined A+B gate proves upload → list → read-back is byte-identical
  to `binbook inspect` on the live card. See the B spec for the exact sequence.

A's host tests passing is necessary but not sufficient; the A→B boundary is the
first hardware-verified milestone.

## Open questions for the implementation plan

- Confirm `embedded-sdmmc` is current, `esp-hal`-compatible, and license-compatible;
  pin a version. Evaluate its shared-bus suitability and sector-cache size.
- Exact async shape of the shared-bus arbiter in `xteink-hal` (Embassy mutex vs.
  a dedicated arbiter task) and how SD reads await display-idle without busy-wait.
- Final public API signatures for `embedded-sd-storage` and `binbook-storage`.
