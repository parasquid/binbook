use core::cmp::Ordering;
use core::num::NonZeroU16;
use core::ops::Range;

use binbook_storage::{Filesystem, MAX_BINBOOK_NAME_BYTES};
use heapless::{String, Vec};

use crate::reader_state::{filename_hash, ReaderState};

pub const MAX_LIBRARY_ENTRIES: usize = 200;
pub const LIBRARY_VIEWPORT_CAPACITY: usize = 15;
pub const INDEX_HEADER_SIZE: usize = 16;
pub const INDEX_RECORD_SIZE: usize = 18 + MAX_BINBOOK_NAME_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MountFileToken(NonZeroU16);

impl MountFileToken {
    #[must_use]
    pub const fn new(value: u16) -> Option<Self> {
        match NonZeroU16::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }

    #[must_use]
    pub const fn for_mount_entry(generation: u8, index: u8) -> Option<Self> {
        if generation == 0 || index >= 200 {
            return None;
        }
        Self::new(((generation as u16) << 8) | (index as u16 + 1))
    }

    #[must_use]
    pub const fn mount_generation(self) -> u8 {
        (self.get() >> 8) as u8
    }

    #[must_use]
    pub const fn entry_index(self) -> Option<usize> {
        let encoded = (self.get() & 0xff) as u8;
        if encoded == 0 || encoded > 200 {
            None
        } else {
            Some((encoded - 1) as usize)
        }
    }
}

const INDEX_MAGIC: &[u8; 4] = b"BBLI";
const INDEX_VERSION: u16 = 1;
const INDEX_CRC_OFFSET: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryStatus {
    NoCard,
    CardError,
    NoBooks,
    Ready,
    TooManyBooks,
    UnsupportedName,
}

impl LibraryStatus {
    pub const fn code(self) -> u8 {
        match self {
            Self::Ready => 0,
            Self::NoCard => 1,
            Self::CardError => 2,
            Self::NoBooks => 3,
            Self::TooManyBooks => 4,
            Self::UnsupportedName => 5,
        }
    }

    pub const fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Ready,
            1 => Self::NoCard,
            2 => Self::CardError,
            3 => Self::NoBooks,
            4 => Self::TooManyBooks,
            5 => Self::UnsupportedName,
            _ => Self::CardError,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanErrorKind {
    NoCard,
    CardError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryEntry {
    pub name: String<MAX_BINBOOK_NAME_BYTES>,
    pub size: u64,
    pub filename_hash: u64,
    /// Valid only for the current mounted-filesystem generation.
    pub mount_token: Option<MountFileToken>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    entries: Vec<LibraryEntry, MAX_LIBRARY_ENTRIES>,
    status: LibraryStatus,
}

pub struct LibraryViewport {
    start: usize,
    total: usize,
    entries: Vec<LibraryEntry, LIBRARY_VIEWPORT_CAPACITY>,
    valid: bool,
    cursor: Option<LibraryIndexCursor>,
}

impl LibraryViewport {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            start: 0,
            total: 0,
            entries: Vec::new(),
            valid: false,
            cursor: None,
        }
    }

    #[must_use]
    pub const fn start(&self) -> usize {
        self.start
    }

    #[must_use]
    pub const fn total(&self) -> usize {
        self.total
    }

