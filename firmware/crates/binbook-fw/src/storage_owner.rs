//! Fixed-capacity protocol for the single runtime storage owner.

use binbook_storage::MAX_BINBOOK_NAME_BYTES;
use heapless::{String, Vec};

pub const MAX_BOUNDED_IO_BYTES: u16 = 4096;
pub const SCAN_ENTRY_BUDGET: u8 = 8;
pub const SCAN_RECORD_BYTES: usize = 1 + binbook_storage::MAX_BINBOOK_NAME_BYTES + 8 + 2;
pub const SCAN_BATCH_BYTES: usize = SCAN_RECORD_BYTES * SCAN_ENTRY_BUDGET as usize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanRecord {
    pub name: StorageName,
    pub size: u64,
    pub mount_token: Option<crate::library::MountFileToken>,
}

pub fn encode_scan_records(records: &[ScanRecord], out: &mut [u8]) -> Result<(), RequestError> {
    if records.len() > SCAN_ENTRY_BUDGET as usize || out.len() < SCAN_BATCH_BYTES {
        return Err(RequestError::TooLarge);
    }
    out[..SCAN_BATCH_BYTES].fill(0);
    for (index, record) in records.iter().enumerate() {
        let start = index * SCAN_RECORD_BYTES;
        let name = record.name.as_str().as_bytes();
        out[start] = name.len() as u8;
        out[start + 1..start + 1 + name.len()].copy_from_slice(name);
        let size_offset = start + 1 + binbook_storage::MAX_BINBOOK_NAME_BYTES;
        out[size_offset..size_offset + 8].copy_from_slice(&record.size.to_le_bytes());
        let token = record
            .mount_token
            .map_or(0, crate::library::MountFileToken::get);
        out[size_offset + 8..size_offset + 10].copy_from_slice(&token.to_le_bytes());
    }
    Ok(())
}

