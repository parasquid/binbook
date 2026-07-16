use crate::{
    library::{
        IncrementalLibraryScan, Library, LibraryIndexDecoder, LibraryStatus, INDEX_HEADER_SIZE,
        INDEX_RECORD_SIZE,
    },
    storage_access::LIBRARY_INDEX_FILE,
    storage_owner::{
        BufferSlot, Request, RequestEnvelope, Response, ResponseEnvelope, StorageClient,
        StorageName,
    },
};

pub const INDEX_RECORDS_PER_READ: usize = 29;
pub const INDEX_READ_BYTES: usize = INDEX_RECORDS_PER_READ * INDEX_RECORD_SIZE;

enum Target<'a> {
    Library(&'a mut Library),
    Decoder(LibraryIndexDecoder<'a>),
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Size,
    Header {
        file_len: usize,
    },
    Records {
        file_len: usize,
        offset: usize,
        len: usize,
    },
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryIndexLoadError {
    Correlation,
    Completion,
    Length,
    MissingBytes,
    Decode(crate::library::LibraryIndexError),
    HeaderLength { file_len: u32, record_count: u16 },
    AlreadyComplete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryIndexLoadUpdate {
    Request(RequestEnvelope),
    Complete(LibraryStatus),
}

pub struct LibraryIndexLoadClient<'a> {
    target: Target<'a>,
    phase: Phase,
    expected_sequence: u32,
    next_sequence: u32,
}

impl<'a> LibraryIndexLoadClient<'a> {
    pub fn begin(library: &'a mut Library, sequence: u32) -> (Self, RequestEnvelope) {
        let request = envelope(sequence, Request::FileSize { name: index_name() });
        (
            Self {
                target: Target::Library(library),
                phase: Phase::Size,
                expected_sequence: sequence,
                next_sequence: sequence.wrapping_add(1),
            },
            request,
        )
    }

    pub fn accept(
        &mut self,
        response: ResponseEnvelope,
        bytes: Option<&[u8]>,
    ) -> Result<LibraryIndexLoadUpdate, LibraryIndexLoadError> {
        if response.client != StorageClient::Library || response.sequence != self.expected_sequence
        {
            self.fail();
            return Err(LibraryIndexLoadError::Correlation);
        }
        let response = match response.response {
            Ok(response) => response,
            Err(_) => {
                self.fail();
                return Err(LibraryIndexLoadError::Completion);
            }
        };
        match (self.phase, response) {
            (Phase::Size, Response::FileSize(file_len)) => {
                let file_len =
                    usize::try_from(file_len).map_err(|_| LibraryIndexLoadError::Length)?;
                if !(INDEX_HEADER_SIZE..=Library::max_index_bytes()).contains(&file_len)
                    || !(file_len - INDEX_HEADER_SIZE).is_multiple_of(INDEX_RECORD_SIZE)
                {
                    self.fail();
                    return Err(LibraryIndexLoadError::Length);
                }
                Ok(self.issue_read(0, INDEX_HEADER_SIZE, Phase::Header { file_len }))
            }
            (Phase::Header { file_len }, Response::Read(read))
                if usize::from(read) == INDEX_HEADER_SIZE =>
            {
                let bytes = bytes.ok_or_else(|| {
                    self.fail();
                    LibraryIndexLoadError::MissingBytes
                })?;
                let header: &[u8; INDEX_HEADER_SIZE] = bytes
                    .get(..INDEX_HEADER_SIZE)
                    .and_then(|value| value.try_into().ok())
                    .ok_or(LibraryIndexLoadError::MissingBytes)?;
                let Target::Library(library) = core::mem::replace(&mut self.target, Target::None)
                else {
                    self.fail();
                    return Err(LibraryIndexLoadError::Decode(
                        crate::library::LibraryIndexError::Length,
                    ));
                };
                let decoder =
                    LibraryIndexDecoder::new(header, file_len, library).map_err(|error| {
                        if error == crate::library::LibraryIndexError::Length {
                            LibraryIndexLoadError::HeaderLength {
                                file_len: file_len as u32,
                                record_count: u16::from_le_bytes([header[8], header[9]]),
                            }
                        } else {
                            LibraryIndexLoadError::Decode(error)
                        }
                    })?;
                self.target = Target::Decoder(decoder);
                if file_len == INDEX_HEADER_SIZE {
                    self.complete_decoder()
                } else {
                    let len = (file_len - INDEX_HEADER_SIZE).min(INDEX_READ_BYTES);
                    Ok(self.issue_read(
                        INDEX_HEADER_SIZE,
                        len,
                        Phase::Records {
                            file_len,
                            offset: INDEX_HEADER_SIZE,
                            len,
                        },
                    ))
                }
            }
            (
                Phase::Records {
                    file_len,
                    offset,
                    len,
                },
                Response::Read(read),
            ) if usize::from(read) == len => {
                let bytes = bytes.ok_or_else(|| {
                    self.fail();
                    LibraryIndexLoadError::MissingBytes
                })?;
                let chunk = bytes
                    .get(..len)
                    .ok_or(LibraryIndexLoadError::MissingBytes)?;
                let Target::Decoder(decoder) = &mut self.target else {
                    self.fail();
                    return Err(LibraryIndexLoadError::Decode(
                        crate::library::LibraryIndexError::Length,
                    ));
                };
                for record in chunk.chunks_exact(INDEX_RECORD_SIZE) {
                    decoder
                        .push_record(record.try_into().unwrap())
                        .map_err(LibraryIndexLoadError::Decode)?;
                }
                let next = offset + len;
                if next == file_len {
                    self.complete_decoder()
                } else {
                    let next_len = (file_len - next).min(INDEX_READ_BYTES);
                    Ok(self.issue_read(
                        next,
                        next_len,
                        Phase::Records {
                            file_len,
                            offset: next,
                            len: next_len,
                        },
                    ))
                }
            }
            (Phase::Complete, _) => Err(LibraryIndexLoadError::AlreadyComplete),
            _ => {
                self.fail();
                Err(LibraryIndexLoadError::Completion)
            }
        }
    }

    fn issue_read(&mut self, offset: usize, len: usize, phase: Phase) -> LibraryIndexLoadUpdate {
        let sequence = self.next_sequence;
        self.next_sequence = sequence.wrapping_add(1);
        self.expected_sequence = sequence;
        self.phase = phase;
        LibraryIndexLoadUpdate::Request(envelope(
            sequence,
            Request::read(
                index_name(),
                offset as u64,
                len as u16,
                BufferSlot::LibraryIndex,
            )
            .expect("index chunks are bounded"),
        ))
    }

    fn complete_decoder(&mut self) -> Result<LibraryIndexLoadUpdate, LibraryIndexLoadError> {
        let Target::Decoder(decoder) = core::mem::replace(&mut self.target, Target::None) else {
            self.fail();
            return Err(LibraryIndexLoadError::Decode(
                crate::library::LibraryIndexError::Length,
            ));
        };
        decoder.finish().map_err(LibraryIndexLoadError::Decode)?;
        self.phase = Phase::Complete;
        Ok(LibraryIndexLoadUpdate::Complete(LibraryStatus::Ready))
    }

    fn fail(&mut self) {
        match core::mem::replace(&mut self.target, Target::None) {
            Target::Library(library) => {
                IncrementalLibraryScan::begin(library).fail(LibraryStatus::CardError)
            }
            Target::Decoder(decoder) => decoder.fail(LibraryStatus::CardError),
            Target::None => {}
        }
        self.phase = Phase::Complete;
    }
}

fn index_name_for_cursor(cursor: crate::library::LibraryIndexCursor) -> StorageName {
    use crate::{
        library::LibraryIndexSlot,
        storage_access::{LIBRARY_INDEX_SLOT_A, LIBRARY_INDEX_SLOT_B},
    };
    StorageName::new(match cursor.slot() {
        LibraryIndexSlot::A => LIBRARY_INDEX_SLOT_A,
        LibraryIndexSlot::B => LIBRARY_INDEX_SLOT_B,
        LibraryIndexSlot::Legacy => LIBRARY_INDEX_FILE,
    })
    .expect("library index names are bounded")
}

fn index_name() -> StorageName {
    StorageName::new(LIBRARY_INDEX_FILE).expect("library index name is bounded")
}

fn envelope(sequence: u32, request: Request) -> RequestEnvelope {
    RequestEnvelope {
        client: StorageClient::Library,
        sequence,
        request,
    }
}

enum ViewportTarget<'a> {
    Empty(&'a mut crate::library::LibraryViewport),
    Validating {
        viewport: &'a mut crate::library::LibraryViewport,
        cursor: crate::library::LibraryIndexCursor,
        validator: crate::library::LibraryIndexCursorValidator,
    },
    None,
}

pub struct LibraryViewportLoadClient<'a> {
    target: ViewportTarget<'a>,
    requested_start: usize,
    phase: Phase,
    expected_sequence: u32,
    next_sequence: u32,
}

impl<'a> LibraryViewportLoadClient<'a> {
    pub fn begin(
        viewport: &'a mut crate::library::LibraryViewport,
        requested_start: usize,
        sequence: u32,
    ) -> (Self, RequestEnvelope) {
        viewport.begin(requested_start);
        let request = envelope(sequence, Request::FileSize { name: index_name() });
        (
            Self {
                target: ViewportTarget::Empty(viewport),
                requested_start,
                phase: Phase::Size,
                expected_sequence: sequence,
                next_sequence: sequence.wrapping_add(1),
            },
            request,
        )
    }

    pub fn accept(
        &mut self,
        response: ResponseEnvelope,
        bytes: Option<&[u8]>,
    ) -> Result<LibraryIndexLoadUpdate, LibraryIndexLoadError> {
        if response.client != StorageClient::Library || response.sequence != self.expected_sequence
        {
            self.fail();
            return Err(LibraryIndexLoadError::Correlation);
        }
        let response = response.response.map_err(|_| {
            self.fail();
            LibraryIndexLoadError::Completion
        })?;
        match (self.phase, response) {
            (Phase::Size, Response::FileSize(file_len)) => {
                let file_len =
                    usize::try_from(file_len).map_err(|_| LibraryIndexLoadError::Length)?;
                if !(INDEX_HEADER_SIZE..=Library::max_index_bytes()).contains(&file_len)
                    || !(file_len - INDEX_HEADER_SIZE).is_multiple_of(INDEX_RECORD_SIZE)
                {
                    self.fail();
                    return Err(LibraryIndexLoadError::Length);
                }
                Ok(self.issue_read(0, INDEX_HEADER_SIZE, Phase::Header { file_len }))
            }
            (Phase::Header { file_len }, Response::Read(read))
                if usize::from(read) == INDEX_HEADER_SIZE =>
            {
                let bytes = bytes.ok_or(LibraryIndexLoadError::MissingBytes)?;
                let header: &[u8; INDEX_HEADER_SIZE] = bytes
                    .get(..INDEX_HEADER_SIZE)
                    .and_then(|value| value.try_into().ok())
                    .ok_or(LibraryIndexLoadError::MissingBytes)?;
                let cursor = crate::library::LibraryIndexCursor::from_header(header, file_len)
                    .map_err(LibraryIndexLoadError::Decode)?;
                let ViewportTarget::Empty(viewport) =
                    core::mem::replace(&mut self.target, ViewportTarget::None)
                else {
                    self.fail();
                    return Err(LibraryIndexLoadError::AlreadyComplete);
                };
                viewport.begin(self.requested_start.min(cursor.entry_count()));
                let validator = crate::library::LibraryIndexCursorValidator::new(cursor, header);
                self.target = ViewportTarget::Validating {
                    viewport,
                    cursor,
                    validator,
                };
                if file_len == INDEX_HEADER_SIZE {
                    self.complete()
                } else {
                    let len = (file_len - INDEX_HEADER_SIZE).min(INDEX_READ_BYTES);
                    Ok(self.issue_read(
                        INDEX_HEADER_SIZE,
                        len,
                        Phase::Records {
                            file_len,
                            offset: INDEX_HEADER_SIZE,
                            len,
                        },
                    ))
                }
            }
            (
                Phase::Records {
                    file_len,
                    offset,
                    len,
                },
                Response::Read(read),
            ) if usize::from(read) == len => {
                let chunk = bytes
                    .and_then(|value| value.get(..len))
                    .ok_or(LibraryIndexLoadError::MissingBytes)?;
                let first_record = (offset - INDEX_HEADER_SIZE) / INDEX_RECORD_SIZE;
                let ViewportTarget::Validating {
                    viewport,
                    cursor,
                    validator,
                } = &mut self.target
                else {
                    self.fail();
                    return Err(LibraryIndexLoadError::AlreadyComplete);
                };
                for (local, record) in chunk.chunks_exact(INDEX_RECORD_SIZE).enumerate() {
                    let record: &[u8; INDEX_RECORD_SIZE] = record.try_into().unwrap();
                    validator
                        .push_record(record)
                        .map_err(LibraryIndexLoadError::Decode)?;
                    let index = first_record + local;
                    if index >= viewport.start()
                        && index < viewport.start() + crate::library::LIBRARY_VIEWPORT_CAPACITY
                    {
                        viewport
                            .push(
                                cursor
                                    .decode_record(record)
                                    .map_err(LibraryIndexLoadError::Decode)?,
                            )
                            .map_err(LibraryIndexLoadError::Decode)?;
                    }
                }
                let next = offset + len;
                if next == file_len {
                    self.complete()
                } else {
                    let next_len = (file_len - next).min(INDEX_READ_BYTES);
                    Ok(self.issue_read(
                        next,
                        next_len,
                        Phase::Records {
                            file_len,
                            offset: next,
                            len: next_len,
                        },
                    ))
                }
            }
            (Phase::Complete, _) => Err(LibraryIndexLoadError::AlreadyComplete),
            _ => {
                self.fail();
                Err(LibraryIndexLoadError::Completion)
            }
        }
    }

    fn issue_read(&mut self, offset: usize, len: usize, phase: Phase) -> LibraryIndexLoadUpdate {
        let sequence = self.next_sequence;
        self.next_sequence = sequence.wrapping_add(1);
        self.expected_sequence = sequence;
        self.phase = phase;
        LibraryIndexLoadUpdate::Request(envelope(
            sequence,
            Request::read(
                index_name(),
                offset as u64,
                len as u16,
                BufferSlot::LibraryIndex,
            )
            .expect("viewport index chunks are bounded"),
        ))
    }

    fn complete(&mut self) -> Result<LibraryIndexLoadUpdate, LibraryIndexLoadError> {
        let ViewportTarget::Validating {
            viewport,
            cursor,
            validator,
        } = core::mem::replace(&mut self.target, ViewportTarget::None)
        else {
            self.fail();
            return Err(LibraryIndexLoadError::AlreadyComplete);
        };
        validator.finish().map_err(LibraryIndexLoadError::Decode)?;
        viewport.finish(cursor);
        self.phase = Phase::Complete;
        Ok(LibraryIndexLoadUpdate::Complete(LibraryStatus::Ready))
    }

    fn fail(&mut self) {
        match core::mem::replace(&mut self.target, ViewportTarget::None) {
            ViewportTarget::Empty(viewport) | ViewportTarget::Validating { viewport, .. } => {
                viewport.fail()
            }
            ViewportTarget::None => {}
        }
        self.phase = Phase::Complete;
    }
}

pub struct LibraryViewportWindowClient<'a> {
    viewport: Option<&'a mut crate::library::LibraryViewport>,
    cursor: crate::library::LibraryIndexCursor,
    expected_sequence: u32,
    expected_len: u16,
}

