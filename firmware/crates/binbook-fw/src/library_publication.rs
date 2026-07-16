use crate::{
    library::{
        encode_index_entry, encode_v2_header_fields, LibraryIndexCursor,
        LibraryIndexCursorValidator, LibraryIndexError, LibraryIndexSlot, LibraryStatus,
        MountFileToken, INDEX_RECORD_SIZE, INDEX_V2_HEADER_SIZE,
    },
    library_external_sort::{ExternalSortError, LibraryRefreshWorkspace, SPOOL_RECORD_SIZE},
    storage_access::{LIBRARY_INDEX_SLOT_A, LIBRARY_INDEX_SLOT_B},
    storage_owner::{
        BufferSlot, Request, RequestEnvelope, Response, ResponseEnvelope, StorageClient,
        StorageName,
    },
};

const VALIDATION_RECORDS_PER_READ: usize = 29;
const VALIDATION_READ_BYTES: usize = VALIDATION_RECORDS_PER_READ * INDEX_RECORD_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparedMount {
    pub absolute_index: u16,
    pub token: MountFileToken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryPublication {
    pub status: LibraryStatus,
    pub cursor: Option<LibraryIndexCursor>,
    pub book_count: u16,
    pub prepared: heapless::Vec<PreparedMount, 15>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryPublicationError {
    Correlation,
    Completion,
    MissingBuffer,
    Codec(LibraryIndexError),
    Sort(ExternalSortError),
    PreparedOverflow,
    InjectedFault,
    AlreadyComplete,
}

impl LibraryPublicationError {
    pub const fn code(&self) -> u32 {
        match self {
            Self::Correlation => 1,
            Self::Completion => 2,
            Self::MissingBuffer => 3,
            Self::Codec(_) => 4,
            Self::Sort(_) => 5,
            Self::PreparedOverflow => 6,
            Self::InjectedFault => 7,
            Self::AlreadyComplete => 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryPublicationUpdate {
    Request(RequestEnvelope),
    Complete(LibraryPublication),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryPublicationPhase {
    Begin,
    ProvisionalHeader,
    InitialHead,
    Replacement,
    RecordWrite,
    FinalHeader,
    Commit,
    ValidateSize,
    ValidateHeader,
    ValidateRecords,
    ScratchDelete,
    AbortThenDelete,
    CleanupDelete,
    Complete,
}

impl LibraryPublicationPhase {
    pub const fn code(self) -> u8 {
        match self {
            Self::Begin => 1,
            Self::ProvisionalHeader => 2,
            Self::InitialHead => 3,
            Self::Replacement => 4,
            Self::RecordWrite => 5,
            Self::FinalHeader => 6,
            Self::Commit => 7,
            Self::ValidateSize => 8,
            Self::ValidateHeader => 9,
            Self::ValidateRecords => 10,
            Self::ScratchDelete => 11,
            Self::AbortThenDelete => 12,
            Self::CleanupDelete => 13,
            Self::Complete => 14,
        }
    }
}

enum Phase {
    Begin,
    ProvisionalHeader {
        upload_id: u32,
    },
    InitialHead {
        upload_id: u32,
    },
    Replacement {
        upload_id: u32,
        head: u8,
    },
    RecordWrite {
        upload_id: u32,
    },
    FinalHeader {
        upload_id: u32,
    },
    Commit {
        upload_id: u32,
    },
    ValidateSize,
    ValidateHeader {
        file_len: usize,
    },
    ValidateRecords {
        cursor: LibraryIndexCursor,
        validator: LibraryIndexCursorValidator,
        offset: usize,
        len: usize,
    },
    ScratchDelete {
        cursor: LibraryIndexCursor,
    },
    AbortThenDelete,
    CleanupDelete,
    Complete,
}

pub struct LibraryPublicationClient<'a> {
    workspace: &'a mut LibraryRefreshWorkspace,
    slot: LibraryIndexSlot,
    generation: u32,
    count: usize,
    emitted: usize,
    crc_state: u32,
    prepared: heapless::Vec<PreparedMount, 15>,
    phase: Phase,
    expected_sequence: u32,
    next_sequence: u32,
}

impl<'a> LibraryPublicationClient<'a> {
    pub const fn request_needs_buffer(request: &Request) -> bool {
        matches!(
            request,
            Request::UploadBegin { .. }
                | Request::UploadWrite { .. }
                | Request::LibraryScratchRead { .. }
                | Request::Read { .. }
        )
    }

    pub const fn phase(&self) -> LibraryPublicationPhase {
        match &self.phase {
            Phase::Begin => LibraryPublicationPhase::Begin,
            Phase::ProvisionalHeader { .. } => LibraryPublicationPhase::ProvisionalHeader,
            Phase::InitialHead { .. } => LibraryPublicationPhase::InitialHead,
            Phase::Replacement { .. } => LibraryPublicationPhase::Replacement,
            Phase::RecordWrite { .. } => LibraryPublicationPhase::RecordWrite,
            Phase::FinalHeader { .. } => LibraryPublicationPhase::FinalHeader,
            Phase::Commit { .. } => LibraryPublicationPhase::Commit,
            Phase::ValidateSize => LibraryPublicationPhase::ValidateSize,
            Phase::ValidateHeader { .. } => LibraryPublicationPhase::ValidateHeader,
            Phase::ValidateRecords { .. } => LibraryPublicationPhase::ValidateRecords,
            Phase::ScratchDelete { .. } => LibraryPublicationPhase::ScratchDelete,
            Phase::AbortThenDelete => LibraryPublicationPhase::AbortThenDelete,
            Phase::CleanupDelete => LibraryPublicationPhase::CleanupDelete,
            Phase::Complete => LibraryPublicationPhase::Complete,
        }
    }

    pub fn begin(
        workspace: &'a mut LibraryRefreshWorkspace,
        slot: LibraryIndexSlot,
        generation: u32,
        sequence: u32,
    ) -> Result<(Self, RequestEnvelope), LibraryPublicationError> {
        if slot == LibraryIndexSlot::Legacy || generation == 0 || workspace.buffered_count() != 0 {
            return Err(LibraryPublicationError::Codec(
                LibraryIndexError::Generation,
            ));
        }
        let count = workspace.entry_count();
        let name = slot_name(slot);
        Ok((
            Self {
                workspace,
                slot,
                generation,
                count,
                emitted: 0,
                crc_state: u32::MAX,
                prepared: heapless::Vec::new(),
                phase: Phase::Begin,
                expected_sequence: sequence,
                next_sequence: sequence.wrapping_add(1),
            },
            envelope(
                sequence,
                Request::UploadBegin {
                    name,
                    expected_size: (INDEX_V2_HEADER_SIZE + count * INDEX_RECORD_SIZE) as u64,
                    expected_crc32: 0,
                    power_save: false,
                },
            ),
        ))
    }

    pub fn accept(
        &mut self,
        response: ResponseEnvelope,
        buffer: Option<&mut [u8]>,
    ) -> Result<LibraryPublicationUpdate, LibraryPublicationError> {
        if response.client != StorageClient::Library || response.sequence != self.expected_sequence
        {
            self.phase = Phase::Complete;
            return Err(LibraryPublicationError::Correlation);
        }
        let response = response
            .response
            .map_err(|_| LibraryPublicationError::Completion)?;
        match (
            core::mem::replace(&mut self.phase, Phase::Complete),
            response,
        ) {
            (Phase::Begin, Response::UploadStarted(upload_id)) => {
                self.phase = Phase::ProvisionalHeader { upload_id };
                let buffer = required_buffer(buffer, INDEX_V2_HEADER_SIZE)?;
                let header: &mut [u8; INDEX_V2_HEADER_SIZE] =
                    (&mut buffer[..INDEX_V2_HEADER_SIZE]).try_into().unwrap();
                encode_v2_header_fields(self.count, self.generation, 0, header)
                    .map_err(LibraryPublicationError::Codec)?;
                self.crc_state = update_crc(u32::MAX, header);
                Ok(self.issue_upload_write(
                    upload_id,
                    0,
                    INDEX_V2_HEADER_SIZE as u16,
                    Phase::ProvisionalHeader { upload_id },
                ))
            }
            (Phase::ProvisionalHeader { upload_id }, Response::Written(len))
                if usize::from(len) == INDEX_V2_HEADER_SIZE =>
            {
                self.workspace
                    .start_merge()
                    .map_err(LibraryPublicationError::Sort)?;
                self.request_initial_or_emit(upload_id, buffer)
            }
            (Phase::InitialHead { upload_id }, Response::Read(len))
                if usize::from(len) == SPOOL_RECORD_SIZE =>
            {
                let buffer = required_buffer(buffer, SPOOL_RECORD_SIZE)?;
                self.workspace
                    .accept_initial_head(buffer[..SPOOL_RECORD_SIZE].try_into().unwrap())
                    .map_err(LibraryPublicationError::Sort)?;
                self.request_initial_or_emit(upload_id, Some(buffer))
            }
            (Phase::Replacement { upload_id, head }, Response::Read(len))
                if usize::from(len) == SPOOL_RECORD_SIZE =>
            {
                let buffer = required_buffer(buffer, INDEX_RECORD_SIZE + SPOOL_RECORD_SIZE)?;
                let replacement: &[u8; SPOOL_RECORD_SIZE] = buffer
                    [INDEX_RECORD_SIZE..INDEX_RECORD_SIZE + SPOOL_RECORD_SIZE]
                    .try_into()
                    .unwrap();
                self.workspace
                    .advance_head(usize::from(head), Some(replacement))
                    .map_err(LibraryPublicationError::Sort)?;
                Ok(self.issue_record_write(upload_id))
            }
            (Phase::RecordWrite { upload_id }, Response::Written(len))
                if usize::from(len) == INDEX_RECORD_SIZE =>
            {
                self.emitted += 1;
                #[cfg(feature = "library-index-fault-after-first-record")]
                if self.emitted == 1 {
                    self.phase = Phase::RecordWrite { upload_id };
                    return Err(LibraryPublicationError::InjectedFault);
                }
                self.emit_next_or_finish(upload_id, buffer)
            }
            (Phase::FinalHeader { upload_id }, Response::Written(len))
                if usize::from(len) == INDEX_V2_HEADER_SIZE =>
            {
                Ok(self.issue(
                    Request::upload_commit(upload_id, false),
                    Phase::Commit { upload_id },
                ))
            }
            (Phase::Commit { .. }, Response::Ack) => Ok(self.issue(
                Request::FileSize {
                    name: slot_name(self.slot),
                },
                Phase::ValidateSize,
            )),
            (Phase::ValidateSize, Response::FileSize(file_len)) => {
                let file_len = usize::try_from(file_len)
                    .map_err(|_| LibraryPublicationError::Codec(LibraryIndexError::Length))?;
                if file_len != INDEX_V2_HEADER_SIZE + self.count * INDEX_RECORD_SIZE {
                    return Err(LibraryPublicationError::Codec(LibraryIndexError::Length));
                }
                Ok(self.issue_read(
                    0,
                    INDEX_V2_HEADER_SIZE,
                    0,
                    Phase::ValidateHeader { file_len },
                ))
            }
            (Phase::ValidateHeader { file_len }, Response::Read(len))
                if usize::from(len) == INDEX_V2_HEADER_SIZE =>
            {
                let buffer = required_buffer(buffer, INDEX_V2_HEADER_SIZE)?;
                let header: &[u8; INDEX_V2_HEADER_SIZE] =
                    buffer[..INDEX_V2_HEADER_SIZE].try_into().unwrap();
                let cursor = LibraryIndexCursor::from_v2_header(self.slot, header, file_len)
                    .map_err(LibraryPublicationError::Codec)?;
                let validator = LibraryIndexCursorValidator::new(cursor, header);
                if self.count == 0 {
                    validator.finish().map_err(LibraryPublicationError::Codec)?;
                    Ok(self.issue(
                        Request::LibraryScratchDelete,
                        Phase::ScratchDelete { cursor },
                    ))
                } else {
                    let len = (file_len - INDEX_V2_HEADER_SIZE).min(VALIDATION_READ_BYTES);
                    Ok(self.issue_read(
                        INDEX_V2_HEADER_SIZE,
                        len,
                        0,
                        Phase::ValidateRecords {
                            cursor,
                            validator,
                            offset: INDEX_V2_HEADER_SIZE,
                            len,
                        },
                    ))
                }
            }
            (
                Phase::ValidateRecords {
                    cursor,
                    mut validator,
                    offset,
                    len,
                },
                Response::Read(actual),
            ) if usize::from(actual) == len => {
                let buffer = required_buffer(buffer, len)?;
                for record in buffer[..len].chunks_exact(INDEX_RECORD_SIZE) {
                    validator
                        .push_record(record.try_into().unwrap())
                        .map_err(LibraryPublicationError::Codec)?;
                }
                let next = offset + len;
                let file_len = INDEX_V2_HEADER_SIZE + self.count * INDEX_RECORD_SIZE;
                if next == file_len {
                    validator.finish().map_err(LibraryPublicationError::Codec)?;
                    Ok(self.issue(
                        Request::LibraryScratchDelete,
                        Phase::ScratchDelete { cursor },
                    ))
                } else {
                    let next_len = (file_len - next).min(VALIDATION_READ_BYTES);
                    Ok(self.issue_read(
                        next,
                        next_len,
                        0,
                        Phase::ValidateRecords {
                            cursor,
                            validator,
                            offset: next,
                            len: next_len,
                        },
                    ))
                }
            }
            (Phase::ScratchDelete { cursor }, Response::Ack) => {
                self.phase = Phase::Complete;
                Ok(LibraryPublicationUpdate::Complete(LibraryPublication {
                    status: if self.count == 0 {
                        LibraryStatus::NoBooks
                    } else {
                        LibraryStatus::Ready
                    },
                    cursor: Some(cursor),
                    book_count: self.count as u16,
                    prepared: core::mem::take(&mut self.prepared),
                }))
            }
            (Phase::AbortThenDelete, Response::Ack) => {
                Ok(self.issue(Request::LibraryScratchDelete, Phase::CleanupDelete))
            }
            (Phase::CleanupDelete, Response::Ack) => {
                self.phase = Phase::Complete;
                Err(LibraryPublicationError::Completion)
            }
            (Phase::Complete, _) => Err(LibraryPublicationError::AlreadyComplete),
            (phase, _) => {
                self.phase = phase;
                Err(LibraryPublicationError::Completion)
            }
        }
    }

    pub fn cleanup(&mut self) -> Option<RequestEnvelope> {
        let upload_id = match &self.phase {
            Phase::ProvisionalHeader { upload_id }
            | Phase::InitialHead { upload_id }
            | Phase::Replacement { upload_id, .. }
            | Phase::RecordWrite { upload_id }
            | Phase::FinalHeader { upload_id }
            | Phase::Commit { upload_id } => Some(*upload_id),
            _ => None,
        };
        if let Some(upload_id) = upload_id {
            return Some(
                match self.issue(Request::UploadAbort { upload_id }, Phase::AbortThenDelete) {
                    LibraryPublicationUpdate::Request(request) => request,
                    LibraryPublicationUpdate::Complete(_) => unreachable!(),
                },
            );
        }
        if matches!(&self.phase, Phase::Complete | Phase::CleanupDelete) {
            return None;
        }
        Some(
            match self.issue(Request::LibraryScratchDelete, Phase::CleanupDelete) {
                LibraryPublicationUpdate::Request(request) => request,
                LibraryPublicationUpdate::Complete(_) => unreachable!(),
            },
        )
    }

    fn request_initial_or_emit(
        &mut self,
        upload_id: u32,
        buffer: Option<&mut [u8]>,
    ) -> Result<LibraryPublicationUpdate, LibraryPublicationError> {
        if let Some(record) = self.workspace.next_initial_head_record() {
            Ok(self.issue_scratch_read(record, 0, Phase::InitialHead { upload_id }))
        } else {
            self.emit_next_or_finish(upload_id, buffer)
        }
    }

    fn emit_next_or_finish(
        &mut self,
        upload_id: u32,
        buffer: Option<&mut [u8]>,
    ) -> Result<LibraryPublicationUpdate, LibraryPublicationError> {
        let Some(head) = self.workspace.minimum_head() else {
            let buffer = required_buffer(buffer, INDEX_V2_HEADER_SIZE)?;
            let crc = !self.crc_state;
            encode_v2_header_fields(
                self.count,
                self.generation,
                crc,
                (&mut buffer[..INDEX_V2_HEADER_SIZE]).try_into().unwrap(),
            )
            .map_err(LibraryPublicationError::Codec)?;
            return Ok(self.issue_upload_write(
                upload_id,
                0,
                INDEX_V2_HEADER_SIZE as u16,
                Phase::FinalHeader { upload_id },
            ));
        };
        let selected = self
            .workspace
            .selected_head(head)
            .map_err(LibraryPublicationError::Sort)?;
        if let Some(token) = selected.mount_token {
            self.prepared
                .push(PreparedMount {
                    absolute_index: self.emitted as u16,
                    token,
                })
                .map_err(|_| LibraryPublicationError::PreparedOverflow)?;
        }
        let buffer = required_buffer(buffer, INDEX_RECORD_SIZE + SPOOL_RECORD_SIZE)?;
        encode_index_entry(
            &selected.entry,
            (&mut buffer[..INDEX_RECORD_SIZE]).try_into().unwrap(),
        );
        self.crc_state = update_crc(self.crc_state, &buffer[..INDEX_RECORD_SIZE]);
        if let Some(record) = self.workspace.replacement_record(head) {
            Ok(self.issue_scratch_read(
                record,
                INDEX_RECORD_SIZE,
                Phase::Replacement {
                    upload_id,
                    head: head as u8,
                },
            ))
        } else {
            self.workspace
                .advance_head(head, None)
                .map_err(LibraryPublicationError::Sort)?;
            Ok(self.issue_record_write(upload_id))
        }
    }

    fn issue_record_write(&mut self, upload_id: u32) -> LibraryPublicationUpdate {
        self.issue_upload_write(
            upload_id,
            (INDEX_V2_HEADER_SIZE + self.emitted * INDEX_RECORD_SIZE) as u64,
            INDEX_RECORD_SIZE as u16,
            Phase::RecordWrite { upload_id },
        )
    }

    fn issue_upload_write(
        &mut self,
        upload_id: u32,
        offset: u64,
        len: u16,
        phase: Phase,
    ) -> LibraryPublicationUpdate {
        self.issue(
            Request::UploadWrite {
                upload_id,
                offset,
                buffer_offset: 0,
                len,
                buffer: BufferSlot::LibraryIndex,
                power_save: false,
            },
            phase,
        )
    }

    fn issue_scratch_read(
        &mut self,
        record: u16,
        buffer_offset: usize,
        phase: Phase,
    ) -> LibraryPublicationUpdate {
        self.issue(
            Request::LibraryScratchRead {
                offset: u64::from(record) * SPOOL_RECORD_SIZE as u64,
                buffer_offset: buffer_offset as u16,
                len: SPOOL_RECORD_SIZE as u16,
                buffer: BufferSlot::LibraryIndex,
            },
            phase,
        )
    }

    fn issue_read(
        &mut self,
        offset: usize,
        len: usize,
        buffer_offset: usize,
        phase: Phase,
    ) -> LibraryPublicationUpdate {
        self.issue(
            Request::read_into(
                slot_name(self.slot),
                offset as u64,
                buffer_offset as u16,
                len as u16,
                BufferSlot::LibraryIndex,
            )
            .expect("validation reads are bounded"),
            phase,
        )
    }

    fn issue(&mut self, request: Request, phase: Phase) -> LibraryPublicationUpdate {
        let sequence = self.next_sequence;
        self.next_sequence = sequence.wrapping_add(1);
        self.expected_sequence = sequence;
        self.phase = phase;
        LibraryPublicationUpdate::Request(envelope(sequence, request))
    }
}

fn required_buffer(
    buffer: Option<&mut [u8]>,
    len: usize,
) -> Result<&mut [u8], LibraryPublicationError> {
    buffer
        .filter(|buffer| buffer.len() >= len)
        .ok_or(LibraryPublicationError::MissingBuffer)
}

fn slot_name(slot: LibraryIndexSlot) -> StorageName {
    let value = match slot {
        LibraryIndexSlot::A => LIBRARY_INDEX_SLOT_A,
        LibraryIndexSlot::B => LIBRARY_INDEX_SLOT_B,
        LibraryIndexSlot::Legacy => unreachable!(),
    };
    StorageName::new(value).expect("library slot names are bounded")
}

fn envelope(sequence: u32, request: Request) -> RequestEnvelope {
    RequestEnvelope {
        client: StorageClient::Library,
        sequence,
        request,
    }
}

fn update_crc(mut state: u32, bytes: &[u8]) -> u32 {
    for byte in bytes {
        state = crate::crc32::update_byte(state, *byte);
    }
    state
}
