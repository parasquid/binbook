use crate::{
    library::{IncrementalLibraryScan, Library, LibraryStatus, LIBRARY_VIEWPORT_CAPACITY},
    storage_owner::{
        decode_scan_records, Request, RequestEnvelope, Response, ResponseEnvelope, ScanToken,
        StorageClient, StorageName,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    WaitingStart,
    WaitingPrefetch(ScanToken, usize),
    WaitingStep(ScanToken),
    WaitingFinish(ScanToken),
    WaitingCancel,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryRefreshError {
    Correlation,
    Completion,
    MissingBatch,
    Decode,
    AlreadyComplete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryRefreshUpdate {
    Request(RequestEnvelope),
    Complete(LibraryStatus),
}

pub struct LibraryRefreshClient<'a> {
    scan: Option<IncrementalLibraryScan<'a>>,
    phase: Phase,
    expected_sequence: u32,
    next_sequence: u32,
    prefetch: heapless::Vec<StorageName, LIBRARY_VIEWPORT_CAPACITY>,
}

impl<'a> LibraryRefreshClient<'a> {
    pub fn begin(library: &'a mut Library, sequence: u32) -> (Self, RequestEnvelope) {
        let request = envelope(sequence, Request::ScanStart);
        (
            Self {
                scan: Some(IncrementalLibraryScan::begin(library)),
                phase: Phase::WaitingStart,
                expected_sequence: sequence,
                next_sequence: sequence.wrapping_add(1),
                prefetch: heapless::Vec::new(),
            },
            request,
        )
    }

    pub fn begin_with_prefetch(
        library: &'a mut Library,
        names: &[StorageName],
        sequence: u32,
    ) -> Result<(Self, RequestEnvelope), crate::storage_owner::RequestError> {
        let (mut client, request) = Self::begin(library, sequence);
        for name in names.iter().take(LIBRARY_VIEWPORT_CAPACITY) {
            client
                .prefetch
                .push(name.clone())
                .map_err(|_| crate::storage_owner::RequestError::TooLarge)?;
        }
        Ok((client, request))
    }

    pub fn accept(
        &mut self,
        response: ResponseEnvelope,
        batch: Option<&[u8]>,
    ) -> Result<LibraryRefreshUpdate, LibraryRefreshError> {
        if response.client != StorageClient::Library || response.sequence != self.expected_sequence
        {
            self.fail();
            return Err(LibraryRefreshError::Correlation);
        }
        let response = match response.response {
            Ok(response) => response,
            Err(_) => {
                if matches!(self.phase, Phase::WaitingStart) {
                    self.fail();
                }
                return Err(LibraryRefreshError::Completion);
            }
        };
        match (self.phase, response) {
            (Phase::WaitingStart, Response::ScanStarted(token)) => {
                if let Some(name) = self.prefetch.first().cloned() {
                    Ok(self.issue(
                        Request::ScanPrefetch { token, name },
                        Phase::WaitingPrefetch(token, 1),
                    ))
                } else {
                    Ok(self.issue(Request::scan_step(token), Phase::WaitingStep(token)))
                }
            }
            (Phase::WaitingPrefetch(token, next), Response::Ack) => {
                if let Some(name) = self.prefetch.get(next).cloned() {
                    Ok(self.issue(
                        Request::ScanPrefetch { token, name },
                        Phase::WaitingPrefetch(token, next + 1),
                    ))
                } else {
                    Ok(self.issue(Request::scan_step(token), Phase::WaitingStep(token)))
                }
            }
            (Phase::WaitingStep(token), Response::ScanProgress(progress)) => {
                let bytes = batch.ok_or_else(|| {
                    self.fail();
                    LibraryRefreshError::MissingBatch
                })?;
                let records = decode_scan_records(progress.entries, bytes).map_err(|_| {
                    self.fail();
                    LibraryRefreshError::Decode
                })?;
                let scan = self
                    .scan
                    .as_mut()
                    .ok_or(LibraryRefreshError::AlreadyComplete)?;
                for record in records {
                    scan.push_mounted(record.name.as_str(), record.size, record.mount_token);
                }
                if progress.complete {
                    Ok(self.issue(Request::ScanFinish { token }, Phase::WaitingFinish(token)))
                } else {
                    Ok(self.issue(Request::scan_step(token), Phase::WaitingStep(token)))
                }
            }
            (Phase::WaitingFinish(_), Response::Ack) => {
                let scan = self
                    .scan
                    .take()
                    .ok_or(LibraryRefreshError::AlreadyComplete)?;
                scan.finish();
                self.phase = Phase::Complete;
                Ok(LibraryRefreshUpdate::Complete(LibraryStatus::Ready))
            }
            (Phase::WaitingCancel, Response::Ack) => {
                self.phase = Phase::Complete;
                Ok(LibraryRefreshUpdate::Complete(LibraryStatus::CardError))
            }
            (Phase::Complete, _) => Err(LibraryRefreshError::AlreadyComplete),
            _ => {
                self.fail();
                Err(LibraryRefreshError::Completion)
            }
        }
    }

    pub fn cancel(&mut self) -> Option<RequestEnvelope> {
        let token = match self.phase {
            Phase::WaitingPrefetch(token, _)
            | Phase::WaitingStep(token)
            | Phase::WaitingFinish(token) => token,
            _ => return None,
        };
        self.scan.take();
        Some(
            match self.issue(Request::ScanCancel { token }, Phase::WaitingCancel) {
                LibraryRefreshUpdate::Request(request) => request,
                LibraryRefreshUpdate::Complete(_) => unreachable!(),
            },
        )
    }

    fn issue(&mut self, request: Request, phase: Phase) -> LibraryRefreshUpdate {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.expected_sequence = sequence;
        self.phase = phase;
        LibraryRefreshUpdate::Request(envelope(sequence, request))
    }

    fn fail(&mut self) {
        if let Some(scan) = self.scan.take() {
            scan.fail(LibraryStatus::CardError);
        }
        self.phase = Phase::Complete;
    }
}

fn envelope(sequence: u32, request: Request) -> RequestEnvelope {
    RequestEnvelope {
        client: StorageClient::Library,
        sequence,
        request,
    }
}
