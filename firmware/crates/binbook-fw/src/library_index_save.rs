use crate::{
    library::{Library, LibraryIndexError, INDEX_HEADER_SIZE, INDEX_RECORD_SIZE},
    storage_access::LIBRARY_INDEX_FILE,
    storage_owner::{
        BufferSlot, Request, RequestEnvelope, Response, ResponseEnvelope, StorageClient,
        StorageName,
    },
};

pub const INDEX_RECORDS_PER_WRITE: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Begin,
    Write {
        upload_id: u32,
        expected_len: u16,
        next_record: usize,
    },
    Commit,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryIndexSaveError {
    Correlation,
    Completion,
    BufferTooSmall,
    Encode(LibraryIndexError),
    AlreadyComplete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryIndexSaveUpdate {
    Request(RequestEnvelope),
    Complete,
}

pub struct LibraryIndexSaveClient<'a> {
    library: &'a Library,
    phase: Phase,
    expected_sequence: u32,
    next_sequence: u32,
}

impl<'a> LibraryIndexSaveClient<'a> {
    pub fn begin(library: &'a Library, sequence: u32) -> (Self, RequestEnvelope) {
        let request = envelope(
            sequence,
            Request::UploadBegin {
                name: index_name(),
                expected_size: library.index_encoded_len() as u64,
                expected_crc32: 0,
                power_save: false,
            },
        );
        (
            Self {
                library,
                phase: Phase::Begin,
                expected_sequence: sequence,
                next_sequence: sequence.wrapping_add(1),
            },
            request,
        )
    }

    pub fn accept(
        &mut self,
        response: ResponseEnvelope,
        buffer: &mut [u8],
    ) -> Result<LibraryIndexSaveUpdate, LibraryIndexSaveError> {
        if response.client != StorageClient::Library || response.sequence != self.expected_sequence
        {
            self.phase = Phase::Complete;
            return Err(LibraryIndexSaveError::Correlation);
        }
        let response = response.response.map_err(|_| {
            self.phase = Phase::Complete;
            LibraryIndexSaveError::Completion
        })?;
        match (self.phase, response) {
            (Phase::Begin, Response::UploadStarted(upload_id)) => {
                if buffer.len() < INDEX_HEADER_SIZE {
                    self.phase = Phase::Complete;
                    return Err(LibraryIndexSaveError::BufferTooSmall);
                }
                self.library
                    .encode_index_header((&mut buffer[..INDEX_HEADER_SIZE]).try_into().unwrap())
                    .map_err(LibraryIndexSaveError::Encode)?;
                Ok(self.issue_write(upload_id, 0, INDEX_HEADER_SIZE as u16, 0))
            }
            (
                Phase::Write {
                    upload_id,
                    expected_len,
                    next_record,
                },
                Response::Written(actual),
            ) if actual == expected_len => {
                if next_record == self.library.entries().len() {
                    let sequence = self.take_sequence();
                    self.phase = Phase::Commit;
                    Ok(LibraryIndexSaveUpdate::Request(envelope(
                        sequence,
                        Request::upload_commit(upload_id, false),
                    )))
                } else {
                    let count =
                        (self.library.entries().len() - next_record).min(INDEX_RECORDS_PER_WRITE);
                    let len = count * INDEX_RECORD_SIZE;
                    if buffer.len() < len {
                        self.phase = Phase::Complete;
                        return Err(LibraryIndexSaveError::BufferTooSmall);
                    }
                    for index in 0..count {
                        let start = index * INDEX_RECORD_SIZE;
                        self.library
                            .encode_index_record(
                                next_record + index,
                                (&mut buffer[start..start + INDEX_RECORD_SIZE])
                                    .try_into()
                                    .unwrap(),
                            )
                            .map_err(LibraryIndexSaveError::Encode)?;
                    }
                    let offset = INDEX_HEADER_SIZE + next_record * INDEX_RECORD_SIZE;
                    Ok(self.issue_write(upload_id, offset as u64, len as u16, next_record + count))
                }
            }
            (Phase::Commit, Response::Ack) => {
                self.phase = Phase::Complete;
                Ok(LibraryIndexSaveUpdate::Complete)
            }
            (Phase::Complete, _) => Err(LibraryIndexSaveError::AlreadyComplete),
            _ => {
                self.phase = Phase::Complete;
                Err(LibraryIndexSaveError::Completion)
            }
        }
    }

    fn issue_write(
        &mut self,
        upload_id: u32,
        offset: u64,
        len: u16,
        next_record: usize,
    ) -> LibraryIndexSaveUpdate {
        let sequence = self.take_sequence();
        self.phase = Phase::Write {
            upload_id,
            expected_len: len,
            next_record,
        };
        LibraryIndexSaveUpdate::Request(envelope(
            sequence,
            Request::UploadWrite {
                upload_id,
                offset,
                buffer_offset: 0,
                len,
                buffer: BufferSlot::LibraryIndex,
                power_save: false,
            },
        ))
    }

    fn take_sequence(&mut self) -> u32 {
        let sequence = self.next_sequence;
        self.next_sequence = sequence.wrapping_add(1);
        self.expected_sequence = sequence;
        sequence
    }
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