#[allow(clippy::large_enum_variant)]
enum PruneTarget<'a> {
    Empty(&'a mut crate::reader_state::ReaderState),
    Validating {
        state: &'a mut crate::reader_state::ReaderState,
        cursor: crate::library::LibraryIndexCursor,
        validator: crate::library::LibraryIndexCursorValidator,
        matched: [bool; crate::reader_state::MAX_PROGRESS_RECORDS],
    },
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryIndexPruneUpdate {
    Request(RequestEnvelope),
    Complete { removed: usize },
}

/// Validates the complete persistent index before pruning state records whose
/// filename hashes are absent. It retains only a 200-bit match mask, never the
/// index's filenames.
pub struct LibraryIndexPruneClient<'a> {
    target: PruneTarget<'a>,
    fixed_cursor: Option<crate::library::LibraryIndexCursor>,
    name: StorageName,
    phase: Phase,
    expected_sequence: u32,
    next_sequence: u32,
}

impl<'a> LibraryIndexPruneClient<'a> {
    pub fn begin(
        state: &'a mut crate::reader_state::ReaderState,
        sequence: u32,
    ) -> (Self, RequestEnvelope) {
        (
            Self {
                target: PruneTarget::Empty(state),
                fixed_cursor: None,
                name: index_name(),
                phase: Phase::Size,
                expected_sequence: sequence,
                next_sequence: sequence.wrapping_add(1),
            },
            envelope(sequence, Request::FileSize { name: index_name() }),
        )
    }

