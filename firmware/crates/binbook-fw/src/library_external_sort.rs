use crate::library::{
    ascii_casefold_order, decode_index_entry, encode_index_entry, LibraryEntry, LibraryIndexError,
    MountFileToken, INDEX_RECORD_SIZE, LIBRARY_VIEWPORT_CAPACITY, MAX_LIBRARY_ENTRIES,
};

pub const SPOOL_RECORD_SIZE: usize = 2 + INDEX_RECORD_SIZE;
pub const MAX_SORT_RUNS: usize = MAX_LIBRARY_ENTRIES.div_ceil(LIBRARY_VIEWPORT_CAPACITY);
pub const MAX_RUN_BYTES: usize = LIBRARY_VIEWPORT_CAPACITY * SPOOL_RECORD_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunDescriptor {
    pub start_record: u16,
    pub len: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolEntry {
    pub entry: LibraryEntry,
    pub mount_token: Option<MountFileToken>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalSortError {
    WorkspaceFull,
    TooManyBooks,
    TooManyRuns,
    BufferTooSmall,
    InvalidSpoolRecord(LibraryIndexError),
    WrongPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Runs,
    Merge,
}

pub struct LibraryRefreshWorkspace {
    entries: heapless::Vec<LibraryEntry, LIBRARY_VIEWPORT_CAPACITY>,
    runs: heapless::Vec<RunDescriptor, MAX_SORT_RUNS>,
    total: u16,
    phase: Phase,
    previous_name: Option<heapless::String<{ binbook_storage::MAX_BINBOOK_NAME_BYTES }>>,
}

impl LibraryRefreshWorkspace {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: heapless::Vec::new(),
            runs: heapless::Vec::new(),
            total: 0,
            phase: Phase::Runs,
            previous_name: None,
        }
    }

    pub fn reset(&mut self) {
        self.entries.clear();
        self.runs.clear();
        self.total = 0;
        self.phase = Phase::Runs;
        self.previous_name = None;
    }

    #[must_use]
    pub fn entry_count(&self) -> usize {
        usize::from(self.total)
    }

    #[must_use]
    pub fn buffered_count(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    pub fn push(&mut self, entry: LibraryEntry) -> Result<(), ExternalSortError> {
        if self.phase != Phase::Runs {
            return Err(ExternalSortError::WrongPhase);
        }
        if usize::from(self.total) == MAX_LIBRARY_ENTRIES {
            return Err(ExternalSortError::TooManyBooks);
        }
        self.entries
            .push(entry)
            .map_err(|_| ExternalSortError::WorkspaceFull)?;
        self.total += 1;
        Ok(())
    }

    pub fn flush_run(&mut self, out: &mut [u8]) -> Result<usize, ExternalSortError> {
        if self.phase != Phase::Runs {
            return Err(ExternalSortError::WrongPhase);
        }
        if self.entries.is_empty() {
            return Ok(0);
        }
        let used = self.entries.len() * SPOOL_RECORD_SIZE;
        if out.len() < used {
            return Err(ExternalSortError::BufferTooSmall);
        }
        self.entries
            .as_mut_slice()
            .sort_unstable_by(|left, right| ascii_casefold_order(&left.name, &right.name));
        let start_record = self.total - self.entries.len() as u16;
        self.runs
            .push(RunDescriptor {
                start_record,
                len: self.entries.len() as u8,
            })
            .map_err(|_| ExternalSortError::TooManyRuns)?;
        for (index, entry) in self.entries.iter().enumerate() {
            encode_spool_entry(
                entry,
                &mut out[index * SPOOL_RECORD_SIZE..][..SPOOL_RECORD_SIZE],
            );
        }
        self.entries.clear();
        Ok(used)
    }

    pub fn begin_merge(
        &mut self,
        mut read: impl FnMut(u16, &mut [u8; SPOOL_RECORD_SIZE]) -> Result<(), ExternalSortError>,
    ) -> Result<(), ExternalSortError> {
        if self.phase != Phase::Runs || !self.entries.is_empty() {
            return Err(ExternalSortError::WrongPhase);
        }
        self.start_merge()?;
        let mut record = [0; SPOOL_RECORD_SIZE];
        while let Some(index) = self.next_initial_head_record() {
            read(index, &mut record)?;
            self.accept_initial_head(&record)?;
        }
        Ok(())
    }

    pub fn start_merge(&mut self) -> Result<(), ExternalSortError> {
        if self.phase != Phase::Runs || !self.entries.is_empty() {
            return Err(ExternalSortError::WrongPhase);
        }
        self.phase = Phase::Merge;
        self.previous_name = None;
        Ok(())
    }

    #[must_use]
    pub fn next_initial_head_record(&self) -> Option<u16> {
        if self.phase != Phase::Merge || self.entries.len() >= self.runs.len() {
            return None;
        }
        Some(self.runs[self.entries.len()].start_record)
    }

    pub fn accept_initial_head(
        &mut self,
        record: &[u8; SPOOL_RECORD_SIZE],
    ) -> Result<(), ExternalSortError> {
        let run_index = self.entries.len();
        if self.phase != Phase::Merge || run_index >= self.runs.len() {
            return Err(ExternalSortError::WrongPhase);
        }
        let head = decode_spool_entry(record)?;
        self.entries
            .push(head.entry)
            .map_err(|_| ExternalSortError::WorkspaceFull)?;
        self.runs[run_index].start_record += 1;
        self.runs[run_index].len -= 1;
        Ok(())
    }

    #[must_use]
    pub fn minimum_head(&self) -> Option<usize> {
        (self.phase == Phase::Merge).then_some(())?;
        (0..self.entries.len()).min_by(|left, right| {
            ascii_casefold_order(&self.entries[*left].name, &self.entries[*right].name)
        })
    }

    pub fn selected_head(&mut self, index: usize) -> Result<SpoolEntry, ExternalSortError> {
        if self.phase != Phase::Merge || index >= self.entries.len() {
            return Err(ExternalSortError::WrongPhase);
        }
        let entry = self.entries[index].clone();
        if let Some(previous) = &self.previous_name {
            match ascii_casefold_order(previous, &entry.name) {
                core::cmp::Ordering::Less => {}
                core::cmp::Ordering::Equal => {
                    return Err(ExternalSortError::InvalidSpoolRecord(
                        LibraryIndexError::DuplicateName,
                    ));
                }
                core::cmp::Ordering::Greater => {
                    return Err(ExternalSortError::InvalidSpoolRecord(
                        LibraryIndexError::Ordering,
                    ));
                }
            }
        }
        self.previous_name = Some(entry.name.clone());
        let mount_token = entry.mount_token;
        Ok(SpoolEntry { entry, mount_token })
    }

    #[must_use]
    pub fn replacement_record(&self, index: usize) -> Option<u16> {
        self.runs
            .get(index)
            .filter(|run| run.len != 0)
            .map(|run| run.start_record)
    }

    pub fn advance_head(
        &mut self,
        index: usize,
        replacement: Option<&[u8; SPOOL_RECORD_SIZE]>,
    ) -> Result<(), ExternalSortError> {
        if self.phase != Phase::Merge || index >= self.entries.len() {
            return Err(ExternalSortError::WrongPhase);
        }
        if self.runs[index].len == 0 {
            if replacement.is_some() {
                return Err(ExternalSortError::WrongPhase);
            }
            self.entries.remove(index);
            self.runs.remove(index);
        } else {
            let record = replacement.ok_or(ExternalSortError::WrongPhase)?;
            self.entries[index] = decode_spool_entry(record)?.entry;
            self.runs[index].start_record += 1;
            self.runs[index].len -= 1;
        }
        Ok(())
    }

    pub fn pop_min(
        &mut self,
        mut read: impl FnMut(u16, &mut [u8; SPOOL_RECORD_SIZE]) -> Result<(), ExternalSortError>,
    ) -> Result<Option<SpoolEntry>, ExternalSortError> {
        if self.phase != Phase::Merge {
            return Err(ExternalSortError::WrongPhase);
        }
        let Some(minimum) = self.minimum_head() else {
            return Ok(None);
        };
        let selected = self.selected_head(minimum)?;
        if let Some(next) = self.replacement_record(minimum) {
            let mut record = [0; SPOOL_RECORD_SIZE];
            read(next, &mut record)?;
            self.advance_head(minimum, Some(&record))?;
        } else {
            self.advance_head(minimum, None)?;
        }
        Ok(Some(selected))
    }
}

impl Default for LibraryRefreshWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

pub fn encode_spool_entry(entry: &LibraryEntry, out: &mut [u8]) {
    let token = entry.mount_token.map_or(0, MountFileToken::get);
    out[..2].copy_from_slice(&token.to_le_bytes());
    encode_index_entry(entry, (&mut out[2..]).try_into().unwrap());
}

pub fn decode_spool_entry(
    record: &[u8; SPOOL_RECORD_SIZE],
) -> Result<SpoolEntry, ExternalSortError> {
    let token = MountFileToken::new(u16::from_le_bytes([record[0], record[1]]));
    let mut entry = decode_index_entry(record[2..].try_into().unwrap())
        .map_err(ExternalSortError::InvalidSpoolRecord)?;
    entry.mount_token = token;
    Ok(SpoolEntry {
        entry,
        mount_token: token,
    })
}
