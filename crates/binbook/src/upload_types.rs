use std::path::PathBuf;
use std::time::{Duration, Instant};

use binbook_diagnostic_protocol::{Opcode, ProtocolError, Status};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadRequest<'a> {
    pub destination: &'a str,
    pub skip_library_wait: bool,
}

impl<'a> UploadRequest<'a> {
    pub fn new(destination: &'a str, skip_library_wait: bool) -> Self {
        Self {
            destination,
            skip_library_wait,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadSummary {
    pub bytes: u64,
    pub crc32: u32,
    pub elapsed_ms: u64,
    pub bytes_per_second: u64,
    pub library_visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    Preparing {
        total: u64,
    },
    Transfer {
        acknowledged: u64,
        total: u64,
        elapsed_ms: u64,
    },
    Verifying,
    Publishing,
    UpdatingLibrary,
    Complete(UploadSummary),
}

pub trait ProgressSink {
    fn report(&mut self, event: ProgressEvent);
}

pub trait Clock {
    fn elapsed(&self) -> Duration;
}

pub struct SystemClock(Instant);

impl SystemClock {
    pub fn start() -> Self {
        Self(Instant::now())
    }
}

impl Clock for SystemClock {
    fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }
}

pub trait UploadTransport {
    fn upload_window(&mut self) -> Result<usize, UploadError> {
        Ok(1)
    }

    fn exchange(
        &mut self,
        request: &[u8],
        opcode: Opcode,
        sequence: u16,
    ) -> Result<Vec<u8>, UploadError>;

    fn exchange_batch(&mut self, requests: &[UploadPacket]) -> Result<Vec<Vec<u8>>, UploadError> {
        requests
            .iter()
            .map(|request| self.exchange(&request.frame, request.opcode, request.sequence))
            .collect()
    }

    fn wait_library_poll(&mut self) {}
}

#[derive(Debug)]
pub struct UploadPacket {
    pub frame: Vec<u8>,
    pub opcode: Opcode,
    pub sequence: u16,
    pub sent_len: usize,
}

#[derive(Debug, Error)]
pub enum UploadError {
    #[error("cannot open source {}: {source}", path.display())]
    SourceOpen {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("destination name is not valid UTF-8; pass --name explicitly")]
    DestinationName,
    #[error("progress output failed: {0}")]
    Progress(std::io::Error),
    #[error("local I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol codec failed: {0:?}")]
    Codec(ProtocolError),
    #[error("transport failed: {message}; cleanup confirmed: {cleanup_confirmed}")]
    Transport {
        message: String,
        cleanup_confirmed: bool,
    },
    #[error("unexpected response: {0}")]
    Response(String),
    #[error(
        "device returned {status:?} for {opcode:?} sequence {sequence}; cleanup confirmed: {cleanup_confirmed}"
    )]
    DeviceStatus {
        opcode: Opcode,
        sequence: u16,
        status: Status,
        cleanup_confirmed: bool,
    },
    #[error("device accepted {actual} bytes, expected {expected}; cleanup confirmed: {cleanup_confirmed}")]
    AcceptedLength {
        expected: usize,
        actual: usize,
        cleanup_confirmed: bool,
    },
    #[error("device CRC mismatch: expected 0x{expected:08X}, computed 0x{actual:08X}; cleanup confirmed: {cleanup_confirmed}")]
    Crc {
        expected: u32,
        actual: u32,
        cleanup_confirmed: bool,
    },
    #[error("file is too large for protocol v2: {0} bytes")]
    TooLarge(u64),
    #[error("upload committed, but library update failed: {0}")]
    LibraryWait(String),
}

impl From<ProtocolError> for UploadError {
    fn from(value: ProtocolError) -> Self {
        Self::Codec(value)
    }
}