    pub fn begin_cursor(
        state: &'a mut crate::reader_state::ReaderState,
        cursor: crate::library::LibraryIndexCursor,
        sequence: u32,
    ) -> (Self, RequestEnvelope) {
        let name = index_name_for_cursor(cursor);
        let request = envelope(
            sequence,
            Request::read(
                name.clone(),
                0,
                cursor.header_size() as u16,
                BufferSlot::LibraryIndex,
            )
            .expect("index header is bounded"),
        );
        (
            Self {
                target: PruneTarget::Empty(state),
                fixed_cursor: Some(cursor),
                name,
                phase: Phase::Header {
                    file_len: cursor.file_len(),
                },
                expected_sequence: sequence,
                next_sequence: sequence.wrapping_add(1),
            },
            request,
        )
    }

    pub fn accept(
        &mut self,
        response: ResponseEnvelope,
        bytes: Option<&[u8]>,
    ) -> Result<LibraryIndexPruneUpdate, LibraryIndexLoadError> {
        if response.client != StorageClient::Library || response.sequence != self.expected_sequence
        {
            self.fail();
            return Err(LibraryIndexLoadError::Correlation);
        }
        let response = response.response.map_err(|_| {
            self.fail();
            LibraryIndexLoadError::Completion
        })?;
        match (self.phase, response) {
            (Phase::Size, Response::FileSize(file_len)) => {
                let file_len =
                    usize::try_from(file_len).map_err(|_| LibraryIndexLoadError::Length)?;
                if !(INDEX_HEADER_SIZE..=Library::max_index_bytes()).contains(&file_len)
                    || !(file_len - INDEX_HEADER_SIZE).is_multiple_of(INDEX_RECORD_SIZE)
                {
                    self.fail();
                    return Err(LibraryIndexLoadError::Length);
                }
                Ok(self.issue_read(0, INDEX_HEADER_SIZE, Phase::Header { file_len }))
            }
            (Phase::Header { file_len }, Response::Read(read))
                if usize::from(read)
                    == self
                        .fixed_cursor
                        .map_or(INDEX_HEADER_SIZE, |cursor| cursor.header_size()) =>
            {
                let header_len = self
                    .fixed_cursor
                    .map_or(INDEX_HEADER_SIZE, |cursor| cursor.header_size());
                let header = bytes
                    .and_then(|bytes| bytes.get(..header_len))
                    .ok_or(LibraryIndexLoadError::MissingBytes)?;
                let cursor = if let Some(expected) = self.fixed_cursor {
                    let header: &[u8; crate::library::INDEX_V2_HEADER_SIZE] = header
                        .try_into()
                        .map_err(|_| LibraryIndexLoadError::MissingBytes)?;
                    let actual = crate::library::LibraryIndexCursor::from_v2_header(
                        expected.slot(),
                        header,
                        file_len,
                    )
                    .map_err(LibraryIndexLoadError::Decode)?;
                    if actual != expected {
                        self.fail();
                        return Err(LibraryIndexLoadError::Completion);
                    }
                    actual
                } else {
                    crate::library::LibraryIndexCursor::from_header(
                        header
                            .try_into()
                            .map_err(|_| LibraryIndexLoadError::MissingBytes)?,
                        file_len,
                    )
                    .map_err(LibraryIndexLoadError::Decode)?
                };
                let PruneTarget::Empty(state) =
                    core::mem::replace(&mut self.target, PruneTarget::None)
                else {
                    return Err(LibraryIndexLoadError::AlreadyComplete);
                };
                self.target = PruneTarget::Validating {
                    state,
                    cursor,
                    validator: crate::library::LibraryIndexCursorValidator::new(cursor, header),
                    matched: [false; crate::reader_state::MAX_PROGRESS_RECORDS],
                };
                if file_len == header_len {
                    self.complete()
                } else {
                    let len = (file_len - header_len).min(INDEX_READ_BYTES);
                    Ok(self.issue_read(
                        header_len,
                        len,
                        Phase::Records {
                            file_len,
                            offset: header_len,
                            len,
                        },
                    ))
                }
            }
            (
                Phase::Records {
                    file_len,
                    offset,
                    len,
                },
                Response::Read(read),
            ) if usize::from(read) == len => {
                let chunk = bytes
                    .and_then(|bytes| bytes.get(..len))
                    .ok_or(LibraryIndexLoadError::MissingBytes)?;
                let PruneTarget::Validating {
                    state,
                    cursor,
                    validator,
                    matched,
                } = &mut self.target
                else {
                    return Err(LibraryIndexLoadError::AlreadyComplete);
                };
                for record in chunk.chunks_exact(INDEX_RECORD_SIZE) {
                    let record: &[u8; INDEX_RECORD_SIZE] = record.try_into().unwrap();
                    validator
                        .push_record(record)
                        .map_err(LibraryIndexLoadError::Decode)?;
                    let hash = cursor
                        .decode_record(record)
                        .map_err(LibraryIndexLoadError::Decode)?
                        .filename_hash;
                    for (index, progress) in state.records().iter().enumerate() {
                        if progress.filename_hash == hash {
                            matched[index] = true;
                        }
                    }
                }
                let next = offset + len;
                if next == file_len {
                    self.complete()
                } else {
                    let next_len = (file_len - next).min(INDEX_READ_BYTES);
                    Ok(self.issue_read(
                        next,
                        next_len,
                        Phase::Records {
                            file_len,
                            offset: next,
                            len: next_len,
                        },
                    ))
                }
            }
            (Phase::Complete, _) => Err(LibraryIndexLoadError::AlreadyComplete),
            _ => {
                self.fail();
                Err(LibraryIndexLoadError::Completion)
            }
        }
    }