pub fn decode_scan_records(
    count: u8,
    bytes: &[u8],
) -> Result<Vec<ScanRecord, { SCAN_ENTRY_BUDGET as usize }>, RequestError> {
    if count > SCAN_ENTRY_BUDGET || bytes.len() < SCAN_BATCH_BYTES {
        return Err(RequestError::TooLarge);
    }
    let mut records = Vec::new();
    for index in 0..usize::from(count) {
        let start = index * SCAN_RECORD_BYTES;
        let name_len = usize::from(bytes[start]);
        if name_len == 0 || name_len > binbook_storage::MAX_BINBOOK_NAME_BYTES {
            return Err(RequestError::NameTooLong);
        }
        let name = core::str::from_utf8(&bytes[start + 1..start + 1 + name_len])
            .map_err(|_| RequestError::NameTooLong)?;
        let size_offset = start + 1 + binbook_storage::MAX_BINBOOK_NAME_BYTES;
        let size = u64::from_le_bytes(
            bytes[size_offset..size_offset + 8]
                .try_into()
                .map_err(|_| RequestError::TooLarge)?,
        );
        let token = u16::from_le_bytes(
            bytes[size_offset + 8..size_offset + 10]
                .try_into()
                .map_err(|_| RequestError::TooLarge)?,
        );
        records
            .push(ScanRecord {
                name: StorageName::new(name)?,
                size,
                mount_token: crate::library::MountFileToken::new(token),
            })
            .map_err(|_| RequestError::TooLarge)?;
    }
    Ok(records)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageName(String<MAX_BINBOOK_NAME_BYTES>);

impl StorageName {
    pub fn new(value: &str) -> Result<Self, RequestError> {
        String::try_from(value)
            .map(Self)
            .map_err(|_| RequestError::NameTooLong)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferSlot {
    Reader,
    State,
    Diagnostic,
    LibraryIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequestPriority {
    Background,
    Foreground,
    PowerSave,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanToken(pub u32);

impl ScanToken {
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    ActivateFile {
        name: StorageName,
        token: crate::library::MountFileToken,
    },
    FileSize {
        name: StorageName,
    },
    Read {
        name: StorageName,
        offset: u64,
        buffer_offset: u16,
        len: u16,
        buffer: BufferSlot,
    },
    UploadBegin {
        name: StorageName,
        expected_size: u64,
        power_save: bool,
    },
    UploadWrite {
        upload_id: u32,
        offset: u64,
        buffer_offset: u16,
        len: u16,
        buffer: BufferSlot,
        power_save: bool,
    },
    UploadCommit {
        upload_id: u32,
        power_save: bool,
    },
    UploadAbort {
        upload_id: u32,
    },
    Delete {
        name: StorageName,
    },
    ScanStart,
    ScanPrefetch {
        token: ScanToken,
        name: StorageName,
    },
    ScanStep {
        token: ScanToken,
        buffer: BufferSlot,
    },
    ScanFinish {
        token: ScanToken,
    },
    ScanCancel {
        token: ScanToken,
    },
}

impl Request {
    pub fn read(
        name: StorageName,
        offset: u64,
        len: u16,
        buffer: BufferSlot,
    ) -> Result<Self, RequestError> {
        validate_len(len)?;
        Ok(Self::Read {
            name,
            offset,
            buffer_offset: 0,
            len,
            buffer,
        })
    }

    pub fn read_into(
        name: StorageName,
        offset: u64,
        buffer_offset: u16,
        len: u16,
        buffer: BufferSlot,
    ) -> Result<Self, RequestError> {
        validate_len(len)?;
        Ok(Self::Read {
            name,
            offset,
            buffer_offset,
            len,
            buffer,
        })
    }

    #[must_use]
    pub const fn scan_step(token: ScanToken) -> Self {
        Self::ScanStep {
            token,
            buffer: BufferSlot::LibraryIndex,
        }
    }

    #[must_use]
    pub const fn upload_commit(upload_id: u32, power_save: bool) -> Self {
        Self::UploadCommit {
            upload_id,
            power_save,
        }
    }

    #[must_use]
    pub const fn priority(&self) -> RequestPriority {
        match self {
            Self::ScanStart
            | Self::ScanPrefetch { .. }
            | Self::ScanStep { .. }
            | Self::ScanFinish { .. }
            | Self::ScanCancel { .. } => RequestPriority::Background,
            Self::UploadBegin {
                power_save: true, ..
            }
            | Self::UploadWrite {
                power_save: true, ..
            }
            | Self::UploadCommit {
                power_save: true, ..
            } => RequestPriority::PowerSave,
            _ => RequestPriority::Foreground,
        }
    }

    #[must_use]
    pub const fn is_background(&self) -> bool {
        matches!(self.priority(), RequestPriority::Background)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedRequest {
    pub sequence: u32,
    pub request: Request,
}

pub struct StorageRequestQueue<const N: usize> {
    pending: Vec<QueuedRequest, N>,
}

impl<const N: usize> StorageRequestQueue<N> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    pub fn push(&mut self, sequence: u32, request: Request) -> Result<(), RequestError> {
        self.pending
            .push(QueuedRequest { sequence, request })
            .map_err(|_| RequestError::Busy)
    }

    pub fn pop(&mut self) -> Option<QueuedRequest> {
        let mut selected = 0usize;
        for index in 1..self.pending.len() {
            if self.pending[index].request.priority() > self.pending[selected].request.priority() {
                selected = index;
            }
        }
        if self.pending.is_empty() {
            None
        } else {
            Some(self.pending.remove(selected))
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

impl<const N: usize> Default for StorageRequestQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageClient {
    Reader,
    State,
    Diagnostic,
    Library,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestEnvelope {
    pub client: StorageClient,
    pub sequence: u32,
    pub request: Request,
}

pub struct StorageMailbox<const N: usize> {
    pending: Vec<RequestEnvelope, N>,
}

impl<const N: usize> StorageMailbox<N> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    pub fn push(&mut self, request: RequestEnvelope) -> Result<(), RequestError> {
        self.pending.push(request).map_err(|_| RequestError::Busy)
    }

    pub fn pop(&mut self) -> Option<RequestEnvelope> {
        if self.pending.is_empty() {
            return None;
        }
        let mut selected = 0usize;
        for index in 1..self.pending.len() {
            if self.pending[index].request.priority() > self.pending[selected].request.priority() {
                selected = index;
            }
        }
        Some(self.pending.remove(selected))
    }
}

impl<const N: usize> Default for StorageMailbox<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionError {
    Request(RequestError),
    Backend,
    BufferUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseEnvelope {
    pub client: StorageClient,
    pub sequence: u32,
    pub response: Result<Response, CompletionError>,
}

impl ResponseEnvelope {
    #[must_use]
    pub fn success(request: &RequestEnvelope, response: Response) -> Self {
        Self {
            client: request.client,
            sequence: request.sequence,
            response: Ok(response),
        }
    }

    #[must_use]
    pub fn failure(request: &RequestEnvelope, error: CompletionError) -> Self {
        Self {
            client: request.client,
            sequence: request.sequence,
            response: Err(error),
        }
    }
}

#[derive(Debug)]
pub struct BufferLease<'a> {
    slot: BufferSlot,
    bytes: &'a mut [u8],
}

impl<'a> BufferLease<'a> {
    pub const fn new(slot: BufferSlot, bytes: &'a mut [u8]) -> Self {
        Self { slot, bytes }
    }

    #[must_use]
    pub const fn slot(&self) -> BufferSlot {
        self.slot
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.bytes
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        self.bytes
    }

    pub fn into_bytes(self) -> &'a mut [u8] {
        self.bytes
    }
}

#[derive(Debug)]
pub struct OwnedRequest<'a> {
    pub envelope: RequestEnvelope,
    pub buffer: Option<BufferLease<'a>>,
}

pub struct OwnedResponse<'a> {
    pub envelope: ResponseEnvelope,
    pub buffer: Option<BufferLease<'a>>,
}

/// Reject a request when the runtime owner cannot access its backend while
/// preserving the request correlation and returning any caller-owned lease.
pub fn reject_owned_request<'a>(
    request: OwnedRequest<'a>,
    error: CompletionError,
) -> OwnedResponse<'a> {
    OwnedResponse {
        envelope: ResponseEnvelope::failure(&request.envelope, error),
        buffer: request.buffer,
    }
}

pub struct OwnedMailbox<'a, const N: usize> {
    pending: Vec<OwnedRequest<'a>, N>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageOwnerPhase {
    Queued {
        client: StorageClient,
        sequence: u32,
    },
    Executing {
        client: StorageClient,
        sequence: u32,
    },
    Completed {
        client: StorageClient,
        sequence: u32,
    },
}

/// Synchronous core of the runtime storage task.
///
/// Keeping queueing, priority selection, execution, and correlation here makes
/// the task's request handshake host-testable. The Embassy wrapper is only
/// responsible for channel receive/send and yielding between completions.
pub struct StorageOwnerPump<'a, B, const N: usize> {
    owner: StorageOwner<B>,
    mailbox: OwnedMailbox<'a, N>,
}

impl<'a, B: OwnerBackend, const N: usize> StorageOwnerPump<'a, B, N> {
    pub const fn new(owner: StorageOwner<B>) -> Self {
        Self {
            owner,
            mailbox: OwnedMailbox::new(),
        }
    }

    #[allow(clippy::result_large_err)]
    pub fn enqueue(
        &mut self,
        request: OwnedRequest<'a>,
        mut observe: impl FnMut(StorageOwnerPhase),
    ) -> Result<(), OwnedRequest<'a>> {
        let client = request.envelope.client;
        let sequence = request.envelope.sequence;
        self.mailbox.push(request)?;
        observe(StorageOwnerPhase::Queued { client, sequence });
        Ok(())
    }

    pub fn execute_next(
        &mut self,
        mut observe: impl FnMut(StorageOwnerPhase),
    ) -> Option<OwnedResponse<'a>> {
        let request = if let Some(client) = self.owner.active_upload_client() {
            self.mailbox.pop_client(client)?
        } else {
            self.mailbox.pop()?
        };
        let client = request.envelope.client;
        let sequence = request.envelope.sequence;
        observe(StorageOwnerPhase::Executing { client, sequence });
        let response = self.owner.execute_owned(request);
        observe(StorageOwnerPhase::Completed { client, sequence });
        Some(response)
    }

    pub fn execute_immediate(
        &mut self,
        request: OwnedRequest<'a>,
        mut observe: impl FnMut(StorageOwnerPhase),
    ) -> OwnedResponse<'a> {
        let client = request.envelope.client;
        let sequence = request.envelope.sequence;
        observe(StorageOwnerPhase::Executing { client, sequence });
        let response = self.owner.execute_owned(request);
        observe(StorageOwnerPhase::Completed { client, sequence });
        response
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mailbox.is_empty()
    }

    pub fn into_owner(self) -> StorageOwner<B> {
        self.owner
    }
}

