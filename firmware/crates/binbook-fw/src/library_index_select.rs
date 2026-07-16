use crate::{
    library::{
        select_library_index_cursor, LibraryIndexCursor, LibraryIndexCursorValidator,
        LibraryIndexError, LibraryIndexSlot, INDEX_HEADER_SIZE, INDEX_RECORD_SIZE,
        INDEX_V2_HEADER_SIZE, MAX_LIBRARY_ENTRIES,
    },
    storage_access::{LIBRARY_INDEX_FILE, LIBRARY_INDEX_SLOT_A, LIBRARY_INDEX_SLOT_B},
    storage_owner::{
        BufferSlot, Request, RequestEnvelope, Response, ResponseEnvelope, StorageClient,
        StorageName,
    },
};

const RECORDS_PER_READ: usize = 29;
const READ_BYTES: usize = RECORDS_PER_READ * INDEX_RECORD_SIZE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryIndexSelectError {
    Correlation,
    MissingBuffer,
    AlreadyComplete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryIndexSelectUpdate {
    Request(RequestEnvelope),
    Complete(Option<LibraryIndexCursor>),
}

// The streamed validator is intentionally inline: boxing would require a heap,
// while the client owns only one phase at a time and remains bounded.
#[allow(clippy::large_enum_variant)]
enum Phase {
    Size(LibraryIndexSlot),
    Header {
        slot: LibraryIndexSlot,
        file_len: usize,
    },
    Records {
        slot: LibraryIndexSlot,
        cursor: LibraryIndexCursor,
        validator: LibraryIndexCursorValidator,
        file_len: usize,
        offset: usize,
        len: usize,
    },
    Complete,
}

pub struct LibraryIndexSelectClient {
    valid_a: Option<LibraryIndexCursor>,
    valid_b: Option<LibraryIndexCursor>,
    valid_legacy: Option<LibraryIndexCursor>,
    phase: Phase,
    expected_sequence: u32,
    next_sequence: u32,
}

impl LibraryIndexSelectClient {
    #[must_use]
    pub fn begin(sequence: u32) -> (Self, RequestEnvelope) {
        (
            Self {
                valid_a: None,
                valid_b: None,
                valid_legacy: None,
                phase: Phase::Size(LibraryIndexSlot::A),
                expected_sequence: sequence,
                next_sequence: sequence.wrapping_add(1),
            },
            envelope(
                sequence,
                Request::FileSize {
                    name: slot_name(LibraryIndexSlot::A),
                },
            ),
        )
    }

    pub fn accept(
        &mut self,
        response: ResponseEnvelope,
        buffer: Option<&[u8]>,
    ) -> Result<LibraryIndexSelectUpdate, LibraryIndexSelectError> {
        if response.client != StorageClient::Library || response.sequence != self.expected_sequence
        {
            self.phase = Phase::Complete;
            return Err(LibraryIndexSelectError::Correlation);
        }
        let phase = core::mem::replace(&mut self.phase, Phase::Complete);
        let Ok(response) = response.response else {
            return self.reject_current(phase);
        };
        match (phase, response) {
            (Phase::Size(slot), Response::FileSize(file_len)) => {
                let Ok(file_len) = usize::try_from(file_len) else {
                    return self.advance(slot);
                };
                let header_len = header_len(slot);
                if file_len < header_len
                    || file_len > header_len + MAX_LIBRARY_ENTRIES * INDEX_RECORD_SIZE
                    || !(file_len - header_len).is_multiple_of(INDEX_RECORD_SIZE)
                {
                    return self.advance(slot);
                }
                Ok(self.issue_read(slot, 0, header_len, Phase::Header { slot, file_len }))
            }
            (Phase::Header { slot, file_len }, Response::Read(actual))
                if usize::from(actual) == header_len(slot) =>
            {
                let bytes = buffer.ok_or(LibraryIndexSelectError::MissingBuffer)?;
                let cursor = match slot {
                    LibraryIndexSlot::Legacy => bytes
                        .get(..INDEX_HEADER_SIZE)
                        .and_then(|header| header.try_into().ok())
                        .ok_or(LibraryIndexError::Length)
                        .and_then(|header| LibraryIndexCursor::from_header(header, file_len)),
                    LibraryIndexSlot::A | LibraryIndexSlot::B => bytes
                        .get(..INDEX_V2_HEADER_SIZE)
                        .and_then(|header| header.try_into().ok())
                        .ok_or(LibraryIndexError::Length)
                        .and_then(|header| {
                            LibraryIndexCursor::from_v2_header(slot, header, file_len)
                        }),
                };
                let Ok(cursor) = cursor else {
                    return self.advance(slot);
                };
                let validator =
                    LibraryIndexCursorValidator::new(cursor, &bytes[..header_len(slot)]);
                if cursor.entry_count() == 0 {
                    if validator.finish().is_ok() {
                        self.record_valid(cursor);
                    }
                    self.advance(slot)
                } else {
                    let len = (file_len - header_len(slot)).min(READ_BYTES);
                    Ok(self.issue_read(
                        slot,
                        header_len(slot),
                        len,
                        Phase::Records {
                            slot,
                            cursor,
                            validator,
                            file_len,
                            offset: header_len(slot),
                            len,
                        },
                    ))
                }
            }
            (
                Phase::Records {
                    slot,
                    cursor,
                    mut validator,
                    file_len,
                    offset,
                    len,
                },
                Response::Read(actual),
            ) if usize::from(actual) == len => {
                let bytes = buffer.ok_or(LibraryIndexSelectError::MissingBuffer)?;
                let mut valid = bytes.len() >= len;
                if valid {
                    for record in bytes[..len].chunks_exact(INDEX_RECORD_SIZE) {
                        if validator.push_record(record.try_into().unwrap()).is_err() {
                            valid = false;
                            break;
                        }
                    }
                }
                if !valid {
                    return self.advance(slot);
                }
                let next = offset + len;
                if next == file_len {
                    if validator.finish().is_ok() {
                        self.record_valid(cursor);
                    }
                    self.advance(slot)
                } else {
                    let next_len = (file_len - next).min(READ_BYTES);
                    Ok(self.issue_read(
                        slot,
                        next,
                        next_len,
                        Phase::Records {
                            slot,
                            cursor,
                            validator,
                            file_len,
                            offset: next,
                            len: next_len,
                        },
                    ))
                }
            }
            (Phase::Complete, _) => Err(LibraryIndexSelectError::AlreadyComplete),
            (phase, _) => self.reject_current(phase),
        }
    }

    fn reject_current(
        &mut self,
        phase: Phase,
    ) -> Result<LibraryIndexSelectUpdate, LibraryIndexSelectError> {
        match phase {
            Phase::Size(slot) | Phase::Header { slot, .. } | Phase::Records { slot, .. } => {
                self.advance(slot)
            }
            Phase::Complete => Err(LibraryIndexSelectError::AlreadyComplete),
        }
    }

    fn record_valid(&mut self, cursor: LibraryIndexCursor) {
        match cursor.slot() {
            LibraryIndexSlot::A => self.valid_a = Some(cursor),
            LibraryIndexSlot::B => self.valid_b = Some(cursor),
            LibraryIndexSlot::Legacy => self.valid_legacy = Some(cursor),
        }
    }

    fn advance(
        &mut self,
        slot: LibraryIndexSlot,
    ) -> Result<LibraryIndexSelectUpdate, LibraryIndexSelectError> {
        let next = match slot {
            LibraryIndexSlot::A => Some(LibraryIndexSlot::B),
            LibraryIndexSlot::B if self.valid_a.is_none() && self.valid_b.is_none() => {
                Some(LibraryIndexSlot::Legacy)
            }
            LibraryIndexSlot::B => None,
            LibraryIndexSlot::Legacy => None,
        };
        if let Some(next) = next {
            Ok(self.issue(
                Request::FileSize {
                    name: slot_name(next),
                },
                Phase::Size(next),
            ))
        } else {
            self.phase = Phase::Complete;
            Ok(LibraryIndexSelectUpdate::Complete(
                select_library_index_cursor(self.valid_a, self.valid_b, self.valid_legacy),
            ))
        }
    }

    fn issue_read(
        &mut self,
        slot: LibraryIndexSlot,
        offset: usize,
        len: usize,
        phase: Phase,
    ) -> LibraryIndexSelectUpdate {
        self.issue(
            Request::read(
                slot_name(slot),
                offset as u64,
                len as u16,
                BufferSlot::LibraryIndex,
            )
            .expect("index reads are bounded"),
            phase,
        )
    }

    fn issue(&mut self, request: Request, phase: Phase) -> LibraryIndexSelectUpdate {
        let sequence = self.next_sequence;
        self.next_sequence = sequence.wrapping_add(1);
        self.expected_sequence = sequence;
        self.phase = phase;
        LibraryIndexSelectUpdate::Request(envelope(sequence, request))
    }
}

fn header_len(slot: LibraryIndexSlot) -> usize {
    match slot {
        LibraryIndexSlot::Legacy => INDEX_HEADER_SIZE,
        LibraryIndexSlot::A | LibraryIndexSlot::B => INDEX_V2_HEADER_SIZE,
    }
}

fn slot_name(slot: LibraryIndexSlot) -> StorageName {
    StorageName::new(match slot {
        LibraryIndexSlot::A => LIBRARY_INDEX_SLOT_A,
        LibraryIndexSlot::B => LIBRARY_INDEX_SLOT_B,
        LibraryIndexSlot::Legacy => LIBRARY_INDEX_FILE,
    })
    .expect("index names are bounded")
}

fn envelope(sequence: u32, request: Request) -> RequestEnvelope {
    RequestEnvelope {
        client: StorageClient::Library,
        sequence,
        request,
    }
}