    fn issue_read(&mut self, offset: usize, len: usize, phase: Phase) -> LibraryIndexPruneUpdate {
        let sequence = self.next_sequence;
        self.next_sequence = sequence.wrapping_add(1);
        self.expected_sequence = sequence;
        self.phase = phase;
        LibraryIndexPruneUpdate::Request(envelope(
            sequence,
            Request::read(
                self.name.clone(),
                offset as u64,
                len as u16,
                BufferSlot::LibraryIndex,
            )
            .expect("prune index chunks are bounded"),
        ))
    }

    fn complete(&mut self) -> Result<LibraryIndexPruneUpdate, LibraryIndexLoadError> {
        let PruneTarget::Validating {
            state,
            validator,
            matched,
            ..
        } = core::mem::replace(&mut self.target, PruneTarget::None)
        else {
            return Err(LibraryIndexLoadError::AlreadyComplete);
        };
        validator.finish().map_err(LibraryIndexLoadError::Decode)?;
        let removed = state.retain_record_mask(&matched);
        self.phase = Phase::Complete;
        Ok(LibraryIndexPruneUpdate::Complete { removed })
    }

    fn fail(&mut self) {
        self.target = PruneTarget::None;
        self.phase = Phase::Complete;
    }
}

impl<'a> LibraryViewportWindowClient<'a> {
    pub fn begin(
        viewport: &'a mut crate::library::LibraryViewport,
        cursor: crate::library::LibraryIndexCursor,
        requested_start: usize,
        sequence: u32,
    ) -> Result<(Self, RequestEnvelope), LibraryIndexLoadError> {
        if cursor.entry_count() == 0 {
            viewport.fail();
            return Err(LibraryIndexLoadError::Length);
        }
        let start = requested_start.min(cursor.entry_count() - 1);
        let count = (cursor.entry_count() - start).min(crate::library::LIBRARY_VIEWPORT_CAPACITY);
        let first = cursor
            .record_range(start)
            .map_err(LibraryIndexLoadError::Decode)?;
        let len = count * INDEX_RECORD_SIZE;
        viewport.begin(start);
        let request = envelope(
            sequence,
            Request::read(
                index_name_for_cursor(cursor),
                first.start as u64,
                len as u16,
                BufferSlot::LibraryIndex,
            )
            .expect("viewport window is bounded"),
        );
        Ok((
            Self {
                viewport: Some(viewport),
                cursor,
                expected_sequence: sequence,
                expected_len: len as u16,
            },
            request,
        ))
    }

