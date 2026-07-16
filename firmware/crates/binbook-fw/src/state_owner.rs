use crate::{
    reader_state::{ReaderState, ReaderStateError, MAX_STATE_BYTES, STATE_HEADER_SIZE},
    storage_access::{LEGACY_STATE_SLOT_A, LEGACY_STATE_SLOT_B, STATE_SLOT_A, STATE_SLOT_B},
    storage_owner::{
        BufferSlot, Request, RequestEnvelope, Response, ResponseEnvelope, StorageClient,
        StorageName, MAX_BOUNDED_IO_BYTES,
    },
};

const WRITE_CHUNK_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerStateSlots {
    loaded_slot: Option<usize>,
    loaded_generation: u32,
}

impl OwnerStateSlots {
    pub const fn new() -> Self {
        Self {
            loaded_slot: None,
            loaded_generation: 0,
        }
    }

    pub fn begin_load<'a>(
        &'a mut self,
        target: &'a mut ReaderState,
        sequence: u32,
    ) -> (StateLoadClient<'a>, RequestEnvelope) {
        *target = ReaderState::empty();
        (
            StateLoadClient {
                slots: self,
                target,
                phase: LoadPhase::Size { slot: 0 },
                expected_sequence: sequence,
                next_sequence: sequence.wrapping_add(1),
                selected_slot: None,
                selected_generation: 0,
            },
            envelope(sequence, Request::FileSize { name: slot_name(0) }),
        )
    }

    pub fn begin_save<'a>(
        &'a mut self,
        state: &'a mut ReaderState,
        buffer: &mut [u8],
        sequence: u32,
    ) -> Result<(StateSaveClient<'a>, RequestEnvelope), StateSaveError> {
        let target_slot = match self.loaded_slot {
            Some(0) => 1,
            Some(1) | None => 0,
            _ => unreachable!(),
        };
        let next_generation = self.loaded_generation.wrapping_add(1);
        let used = state
            .encode_with_generation(next_generation, buffer)
            .map_err(StateSaveError::Codec)?;
        let request = envelope(
            sequence,
            Request::UploadBegin {
                name: slot_name(target_slot),
                expected_size: used as u64,
                expected_crc32: 0,
                power_save: true,
            },
        );
        Ok((
            StateSaveClient {
                slots: self,
                state,
                target_slot,
                next_generation,
                used,
                phase: SavePhase::Begin,
                expected_sequence: sequence,
                next_sequence: sequence.wrapping_add(1),
            },
            request,
        ))
    }
}

