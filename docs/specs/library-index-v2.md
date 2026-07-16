# Library Index Version 2

The firmware's library catalog is an internal, card-local cache. It is not part
of the `.binbook` format. The authoritative content remains the files in the
`BOOKS` directory.

## Slots and migration

Version 2 uses `/BINBOOK/BINBOOK.A.INDEX` and
`/BINBOOK/BINBOOK.B.INDEX`. Firmware validates the slots sequentially and
selects the newest valid generation. If both are invalid or absent, it may read
the version-1 `/BINBOOK/BINBOOK.LIBRARY.INDEX` migration fallback. Once either
version-2 slot validates, the legacy file does not participate in selection.

The inactive slot is the only publication target. A candidate becomes eligible
for selection only after commit and a complete bounded readback validation.
An interrupted or malformed candidate therefore cannot replace the previously
selected slot.

## Binary layout

The header is 20 bytes:

| Offset | Size | Meaning |
|---:|---:|---|
| 0 | 4 | ASCII `BBLI` |
| 4 | 2 | little-endian version `2` |
| 6 | 2 | little-endian header size `20` |
| 8 | 2 | record count, `0..=200` |
| 10 | 2 | zero reserved field |
| 12 | 4 | nonzero little-endian generation |
| 16 | 4 | little-endian CRC32 |

The header is followed by exactly `record_count` fixed 139-byte records. Record
encoding and filename ordering are unchanged from version 1. The exact file
length is `20 + record_count * 139`.

CRC32 covers the header with bytes 16 through 19 treated as zero, followed by
all records in stored order. Validation checks the header, exact length, every
record, canonical ordering, duplicate filenames, and final CRC.

Generation increments with `wrapping_add(1).max(1)`. Generation `b` is newer
than `a` only when `b.wrapping_sub(a)` is nonzero and less than
`0x8000_0000`. Equal or exactly antipodal generations select slot A.

## Bounded refresh

Refresh scans at most 200 books into sorted runs of at most 15 entries. Runs are
stored in `/TMP/BBLIBRUN.TMP`; each spool record contains a transient two-byte
mount token and one persistent 139-byte record. The spool is never
authoritative and is deleted after success or failure.

A one-pass merge retains at most 14 run heads. Persistent records stream into
the inactive slot through the shared 4 KiB owner buffer. Overlength `.binbook`
names and entry 201 fail the refresh without changing the selected slot.

Runtime navigation retains a 15-entry `LibraryViewport` and a validated
`LibraryIndexCursor` containing the exact slot, generation, count, and header
size. Window reads never reselect a slot mid-session.