    #[must_use]
    pub fn entries(&self) -> &[LibraryEntry] {
        &self.entries
    }

    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.valid
    }

    #[must_use]
    pub const fn cursor(&self) -> Option<LibraryIndexCursor> {
        self.cursor
    }

    #[cfg(feature = "sd-storage")]
    pub(crate) fn begin(&mut self, start: usize) {
        self.start = start;
        self.total = 0;
        self.entries.clear();
        self.valid = false;
        self.cursor = None;
    }

    #[cfg(feature = "sd-storage")]
    pub(crate) fn push(&mut self, entry: LibraryEntry) -> Result<(), LibraryIndexError> {
        self.entries
            .push(entry)
            .map_err(|_| LibraryIndexError::TooManyEntries)
    }

    #[cfg(feature = "sd-storage")]
    pub(crate) fn finish(&mut self, cursor: LibraryIndexCursor) {
        self.total = cursor.entry_count();
        self.valid = true;
        self.cursor = Some(cursor);
    }

    #[cfg(feature = "sd-storage")]
    pub(crate) fn fail(&mut self) {
        self.entries.clear();
        self.total = 0;
        self.valid = false;
        self.cursor = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshOutcome {
    pub scan_status: LibraryStatus,
    pub adopted: bool,
    pub changed: bool,
}

pub struct IncrementalLibraryScan<'a> {
    library: &'a mut Library,
    unsupported_name: bool,
    overflow: bool,
}

impl<'a> IncrementalLibraryScan<'a> {
    pub fn begin(library: &'a mut Library) -> Self {
        library.entries.clear();
        // An unfinished/dropped builder must never be adoptable as a refresh.
        library.status = LibraryStatus::CardError;
        Self {
            library,
            unsupported_name: false,
            overflow: false,
        }
    }

    pub fn push(&mut self, name: &str, size: u64) {
        self.push_mounted(name, size, None);
    }

    pub fn push_mounted(&mut self, name: &str, size: u64, mount_token: Option<MountFileToken>) {
        if !name.ends_with(".binbook") {
            return;
        }
        let Ok(name) = String::<MAX_BINBOOK_NAME_BYTES>::try_from(name) else {
            self.unsupported_name = true;
            return;
        };
        let entry = LibraryEntry {
            filename_hash: filename_hash(name.as_bytes()),
            name,
            size,
            mount_token,
        };
        if self.library.entries.push(entry).is_err() {
            self.overflow = true;
        }
    }

    pub fn finish(self) {
        self.library
            .entries
            .as_mut_slice()
            .sort_unstable_by(|left, right| ascii_casefold_order(&left.name, &right.name));
        self.library.status = if self.unsupported_name {
            LibraryStatus::UnsupportedName
        } else if self.overflow {
            LibraryStatus::TooManyBooks
        } else if self.library.entries.is_empty() {
            LibraryStatus::NoBooks
        } else {
            LibraryStatus::Ready
        };
    }