impl<'a, const N: usize> OwnedMailbox<'a, N> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    #[allow(clippy::result_large_err)]
    pub fn push(&mut self, request: OwnedRequest<'a>) -> Result<(), OwnedRequest<'a>> {
        self.pending.push(request)
    }

    pub fn pop(&mut self) -> Option<OwnedRequest<'a>> {
        if self.pending.is_empty() {
            return None;
        }
        let mut selected = 0usize;
        for index in 1..self.pending.len() {
            if self.pending[index].envelope.request.priority()
                > self.pending[selected].envelope.request.priority()
            {
                selected = index;
            }
        }
        Some(self.pending.remove(selected))
    }

    pub fn pop_client(&mut self, client: StorageClient) -> Option<OwnedRequest<'a>> {
        let mut selected: Option<usize> = None;
        for (index, request) in self.pending.iter().enumerate() {
            if request.envelope.client != client {
                continue;
            }
            if selected.is_none_or(|current| {
                request.envelope.request.priority()
                    > self.pending[current].envelope.request.priority()
            }) {
                selected = Some(index);
            }
        }
        Some(self.pending.remove(selected?))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

impl<const N: usize> Default for OwnedMailbox<'_, N> {
    fn default() -> Self {
        Self::new()
    }
}

struct LeasePool<'a> {
    lease: Option<BufferLease<'a>>,
}

