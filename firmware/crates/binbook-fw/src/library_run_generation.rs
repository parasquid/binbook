use crate::{
    library::{LibraryEntry, LibraryStatus, LIBRARY_VIEWPORT_CAPACITY},
    library_external_sort::{
        ExternalSortError, LibraryRefreshWorkspace, MAX_RUN_BYTES, SPOOL_RECORD_SIZE,
    },
    reader_state::filename_hash,
    storage_owner::{
        decode_scan_record, BufferSlot, Request, RequestEnvelope, Response, ResponseEnvelope,
        ScanProgress, ScanToken, StorageClient, StorageName, SCAN_BATCH_BYTES,
    },
};

pub const RUN_BUFFER_OFFSET: usize = SCAN_BATCH_BYTES;
const _: () = assert!(RUN_BUFFER_OFFSET + MAX_RUN_BYTES <= 4096);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    ScratchCreate,
    ScanStart,
    Prefetch {
        token: ScanToken,
        next: u8,
    },
    ScanStep(ScanToken),
    RunWrite {
        token: ScanToken,
        progress: ScanProgress,
        next_record: u8,
        expected: u16,
    },
    ScanFinish(ScanToken),
    ScratchFlush,
    ErrorCancel(LibraryStatus),
    ErrorDelete(LibraryStatus),
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryRunGenerationError {
    Correlation,
    Completion,
    MissingBuffer,
    Decode,
    Sort(ExternalSortError),
    AlreadyComplete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryRunGenerationUpdate {
    Request(RequestEnvelope),
    Complete(LibraryStatus),
}

pub struct LibraryRunGenerationClient<'a> {
    workspace: &'a mut LibraryRefreshWorkspace,
    phase: Phase,
    expected_sequence: u32,
    next_sequence: u32,
    prefetch: heapless::Vec<StorageName, LIBRARY_VIEWPORT_CAPACITY>,
}

impl<'a> LibraryRunGenerationClient<'a> {
    pub fn begin(
        workspace: &'a mut LibraryRefreshWorkspace,
        prefetch: &[StorageName],
        sequence: u32,
    ) -> Result<(Self, RequestEnvelope), LibraryRunGenerationError> {
        workspace.reset();
        let mut names = heapless::Vec::new();
        for name in prefetch.iter().take(LIBRARY_VIEWPORT_CAPACITY) {
            names
                .push(name.clone())
                .map_err(|_| LibraryRunGenerationError::Sort(ExternalSortError::WorkspaceFull))?;
        }
        Ok((
            Self {
                workspace,
                phase: Phase::ScratchCreate,
                expected_sequence: sequence,
                next_sequence: sequence.wrapping_add(1),
                prefetch: names,
            },
            envelope(sequence, Request::LibraryScratchCreate),
        ))
    }

    pub fn accept(
        &mut self,
        response: ResponseEnvelope,
        buffer: Option<&mut [u8]>,
    ) -> Result<LibraryRunGenerationUpdate, LibraryRunGenerationError> {
        if response.client != StorageClient::Library || response.sequence != self.expected_sequence
        {
            self.phase = Phase::Complete;
            return Err(LibraryRunGenerationError::Correlation);
        }
        let response = response
            .response
            .map_err(|_| LibraryRunGenerationError::Completion)?;
        match (self.phase, response) {
            (Phase::ScratchCreate, Response::Ack) => {
                Ok(self.issue(Request::ScanStart, Phase::ScanStart))
            }
            (Phase::ScanStart, Response::ScanStarted(token)) => {
                if let Some(name) = self.prefetch.first().cloned() {
                    Ok(self.issue(
                        Request::ScanPrefetch { token, name },
                        Phase::Prefetch { token, next: 1 },
                    ))
                } else {
                    Ok(self.issue(Request::scan_step(token), Phase::ScanStep(token)))
                }
            }
            (Phase::Prefetch { token, next }, Response::Ack) => {
                if let Some(name) = self.prefetch.get(usize::from(next)).cloned() {
                    Ok(self.issue(
                        Request::ScanPrefetch { token, name },
                        Phase::Prefetch {
                            token,
                            next: next + 1,
                        },
                    ))
                } else {
                    Ok(self.issue(Request::scan_step(token), Phase::ScanStep(token)))
                }
            }
            (Phase::ScanStep(token), Response::ScanProgress(progress)) => {
                let buffer = buffer.ok_or(LibraryRunGenerationError::MissingBuffer)?;
                self.consume_batch(token, progress, 0, buffer)
            }
            (
                Phase::RunWrite {
                    token,
                    progress,
                    next_record,
                    expected,
                },
                Response::Written(actual),
            ) if actual == expected => {
                let buffer = buffer.ok_or(LibraryRunGenerationError::MissingBuffer)?;
                self.consume_batch(token, progress, next_record, buffer)
            }
            (Phase::ScanFinish(_), Response::Ack) => {
                Ok(self.issue(Request::LibraryScratchFlush, Phase::ScratchFlush))
            }
            (Phase::ScratchFlush, Response::Ack) => {
                self.phase = Phase::Complete;
                Ok(LibraryRunGenerationUpdate::Complete(
                    if self.workspace.entry_count() == 0 {
                        LibraryStatus::NoBooks
                    } else {
                        LibraryStatus::Ready
                    },
                ))
            }
            (Phase::ErrorCancel(status), Response::Ack) => {
                Ok(self.issue(Request::LibraryScratchDelete, Phase::ErrorDelete(status)))
            }
            (Phase::ErrorDelete(status), Response::Ack) => {
                self.phase = Phase::Complete;
                Ok(LibraryRunGenerationUpdate::Complete(status))
            }
            (Phase::Complete, _) => Err(LibraryRunGenerationError::AlreadyComplete),
            _ => {
                self.phase = Phase::Complete;
                Err(LibraryRunGenerationError::Completion)
            }
        }
    }