    pub fn fail(self, status: LibraryStatus) {
        self.library.entries.clear();
        self.library.status = if matches!(
            status,
            LibraryStatus::NoCard
                | LibraryStatus::CardError
                | LibraryStatus::TooManyBooks
                | LibraryStatus::UnsupportedName
        ) {
            status
        } else {
            LibraryStatus::CardError
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryIndexError {
    BufferTooSmall,
    RecordIndex,
    Status,
    Magic,
    Version,
    HeaderSize,
    Reserved,
    TooManyEntries,
    Length,
    Crc,
    NameLength,
    NameEncoding,
    NamePadding,
    Extension,
    FilenameHash,
    DuplicateName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibraryIndexCursor {
    entry_count: u16,
}

impl LibraryIndexCursor {
    pub fn from_header(
        header: &[u8; INDEX_HEADER_SIZE],
        file_len: usize,
    ) -> Result<Self, LibraryIndexError> {
        validate_index_header(header, file_len)?;
        Ok(Self {
            entry_count: read_u16(header, 8),
        })
    }

    #[must_use]
    pub const fn entry_count(self) -> usize {
        self.entry_count as usize
    }

    pub fn record_range(self, index: usize) -> Result<Range<usize>, LibraryIndexError> {
        if index >= self.entry_count() {
            return Err(LibraryIndexError::RecordIndex);
        }
        let start = INDEX_HEADER_SIZE + index * INDEX_RECORD_SIZE;
        Ok(start..start + INDEX_RECORD_SIZE)
    }

    pub fn decode_record(
        self,
        record: &[u8; INDEX_RECORD_SIZE],
    ) -> Result<LibraryEntry, LibraryIndexError> {
        decode_index_entry(record)
    }
}

pub struct LibraryIndexCursorValidator {
    cursor: LibraryIndexCursor,
    next_record: usize,
    stored_crc: u32,
    crc_state: u32,
}

impl LibraryIndexCursorValidator {
    #[must_use]
    pub fn new(cursor: LibraryIndexCursor, header: &[u8; INDEX_HEADER_SIZE]) -> Self {
        let mut crc_state = u32::MAX;
        for (index, byte) in header.iter().copied().enumerate() {
            let byte = if (INDEX_CRC_OFFSET..INDEX_CRC_OFFSET + 4).contains(&index) {
                0
            } else {
                byte
            };
            crc_state = crate::crc32::update_byte(crc_state, byte);
        }
        Self {
            cursor,
            next_record: 0,
            stored_crc: read_u32(header, INDEX_CRC_OFFSET),
            crc_state,
        }
    }

    pub fn push_record(
        &mut self,
        record: &[u8; INDEX_RECORD_SIZE],
    ) -> Result<(), LibraryIndexError> {
        if self.next_record >= self.cursor.entry_count() {
            return Err(LibraryIndexError::Length);
        }
        decode_index_entry(record)?;
        for byte in record {
            self.crc_state = crate::crc32::update_byte(self.crc_state, *byte);
        }
        self.next_record += 1;
        Ok(())
    }

    pub fn finish(self) -> Result<(), LibraryIndexError> {
        if self.next_record != self.cursor.entry_count() {
            return Err(LibraryIndexError::Length);
        }
        if !self.crc_state != self.stored_crc {
            return Err(LibraryIndexError::Crc);
        }
        Ok(())
    }
}

pub struct LibraryIndexDecoder<'a> {
    library: &'a mut Library,
    expected_count: usize,
    next_record: usize,
    stored_crc: u32,
    crc_state: u32,
}

impl<'a> LibraryIndexDecoder<'a> {
    pub fn new(
        header: &[u8; INDEX_HEADER_SIZE],
        file_len: usize,
        library: &'a mut Library,
    ) -> Result<Self, LibraryIndexError> {
        if let Err(error) = validate_index_header(header, file_len) {
            IncrementalLibraryScan::begin(library).fail(LibraryStatus::CardError);
            return Err(error);
        }
        let expected_count = usize::from(read_u16(header, 8));
        library.entries.clear();
        library.status = LibraryStatus::NoBooks;
        let mut decoder = Self {
            library,
            expected_count,
            next_record: 0,
            stored_crc: read_u32(header, INDEX_CRC_OFFSET),
            crc_state: u32::MAX,
        };
        for (index, byte) in header.iter().copied().enumerate() {
            let byte = if (INDEX_CRC_OFFSET..INDEX_CRC_OFFSET + 4).contains(&index) {
                0
            } else {
                byte
            };
            decoder.crc_state = crate::crc32::update_byte(decoder.crc_state, byte);
        }
        Ok(decoder)
    }

    pub fn push_record(
        &mut self,
        record: &[u8; INDEX_RECORD_SIZE],
    ) -> Result<(), LibraryIndexError> {
        if self.next_record >= self.expected_count {
            return Err(LibraryIndexError::Length);
        }
        for byte in record {
            self.crc_state = crate::crc32::update_byte(self.crc_state, *byte);
        }
        self.library
            .entries
            .push(decode_index_entry(record)?)
            .map_err(|_| LibraryIndexError::TooManyEntries)?;
        self.next_record += 1;
        Ok(())
    }

    pub fn finish(self) -> Result<(), LibraryIndexError> {
        if self.next_record != self.expected_count {
            IncrementalLibraryScan::begin(self.library).fail(LibraryStatus::CardError);
            return Err(LibraryIndexError::Length);
        }
        if !self.crc_state != self.stored_crc {
            IncrementalLibraryScan::begin(self.library).fail(LibraryStatus::CardError);
            return Err(LibraryIndexError::Crc);
        }
        self.library
            .entries
            .as_mut_slice()
            .sort_unstable_by(|left, right| ascii_casefold_order(&left.name, &right.name));
        if self
            .library
            .entries
            .windows(2)
            .any(|pair| pair[0].name == pair[1].name)
        {
            IncrementalLibraryScan::begin(self.library).fail(LibraryStatus::CardError);
            return Err(LibraryIndexError::DuplicateName);
        }
        self.library.status = if self.library.entries.is_empty() {
            LibraryStatus::NoBooks
        } else {
            LibraryStatus::Ready
        };
        Ok(())
    }

    pub fn fail(self, status: LibraryStatus) {
        IncrementalLibraryScan::begin(self.library).fail(status);
    }
}

impl Library {
    pub const fn empty() -> Self {
        Self {
            entries: Vec::new(),
            status: LibraryStatus::NoBooks,
        }
    }

    pub fn scan<F: Filesystem>(
        fs: &mut F,
        classify_error: impl FnOnce(F::Error) -> ScanErrorKind,
    ) -> Self {
        let mut library = Self::empty();
        library.scan_in_place(fs, classify_error);
        library
    }

    pub fn scan_in_place<F: Filesystem>(
        &mut self,
        fs: &mut F,
        classify_error: impl FnOnce(F::Error) -> ScanErrorKind,
    ) {
        self.entries.clear();
        let mut unsupported_name = false;
        let mut overflow = false;
        let result = fs.for_each_entry(&mut |name, size| {
            if !name.ends_with(".binbook") {
                return;
            }
            let Ok(name) = String::<MAX_BINBOOK_NAME_BYTES>::try_from(name) else {
                unsupported_name = true;
                return;
            };
            let entry = LibraryEntry {
                filename_hash: filename_hash(name.as_bytes()),
                name,
                size,
                mount_token: None,
            };
            if self.entries.push(entry).is_err() {
                overflow = true;
            }
        });
        if let Err(error) = result {
            self.entries.clear();
            self.status = match classify_error(error) {
                ScanErrorKind::NoCard => LibraryStatus::NoCard,
                ScanErrorKind::CardError => LibraryStatus::CardError,
            };
            return;
        }
        self.entries
            .as_mut_slice()
            .sort_unstable_by(|left, right| ascii_casefold_order(&left.name, &right.name));
        self.status = if unsupported_name {
            LibraryStatus::UnsupportedName
        } else if overflow {
            LibraryStatus::TooManyBooks
        } else if self.entries.is_empty() {
            LibraryStatus::NoBooks
        } else {
            LibraryStatus::Ready
        };
    }

    pub fn entries(&self) -> &[LibraryEntry] {
        &self.entries
    }

    pub const fn status(&self) -> LibraryStatus {
        self.status
    }

    pub const fn max_index_bytes() -> usize {
        INDEX_HEADER_SIZE + MAX_LIBRARY_ENTRIES * INDEX_RECORD_SIZE
    }

    pub fn index_encoded_len(&self) -> usize {
        INDEX_HEADER_SIZE + self.entries.len() * INDEX_RECORD_SIZE
    }

    pub fn encode_index(&self, out: &mut [u8]) -> Result<usize, LibraryIndexError> {
        let used = self.index_encoded_len();
        if out.len() < used {
            return Err(LibraryIndexError::BufferTooSmall);
        }
        let mut header = [0; INDEX_HEADER_SIZE];
        self.encode_index_header(&mut header)?;
        out[..INDEX_HEADER_SIZE].copy_from_slice(&header);
        let mut record = [0; INDEX_RECORD_SIZE];
        for index in 0..self.entries.len() {
            let start = INDEX_HEADER_SIZE + index * INDEX_RECORD_SIZE;
            self.encode_index_record(index, &mut record)?;
            out[start..start + INDEX_RECORD_SIZE].copy_from_slice(&record);
        }
        Ok(used)
    }

    pub fn encode_index_header(
        &self,
        out: &mut [u8; INDEX_HEADER_SIZE],
    ) -> Result<(), LibraryIndexError> {
        self.validate_index_status()?;
        fill_index_header(self.entries.len(), out);
        let mut crc_state = index_crc_update(u32::MAX, out);
        let mut record = [0; INDEX_RECORD_SIZE];
        for index in 0..self.entries.len() {
            self.encode_index_record(index, &mut record)?;
            crc_state = index_crc_update(crc_state, &record);
        }
        out[INDEX_CRC_OFFSET..INDEX_CRC_OFFSET + 4].copy_from_slice(&(!crc_state).to_le_bytes());
        Ok(())
    }

    pub fn encode_index_record(
        &self,
        index: usize,
        out: &mut [u8; INDEX_RECORD_SIZE],
    ) -> Result<(), LibraryIndexError> {
        self.validate_index_status()?;
        let entry = self
            .entries
            .get(index)
            .ok_or(LibraryIndexError::RecordIndex)?;
        out.fill(0);
        out[0] = entry.name.len() as u8;
        out[2..10].copy_from_slice(&entry.size.to_le_bytes());
        out[10..18].copy_from_slice(&entry.filename_hash.to_le_bytes());
        out[18..18 + entry.name.len()].copy_from_slice(entry.name.as_bytes());
        Ok(())
    }

    fn validate_index_status(&self) -> Result<(), LibraryIndexError> {
        if matches!(self.status, LibraryStatus::Ready | LibraryStatus::NoBooks) {
            Ok(())
        } else {
            Err(LibraryIndexError::Status)
        }
    }

    pub fn decode_index(data: &[u8]) -> Result<Self, LibraryIndexError> {
        if data.len() < INDEX_HEADER_SIZE {
            return Err(LibraryIndexError::Length);
        }
        let header: &[u8; INDEX_HEADER_SIZE] = data[..INDEX_HEADER_SIZE].try_into().unwrap();
        let mut library = Library::empty();
        {
            let mut decoder = LibraryIndexDecoder::new(header, data.len(), &mut library)?;
            for record in data[INDEX_HEADER_SIZE..].chunks_exact(INDEX_RECORD_SIZE) {
                decoder.push_record(record.try_into().unwrap())?;
            }
            decoder.finish()?;
        }
        Ok(library)
    }

    /// Atomically adopt a complete usable scan while retaining a valid cache
    /// across transient or structurally invalid scan results.
    pub fn apply_refresh(&mut self, scanned: Self) -> RefreshOutcome {
        let scan_status = scanned.status;
        if !matches!(scan_status, LibraryStatus::Ready | LibraryStatus::NoBooks) {
            return RefreshOutcome {
                scan_status,
                adopted: false,
                changed: false,
            };
        }
        let changed = !self.visible_content_eq(&scanned);
        *self = scanned;
        RefreshOutcome {
            scan_status,
            adopted: true,
            changed,
        }
    }

    /// Adopt a completed scan by swapping caller-owned buffers. On success,
    /// `scanned` receives the previous cache and can be reused for the next
    /// refresh without allocating another worst-case library.
    pub fn apply_refresh_in_place(&mut self, scanned: &mut Self) -> RefreshOutcome {
        let scan_status = scanned.status;
        if !matches!(scan_status, LibraryStatus::Ready | LibraryStatus::NoBooks) {
            return RefreshOutcome {
                scan_status,
                adopted: false,
                changed: false,
            };
        }
        let changed = !self.visible_content_eq(scanned);
        core::mem::swap(self, scanned);
        RefreshOutcome {
            scan_status,
            adopted: true,
            changed,
        }
    }

    pub fn index_for_filename_hash(&self, hash: u64) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.filename_hash == hash)
    }

    fn visible_content_eq(&self, other: &Self) -> bool {
        self.status == other.status
            && self.entries.len() == other.entries.len()
            && self
                .entries
                .iter()
                .zip(&other.entries)
                .all(|(left, right)| {
                    left.name == right.name
                        && left.size == right.size
                        && left.filename_hash == right.filename_hash
                })
    }

    pub fn prune_state(&self, state: &mut ReaderState) -> bool {
        if !matches!(self.status, LibraryStatus::Ready | LibraryStatus::NoBooks) {
            return false;
        }
        state.prune(|hash| self.entries.iter().any(|entry| entry.filename_hash == hash));
        true
    }
}

fn fill_index_header(count: usize, out: &mut [u8; INDEX_HEADER_SIZE]) {
    out.fill(0);
    out[..4].copy_from_slice(INDEX_MAGIC);
    out[4..6].copy_from_slice(&INDEX_VERSION.to_le_bytes());
    out[6..8].copy_from_slice(&(INDEX_HEADER_SIZE as u16).to_le_bytes());
    out[8..10].copy_from_slice(&(count as u16).to_le_bytes());
}

fn index_crc_update(mut state: u32, data: &[u8]) -> u32 {
    for byte in data {
        let byte = *byte;
        state = crate::crc32::update_byte(state, byte);
    }
    state
}

fn validate_index_header(
    header: &[u8; INDEX_HEADER_SIZE],
    file_len: usize,
) -> Result<(), LibraryIndexError> {
    if &header[..4] != INDEX_MAGIC {
        return Err(LibraryIndexError::Magic);
    }
    if read_u16(header, 4) != INDEX_VERSION {
        return Err(LibraryIndexError::Version);
    }
    if usize::from(read_u16(header, 6)) != INDEX_HEADER_SIZE {
        return Err(LibraryIndexError::HeaderSize);
    }
    if header[10..12] != [0; 2] {
        return Err(LibraryIndexError::Reserved);
    }
    let count = usize::from(read_u16(header, 8));
    if count > MAX_LIBRARY_ENTRIES {
        return Err(LibraryIndexError::TooManyEntries);
    }
    if file_len != INDEX_HEADER_SIZE + count * INDEX_RECORD_SIZE {
        return Err(LibraryIndexError::Length);
    }
    Ok(())
}

fn decode_index_entry(record: &[u8; INDEX_RECORD_SIZE]) -> Result<LibraryEntry, LibraryIndexError> {
    if record[1] != 0 {
        return Err(LibraryIndexError::Reserved);
    }
    let name_len = usize::from(record[0]);
    if name_len == 0 || name_len > MAX_BINBOOK_NAME_BYTES {
        return Err(LibraryIndexError::NameLength);
    }
    let name_end = 18 + name_len;
    if record[name_end..].iter().any(|byte| *byte != 0) {
        return Err(LibraryIndexError::NamePadding);
    }
    let name =
        core::str::from_utf8(&record[18..name_end]).map_err(|_| LibraryIndexError::NameEncoding)?;
    if !name.ends_with(".binbook") {
        return Err(LibraryIndexError::Extension);
    }
    let stored_hash = read_u64(record, 10);
    if stored_hash != filename_hash(name.as_bytes()) {
        return Err(LibraryIndexError::FilenameHash);
    }
    Ok(LibraryEntry {
        name: String::try_from(name).map_err(|_| LibraryIndexError::NameLength)?,
        size: read_u64(record, 2),
        filename_hash: stored_hash,
        mount_token: None,
    })
}

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap())
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn read_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

fn ascii_casefold_order(left: &str, right: &str) -> Ordering {
    left.bytes()
        .map(ascii_lower)
        .cmp(right.bytes().map(ascii_lower))
        .then_with(|| left.as_bytes().cmp(right.as_bytes()))
}

const fn ascii_lower(byte: u8) -> u8 {
    if byte.is_ascii_uppercase() {
        byte + (b'a' - b'A')
    } else {
        byte
    }
}
