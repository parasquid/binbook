#![cfg(feature = "serial-device")]

use std::io::Cursor;
use std::time::Duration;

use binbook::bulk_upload::{bulk_upload_stream, BulkUploadTransport, CreditPollError};
use binbook::upload::{Clock, ProgressEvent, ProgressSink, UploadError, UploadRequest};
use binbook_diagnostic_protocol::{
    Status, StoreBulkCredit, StoreBulkData, StoreBulkFinishResponse, StoreBulkOpenRequest,
    StoreBulkOpenResponse, StoreBulkState, StoreBulkStatusResponse, StoreUploadCommitResult,
    TransferDirection,
};

#[derive(Default)]
struct FakeTransport {
    sent: Vec<(u32, Vec<u8>, bool)>,
    credits: Vec<Result<StoreBulkCredit, CreditPollError>>,
    status_offset: u32,
    aborted: Vec<u32>,
    finish_crc: u32,
}

impl BulkUploadTransport for FakeTransport {
    fn open(
        &mut self,
        request: StoreBulkOpenRequest<'_>,
    ) -> Result<StoreBulkOpenResponse, UploadError> {
        Ok(StoreBulkOpenResponse {
            stream_id: 7,
            selected_chunk: 512,
            initial_credits: 2,
            direction: request.direction,
            durable_offset: 0,
            total_size: request.total_size,
        })
    }

    fn send_data(&mut self, frame: StoreBulkData<'_>) -> Result<(), UploadError> {
        self.sent
            .push((frame.offset, frame.data.to_vec(), frame.end_of_data));
        Ok(())
    }

    fn poll_credit(&mut self) -> Result<StoreBulkCredit, CreditPollError> {
        self.credits.remove(0)
    }

    fn status(&mut self, stream_id: u32) -> Result<StoreBulkStatusResponse, UploadError> {
        Ok(StoreBulkStatusResponse {
            stream_id,
            direction: TransferDirection::Upload,
            state: StoreBulkState::Receiving,
            available_credits: 2,
            durable_offset: self.status_offset,
            total_size: 1_024,
            running_crc32: 1,
        })
    }

    fn finish(&mut self, stream_id: u32) -> Result<StoreBulkFinishResponse, UploadError> {
        Ok(StoreBulkFinishResponse {
            stream_id,
            result: StoreUploadCommitResult::Ok,
            total_bytes: 1_024,
            computed_crc32: self.finish_crc,
        })
    }

    fn abort(&mut self, stream_id: u32) -> bool {
        self.aborted.push(stream_id);
        true
    }

    fn wait_library(&mut self, _name: &str, _size: u64) -> Result<bool, UploadError> {
        Ok(true)
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

fn credit(offset: u32, credits: u8) -> Result<StoreBulkCredit, CreditPollError> {
    Ok(StoreBulkCredit {
        stream_id: 7,
        durable_offset: offset,
        credits,
        status: Status::Ok,
    })
}

#[test]
fn bulk_upload_progress_tracks_only_device_durable_offsets() {
    let bytes = vec![0x5a; 1_024];
    let crc = crc32(&bytes);
    let mut transport = FakeTransport {
        credits: vec![credit(1_024, 2)],
        finish_crc: crc,
        ..FakeTransport::default()
    };
    let mut progress = Progress::default();
    let summary = bulk_upload_stream(
        &mut Cursor::new(bytes),
        UploadRequest::new("book.binbook", false),
        2,
        &mut transport,
        &mut progress,
        &FakeClock,
    )
    .unwrap();

    assert_eq!(
        transport.sent.iter().map(|item| item.0).collect::<Vec<_>>(),
        [0, 512]
    );
    assert!(transport.sent.last().unwrap().2);
    assert_eq!(summary.bytes, 1_024);
    let durable = progress
        .0
        .iter()
        .filter_map(|event| match event {
            ProgressEvent::Transfer { acknowledged, .. } => Some(*acknowledged),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(durable, [1_024]);
}

#[test]
fn timeout_uses_status_barrier_and_resends_only_uncertain_suffix() {
    let bytes = vec![0x33; 1_024];
    let crc = crc32(&bytes);
    let mut transport = FakeTransport {
        credits: vec![Err(CreditPollError::Timeout), credit(1_024, 1)],
        status_offset: 512,
        finish_crc: crc,
        ..FakeTransport::default()
    };
    bulk_upload_stream(
        &mut Cursor::new(bytes),
        UploadRequest::new("book.binbook", false),
        2,
        &mut transport,
        &mut Progress::default(),
        &FakeClock,
    )
    .unwrap();

    assert_eq!(
        transport.sent.iter().map(|item| item.0).collect::<Vec<_>>(),
        [0, 512, 512]
    );
}

#[test]
fn invalid_credit_aborts_and_never_reports_false_progress() {
    let bytes = vec![0x44; 1_024];
    let mut transport = FakeTransport {
        credits: vec![credit(513, 1)],
        ..FakeTransport::default()
    };
    let mut progress = Progress::default();
    let error = bulk_upload_stream(
        &mut Cursor::new(bytes),
        UploadRequest::new("book.binbook", false),
        2,
        &mut transport,
        &mut progress,
        &FakeClock,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        UploadError::Transport {
            cleanup_confirmed: true,
            ..
        }
    ));
    assert_eq!(transport.aborted, [7]);
    assert!(!progress
        .0
        .iter()
        .any(|event| matches!(event, ProgressEvent::Transfer { .. })));
}