    pub fn cleanup(&mut self, status: LibraryStatus) -> Option<RequestEnvelope> {
        let active_token = match self.phase {
            Phase::Prefetch { token, .. }
            | Phase::ScanStep(token)
            | Phase::RunWrite { token, .. }
            | Phase::ScanFinish(token) => Some(token),
            _ => None,
        };
        if let Some(token) = active_token {
            return Some(
                match self.issue(Request::ScanCancel { token }, Phase::ErrorCancel(status)) {
                    LibraryRunGenerationUpdate::Request(request) => request,
                    LibraryRunGenerationUpdate::Complete(_) => unreachable!(),
                },
            );
        }
        if self.phase == Phase::Complete {
            return None;
        }
        Some(
            match self.issue(Request::LibraryScratchDelete, Phase::ErrorDelete(status)) {
                LibraryRunGenerationUpdate::Request(request) => request,
                LibraryRunGenerationUpdate::Complete(_) => unreachable!(),
            },
        )
    }

    fn consume_batch(
        &mut self,
        token: ScanToken,
        progress: ScanProgress,
        mut next: u8,
        buffer: &mut [u8],
    ) -> Result<LibraryRunGenerationUpdate, LibraryRunGenerationError> {
        if progress.unsupported_name {
            return Ok(self.issue(
                Request::ScanCancel { token },
                Phase::ErrorCancel(LibraryStatus::UnsupportedName),
            ));
        }
        if buffer.len() < RUN_BUFFER_OFFSET + MAX_RUN_BYTES {
            return Err(LibraryRunGenerationError::MissingBuffer);
        }
        while next < progress.entries {
            if self.workspace.buffered_count() == LIBRARY_VIEWPORT_CAPACITY {
                return self.write_run(token, progress, next, buffer);
            }
            let record = decode_scan_record(usize::from(next), buffer)
                .map_err(|_| LibraryRunGenerationError::Decode)?;
            let name = record.name.as_str();
            let entry = LibraryEntry {
                name: name
                    .try_into()
                    .map_err(|_| LibraryRunGenerationError::Decode)?,
                size: record.size,
                filename_hash: filename_hash(name.as_bytes()),
                mount_token: record.mount_token,
            };
            match self.workspace.push(entry) {
                Ok(()) => next += 1,
                Err(ExternalSortError::TooManyBooks) => {
                    return Ok(self.issue(
                        Request::ScanCancel { token },
                        Phase::ErrorCancel(LibraryStatus::TooManyBooks),
                    ));
                }
                Err(error) => return Err(LibraryRunGenerationError::Sort(error)),
            }
        }
        if progress.complete {
            if self.workspace.buffered_count() != 0 {
                self.write_run(token, progress, next, buffer)
            } else {
                Ok(self.issue(Request::ScanFinish { token }, Phase::ScanFinish(token)))
            }
        } else {
            Ok(self.issue(Request::scan_step(token), Phase::ScanStep(token)))
        }
    }

    fn write_run(
        &mut self,
        token: ScanToken,
        progress: ScanProgress,
        next_record: u8,
        buffer: &mut [u8],
    ) -> Result<LibraryRunGenerationUpdate, LibraryRunGenerationError> {
        let start_record = self.workspace.entry_count() - self.workspace.buffered_count();
        let used = self
            .workspace
            .flush_run(&mut buffer[RUN_BUFFER_OFFSET..])
            .map_err(LibraryRunGenerationError::Sort)?;
        let expected = used as u16;
        Ok(self.issue(
            Request::LibraryScratchWrite {
                offset: (start_record * SPOOL_RECORD_SIZE) as u64,
                buffer_offset: RUN_BUFFER_OFFSET as u16,
                len: expected,
                buffer: BufferSlot::LibraryIndex,
            },
            Phase::RunWrite {
                token,
                progress,
                next_record,
                expected,
            },
        ))
    }

    fn issue(&mut self, request: Request, phase: Phase) -> LibraryRunGenerationUpdate {
        let sequence = self.next_sequence;
        self.next_sequence = sequence.wrapping_add(1);
        self.expected_sequence = sequence;
        self.phase = phase;
        LibraryRunGenerationUpdate::Request(envelope(sequence, request))
    }
}

fn envelope(sequence: u32, request: Request) -> RequestEnvelope {
    RequestEnvelope {
        client: StorageClient::Library,
        sequence,
        request,
    }
}