    pub fn accept(
        &mut self,
        response: ResponseEnvelope,
        bytes: &[u8],
    ) -> Result<LibraryIndexLoadUpdate, LibraryIndexLoadError> {
        if response.client != StorageClient::Library || response.sequence != self.expected_sequence
        {
            self.fail();
            return Err(LibraryIndexLoadError::Correlation);
        }
        let read = match response.response {
            Ok(Response::Read(read)) if read == self.expected_len => read,
            _ => {
                self.fail();
                return Err(LibraryIndexLoadError::Completion);
            }
        };
        let bytes = bytes
            .get(..usize::from(read))
            .ok_or(LibraryIndexLoadError::MissingBytes)?;
        let viewport = self
            .viewport
            .as_deref_mut()
            .ok_or(LibraryIndexLoadError::AlreadyComplete)?;
        for record in bytes.chunks_exact(INDEX_RECORD_SIZE) {
            let record: &[u8; INDEX_RECORD_SIZE] = record.try_into().unwrap();
            viewport
                .push(
                    self.cursor
                        .decode_record(record)
                        .map_err(LibraryIndexLoadError::Decode)?,
                )
                .map_err(LibraryIndexLoadError::Decode)?;
        }
        viewport.finish(self.cursor);
        self.viewport = None;
        Ok(LibraryIndexLoadUpdate::Complete(LibraryStatus::Ready))
    }

    fn fail(&mut self) {
        if let Some(viewport) = self.viewport.take() {
            viewport.fail();
        }
    }
}
