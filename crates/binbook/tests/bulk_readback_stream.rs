#![cfg(feature = "serial-device")]

use std::io::{Cursor, Seek, SeekFrom, Write};
use std::time::Duration;

use binbook::bulk_readback_stream::{
    bulk_readback_stream, BulkReadbackOutput, BulkReadbackTransport, DataPollError, OwnedBulkData,
};
use binbook::storage_readback::sd_storage_name;
use binbook::storage_readback::ReadbackError;
use binbook::upload::{Clock, ProgressEvent, ProgressSink};
use binbook_diagnostic_protocol::{
    StoreBulkCredit, StoreBulkFinishResponse, StoreBulkOpenRequest, StoreBulkOpenResponse,
    StoreBulkState, StoreBulkStatusResponse, StoreUploadCommitResult, TransferDirection,
};

struct Output(Cursor<Vec<u8>>);

impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl Seek for Output {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.0.seek(position)
    }
}

impl BulkReadbackOutput for Output {
    fn truncate(&mut self, len: u64) -> std::io::Result<()> {
        self.0.get_mut().truncate(len as usize);
        Ok(())
    }
}

struct FakeReadback {
    events: Vec<Result<OwnedBulkData, DataPollError>>,
    credits: Vec<StoreBulkCredit>,
    status_offset: u32,
    status_crc32: u32,
    data: Vec<u8>,
    crc32: u32,
    aborted: bool,
}

impl BulkReadbackTransport for FakeReadback {
    fn open(
        &mut self,
        request: StoreBulkOpenRequest<'_>,
    ) -> Result<StoreBulkOpenResponse, ReadbackError> {
        Ok(StoreBulkOpenResponse {
            stream_id: 9,
            selected_chunk: 512,
            initial_credits: 2,
            direction: request.direction,
            durable_offset: 0,
            total_size: self.data.len() as u32,
        })
    }

    fn poll_data(&mut self) -> Result<OwnedBulkData, DataPollError> {
        self.events.remove(0)
    }

    fn send_credit(&mut self, credit: StoreBulkCredit) -> Result<(), ReadbackError> {
        self.credits.push(credit);
        Ok(())
    }

    fn status(&mut self, stream_id: u32) -> Result<StoreBulkStatusResponse, ReadbackError> {
        Ok(StoreBulkStatusResponse {
            stream_id,
            direction: TransferDirection::Readback,
            state: StoreBulkState::Receiving,
            available_credits: 2,
            durable_offset: self.status_offset,
            total_size: self.data.len() as u32,
            running_crc32: self.status_crc32,
        })
    }

    fn finish(&mut self, stream_id: u32) -> Result<StoreBulkFinishResponse, ReadbackError> {
        Ok(StoreBulkFinishResponse {
            stream_id,
            result: StoreUploadCommitResult::Ok,
            total_bytes: self.data.len() as u32,
            computed_crc32: self.crc32,
        })
    }

    fn abort(&mut self, _stream_id: u32) -> bool {
        self.aborted = true;
        true
    }
}

#[derive(Default)]
struct Progress(Vec<ProgressEvent>);

impl ProgressSink for Progress {
    fn report(&mut self, event: ProgressEvent) {
        self.0.push(event);
    }
}

struct FakeClock;
impl Clock for FakeClock {
    fn elapsed(&self) -> Duration {
        Duration::from_secs(2)
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ 0xedb8_8320
            };
        }
    }
    !crc
}

#[test]
fn sd_storage_name_converts_only_the_legacy_books_path_shape() {
    assert_eq!(
        sd_storage_name("/BOOKS/book.binbook").unwrap(),
        "book.binbook"
    );
    assert_eq!(
        sd_storage_name("BOOKS/book.binbook").unwrap(),
        "book.binbook"
    );
    assert_eq!(sd_storage_name("book.binbook").unwrap(), "book.binbook");
    assert!(sd_storage_name("/TMP/book.binbook").is_err());
    assert!(sd_storage_name("/BOOKS/nested/book.binbook").is_err());
    assert!(sd_storage_name("/BOOKS/").is_err());
}

#[test]
fn readback_credits_only_successfully_written_bytes_and_validates_finish_crc() {
    let data = vec![0x5a; 1_024];
    let mut transport = FakeReadback {
        events: vec![
            Ok(OwnedBulkData {
                stream_id: 9,
                offset: 0,
                data: data[..512].to_vec(),
                end_of_data: false,
            }),
            Ok(OwnedBulkData {
                stream_id: 9,
                offset: 512,
                data: data[512..].to_vec(),
                end_of_data: true,
            }),
        ],
        credits: Vec::new(),
        status_offset: 0,
        status_crc32: 0,
        crc32: crc32(&data),
        aborted: false,
        data: data.clone(),
    };
    let mut output = Output(Cursor::new(Vec::new()));
    let summary = bulk_readback_stream(
        "book.binbook",
        2,
        &mut transport,
        &mut output,
        &mut Progress::default(),
        &FakeClock,
    )
    .unwrap();
    assert_eq!(output.0.into_inner(), data);
    assert_eq!(
        transport
            .credits
            .iter()
            .map(|credit| credit.durable_offset)
            .collect::<Vec<_>>(),
        [512, 1_024]
    );
    assert_eq!(summary.crc32, transport.crc32);
}

#[test]
fn readback_timeout_barrier_flushes_truncates_and_resumes_exactly() {
    let data = vec![0x33; 1_024];
    let mut transport = FakeReadback {
        events: vec![
            Ok(OwnedBulkData {
                stream_id: 9,
                offset: 0,
                data: data[..512].to_vec(),
                end_of_data: false,
            }),
            Err(DataPollError::Timeout),
            Ok(OwnedBulkData {
                stream_id: 9,
                offset: 512,
                data: data[512..].to_vec(),
                end_of_data: true,
            }),
        ],
        credits: Vec::new(),
        status_offset: 512,
        status_crc32: crc32(&data[..512]),
        crc32: crc32(&data),
        aborted: false,
        data: data.clone(),
    };
    let mut output = Output(Cursor::new(Vec::new()));
    bulk_readback_stream(
        "book.binbook",
        2,
        &mut transport,
        &mut output,
        &mut Progress::default(),
        &FakeClock,
    )
    .unwrap();
    assert_eq!(output.0.into_inner(), data);
}

#[test]
fn readback_bad_offset_aborts_and_does_not_credit_or_keep_ambiguous_bytes() {
    let data = vec![0x44; 512];
    let mut transport = FakeReadback {
        events: vec![Ok(OwnedBulkData {
            stream_id: 9,
            offset: 1,
            data: data.clone(),
            end_of_data: true,
        })],
        credits: Vec::new(),
        status_offset: 0,
        status_crc32: 0,
        crc32: crc32(&data),
        aborted: false,
        data,
    };
    let mut output = Output(Cursor::new(Vec::new()));
    assert!(bulk_readback_stream(
        "book.binbook",
        1,
        &mut transport,
        &mut output,
        &mut Progress::default(),
        &FakeClock
    )
    .is_err());
    assert!(transport.aborted);
    assert!(transport.credits.is_empty());
    assert!(output.0.into_inner().is_empty());
}