impl BufferPool for LeasePool<'_> {
    fn buffer_mut(&mut self, slot: BufferSlot) -> Option<&mut [u8]> {
        let lease = self.lease.as_mut()?;
        if lease.slot == slot {
            Some(lease.bytes_mut())
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanProgress {
    pub entries: u8,
    pub complete: bool,
}

pub trait BufferPool {
    fn buffer_mut(&mut self, slot: BufferSlot) -> Option<&mut [u8]>;
}

pub trait OwnerBackend {
    type Error;
    fn activate_file(
        &mut self,
        name: &str,
        _token: crate::library::MountFileToken,
    ) -> Result<u64, Self::Error> {
        self.file_size(name)
    }
    fn file_size(&mut self, name: &str) -> Result<u64, Self::Error>;
    fn read_at(&mut self, name: &str, offset: u64, out: &mut [u8]) -> Result<(), Self::Error>;
    fn upload_begin(&mut self, name: &str, expected_size: u64) -> Result<(), Self::Error>;
    fn upload_write(&mut self, name: &str, offset: u64, data: &[u8]) -> Result<(), Self::Error>;
    fn upload_commit(&mut self, name: &str) -> Result<(), Self::Error>;
    fn upload_abort(&mut self, name: &str) -> Result<(), Self::Error>;
    fn delete(&mut self, name: &str) -> Result<(), Self::Error>;
    fn scan_start(&mut self) -> Result<(), Self::Error>;
    fn scan_prefetch(&mut self, name: &str) -> Result<(), Self::Error>;
    fn scan_step(&mut self, budget: u8, out: &mut [u8]) -> Result<ScanProgress, Self::Error>;
    fn scan_finish(&mut self) -> Result<(), Self::Error>;
    fn scan_cancel(&mut self) -> Result<(), Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum FilesystemOwnerError<E> {
    Backend(E),
    IncrementalScanUnavailable,
}

pub struct FilesystemOwnerBackend<F> {
    filesystem: F,
}

impl<F> FilesystemOwnerBackend<F> {
    pub const fn new(filesystem: F) -> Self {
        Self { filesystem }
    }

    pub fn into_inner(self) -> F {
        self.filesystem
    }
}

impl<F: binbook_storage::WritableFilesystem> OwnerBackend for FilesystemOwnerBackend<F> {
    type Error = FilesystemOwnerError<F::Error>;

    fn activate_file(
        &mut self,
        name: &str,
        _token: crate::library::MountFileToken,
    ) -> Result<u64, Self::Error> {
        self.file_size(name)
    }

    fn file_size(&mut self, name: &str) -> Result<u64, Self::Error> {
        self.filesystem
            .file_size(name)
            .map_err(FilesystemOwnerError::Backend)
    }

    fn read_at(&mut self, name: &str, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        self.filesystem
            .read_at(name, offset, out)
            .map_err(FilesystemOwnerError::Backend)
    }

    fn upload_begin(&mut self, name: &str, expected_size: u64) -> Result<(), Self::Error> {
        self.filesystem
            .begin_upload(name, expected_size)
            .map_err(FilesystemOwnerError::Backend)
    }

    fn upload_write(&mut self, name: &str, offset: u64, data: &[u8]) -> Result<(), Self::Error> {
        self.filesystem
            .write_upload_chunk(name, offset, data)
            .map_err(FilesystemOwnerError::Backend)
    }

    fn upload_commit(&mut self, name: &str) -> Result<(), Self::Error> {
        self.filesystem
            .commit_upload(name)
            .map_err(FilesystemOwnerError::Backend)
    }

    fn upload_abort(&mut self, name: &str) -> Result<(), Self::Error> {
        self.filesystem
            .abort_upload(name)
            .map_err(FilesystemOwnerError::Backend)
    }

    fn delete(&mut self, name: &str) -> Result<(), Self::Error> {
        self.filesystem
            .delete_file(name)
            .map_err(FilesystemOwnerError::Backend)
    }

    fn scan_start(&mut self) -> Result<(), Self::Error> {
        Err(FilesystemOwnerError::IncrementalScanUnavailable)
    }

    fn scan_prefetch(&mut self, _name: &str) -> Result<(), Self::Error> {
        Err(FilesystemOwnerError::IncrementalScanUnavailable)
    }

    fn scan_step(&mut self, _budget: u8, _out: &mut [u8]) -> Result<ScanProgress, Self::Error> {
        Err(FilesystemOwnerError::IncrementalScanUnavailable)
    }

    fn scan_finish(&mut self) -> Result<(), Self::Error> {
        Err(FilesystemOwnerError::IncrementalScanUnavailable)
    }

    fn scan_cancel(&mut self) -> Result<(), Self::Error> {
        Err(FilesystemOwnerError::IncrementalScanUnavailable)
    }
}

#[cfg(feature = "sd-storage")]
#[derive(Debug)]
pub enum MountedStorageOwnerError<D>
where
    D: embedded_sd_storage::BlockDevice,
    D::Error: core::fmt::Debug,
{
    Storage(embedded_sd_storage::sd_filesystem::StorageError<D>),
    UploadSizeMismatch,
    ScanEncoding(RequestError),
    NoScan,
}

#[cfg(feature = "sd-storage")]
pub struct MountedStorageOwnerBackend<'a, D>
where
    D: embedded_sd_storage::BlockDevice,
    D::Error: core::fmt::Debug,
{
    storage: &'a embedded_sd_storage::MountedStorage<D>,
    scan: Option<embedded_sd_storage::MountedDirectoryScan<'a, D>>,
    active_read: Option<(
        StorageName,
        embedded_sd_storage::MountedFileSession<'a, D>,
        Option<usize>,
    )>,
    metadata_dir: Option<embedded_sd_storage::MountedDirectorySession<'a, D>>,
    metadata_reads: Vec<(StorageName, embedded_sd_storage::MountedFileSession<'a, D>), 3>,
    mounted_files: Vec<
        Option<embedded_sd_storage::MountedFileSession<'a, D>>,
        { crate::library::LIBRARY_VIEWPORT_CAPACITY },
    >,
    prefetch_names: Vec<StorageName, { crate::library::LIBRARY_VIEWPORT_CAPACITY }>,
    mount_generation: u8,
    pending_mount_generation: u8,
    mounted_files_ready: bool,
    active_upload: Option<(
        StorageName,
        embedded_sd_storage::MountedWriteSession<'a, D>,
        u64,
    )>,
}

#[cfg(feature = "sd-storage")]
impl<'a, D> MountedStorageOwnerBackend<'a, D>
where
    D: embedded_sd_storage::BlockDevice,
    D::Error: core::fmt::Debug,
{
    pub const fn new(storage: &'a embedded_sd_storage::MountedStorage<D>) -> Self {
        Self {
            storage,
            scan: None,
            active_read: None,
            metadata_dir: None,
            metadata_reads: Vec::new(),
            mounted_files: Vec::new(),
            prefetch_names: Vec::new(),
            mount_generation: 0,
            pending_mount_generation: 0,
            mounted_files_ready: false,
            active_upload: None,
        }
    }

    fn is_metadata(name: &str) -> bool {
        matches!(
            name,
            crate::storage_access::LIBRARY_INDEX_FILE
                | crate::storage_access::STATE_SLOT_A
                | crate::storage_access::STATE_SLOT_B
        )
    }

    fn recycle_active_read(&mut self) {
        let Some((_, file, Some(index))) = self.active_read.take() else {
            self.active_read = None;
            return;
        };
        if let Some(slot) = self.mounted_files.get_mut(index) {
            *slot = Some(file);
        }
    }

    fn read_session(
        &mut self,
        name: &str,
    ) -> Result<&mut embedded_sd_storage::MountedFileSession<'a, D>, MountedStorageOwnerError<D>>
    {
        let dedicated_metadata = matches!(
            name,
            crate::storage_access::LIBRARY_INDEX_FILE
                | crate::storage_access::STATE_SLOT_A
                | crate::storage_access::STATE_SLOT_B
        );
        if dedicated_metadata {
            if let Some(index) = self
                .metadata_reads
                .iter()
                .position(|(active, _)| active.as_str() == name)
            {
                return Ok(&mut self.metadata_reads[index].1);
            }
            if self.metadata_dir.is_none() {
                match self
                    .storage
                    .open_directory(crate::storage_access::METADATA_DIR)
                {
                    Ok(dir) => {
                        self.metadata_dir = Some(dir);
                    }
                    Err(embedded_sd_storage::sd_filesystem::StorageError::NotFound)
                        if name == crate::storage_access::LIBRARY_INDEX_FILE =>
                    {
                        let stored = StorageName::new(name)
                            .map_err(MountedStorageOwnerError::ScanEncoding)?;
                        let file = self
                            .storage
                            .open_file(name)
                            .map_err(MountedStorageOwnerError::Storage)?;
                        self.metadata_reads.push((stored, file)).map_err(|_| {
                            MountedStorageOwnerError::ScanEncoding(RequestError::Busy)
                        })?;
                        return Ok(&mut self.metadata_reads.last_mut().unwrap().1);
                    }
                    Err(error) => return Err(MountedStorageOwnerError::Storage(error)),
                }
            }
            if self.metadata_reads.is_empty() {
                let files = self
                    .metadata_dir
                    .as_ref()
                    .unwrap()
                    .open_matching_files::<3>(&[
                        crate::storage_access::LIBRARY_INDEX_FILE,
                        crate::storage_access::STATE_SLOT_A,
                        crate::storage_access::STATE_SLOT_B,
                    ])
                    .map_err(MountedStorageOwnerError::Storage)?;
                for file in files {
                    self.metadata_reads
                        .push((
                            StorageName::new(file.name.as_str())
                                .map_err(MountedStorageOwnerError::ScanEncoding)?,
                            file.file,
                        ))
                        .map_err(|_| MountedStorageOwnerError::ScanEncoding(RequestError::Busy))?;
                }
            }
            if let Some(index) = self
                .metadata_reads
                .iter()
                .position(|(active, _)| active.as_str() == name)
            {
                return Ok(&mut self.metadata_reads[index].1);
            }
            match self.metadata_dir.as_ref().unwrap().open_file(name) {
                Ok(file) => {
                    let stored =
                        StorageName::new(name).map_err(MountedStorageOwnerError::ScanEncoding)?;
                    self.metadata_reads
                        .push((stored, file))
                        .map_err(|_| MountedStorageOwnerError::ScanEncoding(RequestError::Busy))?;
                    return Ok(&mut self.metadata_reads.last_mut().unwrap().1);
                }
                Err(embedded_sd_storage::sd_filesystem::StorageError::NotFound) => {}
                Err(error) => return Err(MountedStorageOwnerError::Storage(error)),
            }
            if name == crate::storage_access::LIBRARY_INDEX_FILE {
                let stored =
                    StorageName::new(name).map_err(MountedStorageOwnerError::ScanEncoding)?;
                let file = self
                    .storage
                    .open_file(name)
                    .map_err(MountedStorageOwnerError::Storage)?;
                self.metadata_reads
                    .push((stored, file))
                    .map_err(|_| MountedStorageOwnerError::ScanEncoding(RequestError::Busy))?;
                return Ok(&mut self.metadata_reads.last_mut().unwrap().1);
            }
            return Err(MountedStorageOwnerError::Storage(
                embedded_sd_storage::sd_filesystem::StorageError::NotFound,
            ));
        }
        let matches = self
            .active_read
            .as_ref()
            .is_some_and(|(active, _, _)| active.as_str() == name);
        if !matches {
            self.recycle_active_read();
            let stored = StorageName::new(name).map_err(MountedStorageOwnerError::ScanEncoding)?;
            let file = if matches!(
                name,
                crate::storage_access::LEGACY_STATE_SLOT_A
                    | crate::storage_access::LEGACY_STATE_SLOT_B
            ) {
                let underlying = if name == crate::storage_access::LEGACY_STATE_SLOT_A {
                    crate::storage_access::STATE_SLOT_A
                } else {
                    crate::storage_access::STATE_SLOT_B
                };
                self.storage
                    .open_file(underlying)
                    .map_err(MountedStorageOwnerError::Storage)?
            } else {
                self.storage
                    .open_file(name)
                    .map_err(MountedStorageOwnerError::Storage)?
            };
            self.active_read = Some((stored, file, None));
        }
        Ok(&mut self.active_read.as_mut().unwrap().1)
    }
}

#[cfg(feature = "sd-storage")]
impl<D> OwnerBackend for MountedStorageOwnerBackend<'_, D>
where
    D: embedded_sd_storage::BlockDevice,
    D::Error: core::fmt::Debug,
{
    type Error = MountedStorageOwnerError<D>;

    fn activate_file(
        &mut self,
        name: &str,
        token: crate::library::MountFileToken,
    ) -> Result<u64, Self::Error> {
        if !self.mounted_files_ready || token.mount_generation() != self.mount_generation {
            return Err(MountedStorageOwnerError::ScanEncoding(RequestError::Stale));
        }
        let index = token
            .entry_index()
            .ok_or(MountedStorageOwnerError::ScanEncoding(RequestError::Stale))?;
        if self
            .active_read
            .as_ref()
            .is_some_and(|(active, _, origin)| active.as_str() == name && *origin == Some(index))
        {
            return self
                .active_read
                .as_mut()
                .unwrap()
                .1
                .size()
                .map_err(MountedStorageOwnerError::Storage);
        }
        self.recycle_active_read();
        let file = self
            .mounted_files
            .get_mut(index)
            .and_then(Option::take)
            .ok_or(MountedStorageOwnerError::ScanEncoding(RequestError::Stale))?;
        let stored = StorageName::new(name).map_err(MountedStorageOwnerError::ScanEncoding)?;
        self.active_read = Some((stored, file, Some(index)));
        self.active_read
            .as_mut()
            .unwrap()
            .1
            .size()
            .map_err(MountedStorageOwnerError::Storage)
    }

    fn file_size(&mut self, name: &str) -> Result<u64, Self::Error> {
        self.read_session(name)?
            .size()
            .map_err(MountedStorageOwnerError::Storage)
    }

    fn read_at(&mut self, name: &str, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        self.read_session(name)?
            .read_at(offset, out)
            .map_err(MountedStorageOwnerError::Storage)
    }

    fn upload_begin(&mut self, name: &str, expected_size: u64) -> Result<(), Self::Error> {
        if Self::is_metadata(name) {
            self.metadata_reads.clear();
        } else {
            self.active_read = None;
        }
        let session = if Self::is_metadata(name) {
            if self.metadata_dir.is_none() {
                self.metadata_dir = Some(
                    self.storage
                        .open_or_create_directory(crate::storage_access::METADATA_DIR)
                        .map_err(MountedStorageOwnerError::Storage)?,
                );
            }
            self.metadata_dir
                .as_ref()
                .unwrap()
                .create_file_session(name)
        } else {
            self.storage.create_file_session(name)
        }
        .map_err(MountedStorageOwnerError::Storage)?;
        self.active_upload = Some((
            StorageName::new(name).map_err(MountedStorageOwnerError::ScanEncoding)?,
            session,
            expected_size,
        ));
        Ok(())
    }

    fn upload_write(&mut self, name: &str, offset: u64, data: &[u8]) -> Result<(), Self::Error> {
        let (active_name, session, _) = self
            .active_upload
            .as_mut()
            .ok_or(MountedStorageOwnerError::UploadSizeMismatch)?;
        if active_name.as_str() != name {
            return Err(MountedStorageOwnerError::UploadSizeMismatch);
        }
        session
            .write_at(offset, data)
            .map_err(MountedStorageOwnerError::Storage)
    }

    fn upload_commit(&mut self, name: &str) -> Result<(), Self::Error> {
        let (active_name, mut session, expected) = self
            .active_upload
            .take()
            .ok_or(MountedStorageOwnerError::UploadSizeMismatch)?;
        if active_name.as_str() != name {
            return Err(MountedStorageOwnerError::UploadSizeMismatch);
        }
        session.flush().map_err(MountedStorageOwnerError::Storage)?;
        let actual = session.size().map_err(MountedStorageOwnerError::Storage)?;
        if expected != actual {
            return Err(MountedStorageOwnerError::UploadSizeMismatch);
        }
        if Self::is_metadata(active_name.as_str()) {
            self.metadata_reads.clear();
            self.metadata_reads
                .push((active_name, session.into_read_session()))
                .map_err(|_| MountedStorageOwnerError::ScanEncoding(RequestError::Busy))?;
        }
        Ok(())
    }

    fn upload_abort(&mut self, name: &str) -> Result<(), Self::Error> {
        if Self::is_metadata(name) {
            self.metadata_reads.clear();
        } else {
            self.active_read = None;
        }
        self.active_upload = None;
        if Self::is_metadata(name) {
            self.storage
                .delete_file_in_dir(crate::storage_access::METADATA_DIR, name)
        } else {
            self.storage.delete_file(name)
        }
        .map_err(MountedStorageOwnerError::Storage)
    }

    fn delete(&mut self, name: &str) -> Result<(), Self::Error> {
        if Self::is_metadata(name) {
            self.metadata_reads.clear();
        } else {
            self.active_read = None;
        }
        if Self::is_metadata(name) {
            self.storage
                .delete_file_in_dir(crate::storage_access::METADATA_DIR, name)
        } else {
            self.storage.delete_file(name)
        }
        .map_err(MountedStorageOwnerError::Storage)
    }

    fn scan_start(&mut self) -> Result<(), Self::Error> {
        self.active_read = None;
        self.mounted_files.clear();
        self.prefetch_names.clear();
        self.mounted_files_ready = false;
        self.pending_mount_generation = self.mount_generation.wrapping_add(1).max(1);
        self.scan = Some(
            self.storage
                .start_scan()
                .map_err(MountedStorageOwnerError::Storage)?,
        );
        Ok(())
    }

    fn scan_prefetch(&mut self, name: &str) -> Result<(), Self::Error> {
        if self.scan.is_none()
            || self
                .prefetch_names
                .iter()
                .any(|value| value.as_str() == name)
        {
            return Err(MountedStorageOwnerError::ScanEncoding(RequestError::Stale));
        }
        self.prefetch_names
            .push(StorageName::new(name).map_err(MountedStorageOwnerError::ScanEncoding)?)
            .map_err(|_| MountedStorageOwnerError::ScanEncoding(RequestError::TooLarge))?;
        self.mounted_files
            .push(None)
            .map_err(|_| MountedStorageOwnerError::ScanEncoding(RequestError::TooLarge))
    }

    fn scan_step(&mut self, budget: u8, out: &mut [u8]) -> Result<ScanProgress, Self::Error> {
        let batch = self
            .scan
            .as_mut()
            .ok_or(MountedStorageOwnerError::NoScan)?
            .step_with_files(budget)
            .map_err(MountedStorageOwnerError::Storage)?;
        let mut records = Vec::<ScanRecord, { SCAN_ENTRY_BUDGET as usize }>::new();
        for entry in batch.entries {
            let prefetch_index = self
                .prefetch_names
                .iter()
                .position(|name| name.as_str() == entry.name.as_str());
            let mount_token = prefetch_index.and_then(|index| {
                self.mounted_files[index] = Some(entry.file);
                crate::library::MountFileToken::for_mount_entry(
                    self.pending_mount_generation,
                    index as u8,
                )
            });
            records
                .push(ScanRecord {
                    name: StorageName::new(entry.name.as_str())
                        .map_err(MountedStorageOwnerError::ScanEncoding)?,
                    size: entry.size,
                    mount_token,
                })
                .map_err(|_| MountedStorageOwnerError::ScanEncoding(RequestError::TooLarge))?;
        }
        encode_scan_records(&records, out).map_err(MountedStorageOwnerError::ScanEncoding)?;
        Ok(ScanProgress {
            entries: records.len() as u8,
            complete: batch.complete,
        })
    }

    fn scan_finish(&mut self) -> Result<(), Self::Error> {
        self.scan.take().ok_or(MountedStorageOwnerError::NoScan)?;
        self.mount_generation = self.pending_mount_generation;
        self.mounted_files_ready = true;
        Ok(())
    }

    fn scan_cancel(&mut self) -> Result<(), Self::Error> {
        self.scan.take().ok_or(MountedStorageOwnerError::NoScan)?;
        self.mounted_files.clear();
        self.mounted_files_ready = false;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    Ack,
    FileSize(u64),
    Read(u16),
    Written(u16),
    UploadStarted(u32),
    ScanStarted(ScanToken),
    ScanProgress(ScanProgress),
}

#[derive(Debug, PartialEq, Eq)]
pub enum OwnerError<E> {
    Request(RequestError),
    Backend(E),
    BufferUnavailable,
}

pub struct StorageOwner<B> {
    backend: B,
    protocol: StorageOwnerProtocol,
    upload_name: Option<StorageName>,
    upload_client: Option<StorageClient>,
}

impl<B: OwnerBackend> StorageOwner<B> {
    pub const fn new(backend: B) -> Self {
        Self {
            backend,
            protocol: StorageOwnerProtocol::new(),
            upload_name: None,
            upload_client: None,
        }
    }

    pub fn execute<P: BufferPool>(
        &mut self,
        request: Request,
        buffers: &mut P,
    ) -> Result<Response, OwnerError<B::Error>> {
        match request {
            Request::ActivateFile { name, token } => self
                .backend
                .activate_file(name.as_str(), token)
                .map(Response::FileSize)
                .map_err(OwnerError::Backend),
            Request::FileSize { name } => self
                .backend
                .file_size(name.as_str())
                .map(Response::FileSize)
                .map_err(OwnerError::Backend),
            Request::Read {
                name,
                offset,
                buffer_offset,
                len,
                buffer,
            } => {
                validate_len(len).map_err(OwnerError::Request)?;
                let out = buffer_range(buffers, buffer, buffer_offset, len)?;
                self.backend
                    .read_at(name.as_str(), offset, out)
                    .map_err(OwnerError::Backend)?;
                Ok(Response::Read(len))
            }
            Request::UploadBegin {
                name,
                expected_size,
                ..
            } => {
                if self.protocol.upload_id().is_some() {
                    return Err(OwnerError::Request(RequestError::Busy));
                }
                self.backend
                    .upload_begin(name.as_str(), expected_size)
                    .map_err(OwnerError::Backend)?;
                let id = self.protocol.begin_upload().map_err(OwnerError::Request)?;
                self.upload_name = Some(name);
                Ok(Response::UploadStarted(id))
            }
            Request::UploadWrite {
                upload_id,
                offset,
                buffer_offset,
                len,
                buffer,
                ..
            } => {
                self.protocol
                    .validate_upload(upload_id)
                    .map_err(OwnerError::Request)?;
                validate_len(len).map_err(OwnerError::Request)?;
                let data = buffer_range(buffers, buffer, buffer_offset, len)?;
                let name = self
                    .upload_name
                    .as_ref()
                    .ok_or(OwnerError::Request(RequestError::Stale))?;
                self.backend
                    .upload_write(name.as_str(), offset, data)
                    .map_err(OwnerError::Backend)?;
                Ok(Response::Written(len))
            }
            Request::UploadCommit { upload_id, .. } => {
                self.protocol
                    .validate_upload(upload_id)
                    .map_err(OwnerError::Request)?;
                let name = self
                    .upload_name
                    .as_ref()
                    .ok_or(OwnerError::Request(RequestError::Stale))?;
                self.backend
                    .upload_commit(name.as_str())
                    .map_err(OwnerError::Backend)?;
                self.protocol
                    .commit_upload(upload_id)
                    .map_err(OwnerError::Request)?;
                self.upload_name = None;
                Ok(Response::Ack)
            }
            Request::UploadAbort { upload_id } => {
                self.protocol
                    .validate_upload(upload_id)
                    .map_err(OwnerError::Request)?;
                let name = self
                    .upload_name
                    .as_ref()
                    .ok_or(OwnerError::Request(RequestError::Stale))?;
                self.backend
                    .upload_abort(name.as_str())
                    .map_err(OwnerError::Backend)?;
                self.protocol
                    .abort_upload(upload_id)
                    .map_err(OwnerError::Request)?;
                self.upload_name = None;
                Ok(Response::Ack)
            }
            Request::Delete { name } => {
                self.backend
                    .delete(name.as_str())
                    .map_err(OwnerError::Backend)?;
                Ok(Response::Ack)
            }
            Request::ScanStart => {
                if self.protocol.scan_token().is_some() {
                    return Err(OwnerError::Request(RequestError::Busy));
                }
                self.backend.scan_start().map_err(OwnerError::Backend)?;
                let token = self.protocol.start_scan().map_err(OwnerError::Request)?;
                Ok(Response::ScanStarted(token))
            }
            Request::ScanPrefetch { token, name } => {
                self.protocol
                    .validate_scan(token)
                    .map_err(OwnerError::Request)?;
                self.backend
                    .scan_prefetch(name.as_str())
                    .map_err(OwnerError::Backend)?;
                Ok(Response::Ack)
            }
            Request::ScanStep { token, buffer } => {
                self.protocol
                    .validate_scan(token)
                    .map_err(OwnerError::Request)?;
                let out = buffer_range(buffers, buffer, 0, SCAN_BATCH_BYTES as u16)?;
                out.fill(0);
                self.backend
                    .scan_step(SCAN_ENTRY_BUDGET, out)
                    .map(Response::ScanProgress)
                    .map_err(OwnerError::Backend)
            }
            Request::ScanFinish { token } => {
                self.protocol
                    .validate_scan(token)
                    .map_err(OwnerError::Request)?;
                self.backend.scan_finish().map_err(OwnerError::Backend)?;
                self.protocol
                    .finish_scan(token)
                    .map_err(OwnerError::Request)?;
                Ok(Response::Ack)
            }
            Request::ScanCancel { token } => {
                self.protocol
                    .validate_scan(token)
                    .map_err(OwnerError::Request)?;
                self.backend.scan_cancel().map_err(OwnerError::Backend)?;
                self.protocol
                    .cancel_scan(token)
                    .map_err(OwnerError::Request)?;
                Ok(Response::Ack)
            }
        }
    }

    pub fn execute_owned<'a>(&mut self, request: OwnedRequest<'a>) -> OwnedResponse<'a> {
        let client = request.envelope.client;
        let sequence = request.envelope.sequence;
        let begins_upload = matches!(request.envelope.request, Request::UploadBegin { .. });
        let ends_upload = matches!(
            request.envelope.request,
            Request::UploadCommit { .. } | Request::UploadAbort { .. }
        );
        let mut pool = LeasePool {
            lease: request.buffer,
        };
        let response =
            self.execute(request.envelope.request, &mut pool)
                .map_err(|error| match error {
                    OwnerError::Request(error) => CompletionError::Request(error),
                    OwnerError::Backend(_) => CompletionError::Backend,
                    OwnerError::BufferUnavailable => CompletionError::BufferUnavailable,
                });
        if response.is_ok() {
            if begins_upload {
                self.upload_client = Some(client);
            } else if ends_upload {
                self.upload_client = None;
            }
        }
        OwnedResponse {
            envelope: ResponseEnvelope {
                client,
                sequence,
                response,
            },
            buffer: pool.lease,
        }
    }

    pub const fn active_upload_client(&self) -> Option<StorageClient> {
        self.upload_client
    }
}

fn buffer_range<P: BufferPool, E>(
    buffers: &mut P,
    slot: BufferSlot,
    offset: u16,
    len: u16,
) -> Result<&mut [u8], OwnerError<E>> {
    let buffer = buffers
        .buffer_mut(slot)
        .ok_or(OwnerError::BufferUnavailable)?;
    let start = usize::from(offset);
    let end = start
        .checked_add(usize::from(len))
        .ok_or(OwnerError::BufferUnavailable)?;
    buffer
        .get_mut(start..end)
        .ok_or(OwnerError::BufferUnavailable)
}

const fn validate_len(len: u16) -> Result<(), RequestError> {
    if len == 0 {
        Err(RequestError::Empty)
    } else if len > MAX_BOUNDED_IO_BYTES {
        Err(RequestError::TooLarge)
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestError {
    NameTooLong,
    Empty,
    TooLarge,
    Busy,
    Stale,
}

#[derive(Debug, Default)]
pub struct StorageOwnerProtocol {
    scan: Option<ScanToken>,
    next_scan: u32,
    upload: Option<u32>,
    next_upload: u32,
}

impl StorageOwnerProtocol {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            scan: None,
            next_scan: 1,
            upload: None,
            next_upload: 1,
        }
    }

    pub fn start_scan(&mut self) -> Result<ScanToken, RequestError> {
        if self.scan.is_some() {
            return Err(RequestError::Busy);
        }
        let token = ScanToken(self.next_scan);
        self.next_scan = self.next_scan.wrapping_add(1).max(1);
        self.scan = Some(token);
        Ok(token)
    }

    #[must_use]
    pub const fn scan_token(&self) -> Option<ScanToken> {
        self.scan
    }

    pub fn finish_scan(&mut self, token: ScanToken) -> Result<(), RequestError> {
        self.require_scan(token)?;
        self.scan = None;
        Ok(())
    }

    pub fn cancel_scan(&mut self, token: ScanToken) -> Result<(), RequestError> {
        self.finish_scan(token)
    }

    fn require_scan(&self, token: ScanToken) -> Result<(), RequestError> {
        if self.scan == Some(token) {
            Ok(())
        } else {
            Err(RequestError::Stale)
        }
    }

    pub fn validate_scan(&self, token: ScanToken) -> Result<(), RequestError> {
        self.require_scan(token)
    }

    pub fn begin_upload(&mut self) -> Result<u32, RequestError> {
        if self.upload.is_some() {
            return Err(RequestError::Busy);
        }
        let id = self.next_upload;
        self.next_upload = self.next_upload.wrapping_add(1).max(1);
        self.upload = Some(id);
        Ok(id)
    }

    #[must_use]
    pub const fn upload_id(&self) -> Option<u32> {
        self.upload
    }

    pub fn validate_upload(&self, id: u32) -> Result<(), RequestError> {
        self.require_upload(id)
    }

    pub fn commit_upload(&mut self, id: u32) -> Result<(), RequestError> {
        self.require_upload(id)?;
        self.upload = None;
        Ok(())
    }

    pub fn abort_upload(&mut self, id: u32) -> Result<(), RequestError> {
        self.commit_upload(id)
    }

    fn require_upload(&self, id: u32) -> Result<(), RequestError> {
        if self.upload == Some(id) {
            Ok(())
        } else {
            Err(RequestError::Stale)
        }
    }
}