impl Default for OwnerStateSlots {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateLoadError {
    Correlation,
    Completion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateLoadUpdate {
    Request(RequestEnvelope),
    Complete { warning: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadPhase {
    Size {
        slot: usize,
    },
    Read {
        slot: usize,
        size: usize,
        offset: usize,
        len: u16,
    },
    Complete,
}

pub struct StateLoadClient<'a> {
    slots: &'a mut OwnerStateSlots,
    target: &'a mut ReaderState,
    phase: LoadPhase,
    expected_sequence: u32,
    next_sequence: u32,
    selected_slot: Option<usize>,
    selected_generation: u32,
}

impl StateLoadClient<'_> {
    pub fn accept(
        &mut self,
        response: ResponseEnvelope,
        buffer: &mut [u8],
    ) -> Result<StateLoadUpdate, StateLoadError> {
        if response.client != StorageClient::State || response.sequence != self.expected_sequence {
            return Err(StateLoadError::Correlation);
        }
        match self.phase {
            LoadPhase::Size { slot } => match response.response {
                Ok(Response::FileSize(size)) => {
                    let Ok(size) = usize::try_from(size) else {
                        return Ok(self.finish_slot(slot));
                    };
                    if !(STATE_HEADER_SIZE..=MAX_STATE_BYTES).contains(&size) || buffer.len() < size
                    {
                        return Ok(self.finish_slot(slot));
                    }
                    Ok(self.issue_read(slot, size, 0))
                }
                Err(_) => Ok(self.finish_slot(slot)),
                _ => Err(StateLoadError::Completion),
            },
            LoadPhase::Read {
                slot,
                size,
                offset,
                len,
            } => match response.response {
                Ok(Response::Read(actual)) if actual == len => {
                    let next = offset + usize::from(len);
                    if next < size {
                        Ok(self.issue_read(slot, size, next))
                    } else {
                        if let Ok(candidate) = ReaderState::decode(&buffer[..size]) {
                            let is_newer = self.selected_slot.is_none()
                                || candidate.generation.wrapping_sub(self.selected_generation)
                                    < 0x8000_0000;
                            if is_newer {
                                self.selected_slot = Some(slot % 2);
                                self.selected_generation = candidate.generation;
                                *self.target = candidate;
                            }
                        }
                        Ok(self.finish_slot(slot))
                    }
                }
                Err(_) => Ok(self.finish_slot(slot)),
                _ => Err(StateLoadError::Completion),
            },
            LoadPhase::Complete => Err(StateLoadError::Completion),
        }
    }

    fn issue_read(&mut self, slot: usize, size: usize, offset: usize) -> StateLoadUpdate {
        let len = (size - offset).min(MAX_BOUNDED_IO_BYTES as usize) as u16;
        let sequence = self.take_sequence();
        self.phase = LoadPhase::Read {
            slot,
            size,
            offset,
            len,
        };
        StateLoadUpdate::Request(envelope(
            sequence,
            Request::read_into(
                load_slot_name(slot),
                offset as u64,
                offset as u16,
                len,
                BufferSlot::State,
            )
            .expect("state read chunks are bounded"),
        ))
    }

    fn finish_slot(&mut self, slot: usize) -> StateLoadUpdate {
        let next_slot = match slot {
            0 => Some(1),
            1 if self.selected_slot.is_none() => Some(2),
            2 => Some(3),
            1 | 3 => None,
            _ => unreachable!(),
        };
        if let Some(next_slot) = next_slot {
            let sequence = self.take_sequence();
            self.phase = LoadPhase::Size { slot: next_slot };
            StateLoadUpdate::Request(envelope(
                sequence,
                Request::FileSize {
                    name: load_slot_name(next_slot),
                },
            ))
        } else {
            self.phase = LoadPhase::Complete;
            self.slots.loaded_slot = self.selected_slot;
            self.slots.loaded_generation = self.selected_generation;
            StateLoadUpdate::Complete {
                warning: self.selected_slot.is_none(),
            }
        }
    }

    fn take_sequence(&mut self) -> u32 {
        let sequence = self.next_sequence;
        self.next_sequence = sequence.wrapping_add(1);
        self.expected_sequence = sequence;
        sequence
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateSaveError {
    Correlation,
    Completion,
    Codec(ReaderStateError),
    Mismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateSaveUpdate {
    Request(RequestEnvelope),
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SavePhase {
    Begin,
    Write {
        upload_id: u32,
        offset: usize,
        len: u16,
    },
    Commit {
        upload_id: u32,
    },
    Read {
        offset: usize,
        len: u16,
    },
    Abort {
        error: StateSaveError,
    },
    Complete,
}

pub struct StateSaveClient<'a> {
    slots: &'a mut OwnerStateSlots,
    state: &'a mut ReaderState,
    target_slot: usize,
    next_generation: u32,
    used: usize,
    phase: SavePhase,
    expected_sequence: u32,
    next_sequence: u32,
}

impl StateSaveClient<'_> {
    pub fn accept(
        &mut self,
        response: ResponseEnvelope,
        buffer: &mut [u8],
    ) -> Result<StateSaveUpdate, StateSaveError> {
        if response.client != StorageClient::State || response.sequence != self.expected_sequence {
            return Err(StateSaveError::Correlation);
        }
        let response = match response.response {
            Ok(response) => response,
            Err(_) => return self.fail_or_abort(StateSaveError::Completion),
        };
        match (self.phase, response) {
            (SavePhase::Begin, Response::UploadStarted(upload_id)) => {
                Ok(self.issue_write(upload_id, 0))
            }
            (
                SavePhase::Write {
                    upload_id,
                    offset,
                    len,
                },
                Response::Written(actual),
            ) if actual == len => {
                let next = offset + usize::from(len);
                if next < self.used {
                    Ok(self.issue_write(upload_id, next))
                } else {
                    let sequence = self.take_sequence();
                    self.phase = SavePhase::Commit { upload_id };
                    Ok(StateSaveUpdate::Request(envelope(
                        sequence,
                        Request::upload_commit(upload_id, true),
                    )))
                }
            }
            (SavePhase::Commit { .. }, Response::Ack) => Ok(self.issue_read(0)),
            (SavePhase::Read { offset, len }, Response::Read(actual)) if actual == len => {
                let next = offset + usize::from(len);
                if next < self.used {
                    Ok(self.issue_read(next))
                } else {
                    let verified =
                        ReaderState::decode(&buffer[..self.used]).map_err(StateSaveError::Codec)?;
                    if verified.generation != self.next_generation
                        || !self.state.matches_except_generation(&verified)
                    {
                        return Err(StateSaveError::Mismatch);
                    }
                    *self.state = verified;
                    self.slots.loaded_generation = self.next_generation;
                    self.slots.loaded_slot = Some(self.target_slot);
                    self.phase = SavePhase::Complete;
                    Ok(StateSaveUpdate::Complete)
                }
            }
            (SavePhase::Abort { error }, Response::Ack) => {
                self.phase = SavePhase::Complete;
                Err(error)
            }
            (SavePhase::Complete, _) => Err(StateSaveError::Completion),
            _ => self.fail_or_abort(StateSaveError::Completion),
        }
    }

    fn issue_write(&mut self, upload_id: u32, offset: usize) -> StateSaveUpdate {
        let len = (self.used - offset).min(WRITE_CHUNK_BYTES) as u16;
        let sequence = self.take_sequence();
        self.phase = SavePhase::Write {
            upload_id,
            offset,
            len,
        };
        StateSaveUpdate::Request(envelope(
            sequence,
            Request::UploadWrite {
                upload_id,
                offset: offset as u64,
                buffer_offset: offset as u16,
                len,
                buffer: BufferSlot::State,
                power_save: true,
            },
        ))
    }

    fn issue_read(&mut self, offset: usize) -> StateSaveUpdate {
        let len = (self.used - offset).min(MAX_BOUNDED_IO_BYTES as usize) as u16;
        let sequence = self.take_sequence();
        self.phase = SavePhase::Read { offset, len };
        StateSaveUpdate::Request(envelope(
            sequence,
            Request::read_into(
                slot_name(self.target_slot),
                offset as u64,
                offset as u16,
                len,
                BufferSlot::State,
            )
            .expect("state readback chunks are bounded"),
        ))
    }

    fn take_sequence(&mut self) -> u32 {
        let sequence = self.next_sequence;
        self.next_sequence = sequence.wrapping_add(1);
        self.expected_sequence = sequence;
        sequence
    }

    fn fail_or_abort(&mut self, error: StateSaveError) -> Result<StateSaveUpdate, StateSaveError> {
        let upload_id = match self.phase {
            SavePhase::Write { upload_id, .. } | SavePhase::Commit { upload_id } => upload_id,
            SavePhase::Abort { .. }
            | SavePhase::Begin
            | SavePhase::Read { .. }
            | SavePhase::Complete => {
                self.phase = SavePhase::Complete;
                return Err(error);
            }
        };
        let sequence = self.take_sequence();
        self.phase = SavePhase::Abort { error };
        Ok(StateSaveUpdate::Request(envelope(
            sequence,
            Request::UploadAbort { upload_id },
        )))
    }
}

fn slot_name(slot: usize) -> StorageName {
    StorageName::new(if slot == 0 {
        STATE_SLOT_A
    } else {
        STATE_SLOT_B
    })
    .expect("state slot names are bounded")
}

fn load_slot_name(slot: usize) -> StorageName {
    let name = match slot {
        0 => STATE_SLOT_A,
        1 => STATE_SLOT_B,
        2 => LEGACY_STATE_SLOT_A,
        3 => LEGACY_STATE_SLOT_B,
        _ => unreachable!(),
    };
    StorageName::new(name).expect("state load slot names are bounded")
}

fn envelope(sequence: u32, request: Request) -> RequestEnvelope {
    RequestEnvelope {
        client: StorageClient::State,
        sequence,
        request,
    }
}
