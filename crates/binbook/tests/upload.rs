#![cfg(feature = "serial-device")]

use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::time::Duration;

use binbook::upload::{
    upload_stream, Clock, ProgressEvent, ProgressSink, UploadError, UploadPacket, UploadRequest,
    UploadTransport, UPLOAD_CHUNK_BYTES,
};
use binbook::{progress::TextProgress, ProgressMode};
use binbook_diagnostic_protocol::{
    decode_frame, decode_store_upload_write_request, encode_frame, encode_store_entry,
    encode_store_upload_begin_response, encode_store_upload_commit_response,
    encode_store_upload_write_response, FrameHeader, FrameKind, Opcode, Status,
    StoreUploadCommitResult, MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES,
};

struct BoundedReader {
    inner: Cursor<Vec<u8>>,
    largest_read: usize,
}

struct FailingTransferReader {
    inner: Cursor<Vec<u8>>,
    pass: usize,
    transferred: usize,
}

impl Read for FailingTransferReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.pass >= 2 && self.transferred >= UPLOAD_CHUNK_BYTES * 2 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "injected source failure",
            ));
        }
        let count = self.inner.read(buffer)?;
        if self.pass >= 2 {
            self.transferred += count;
        }
        Ok(count)
    }
}

impl Seek for FailingTransferReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        if position == SeekFrom::Start(0) {
            self.pass += 1;
        }
        self.inner.seek(position)
    }
}

impl Read for BoundedReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.largest_read = self.largest_read.max(buffer.len());
        self.inner.read(buffer)
    }
}

impl Seek for BoundedReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

#[derive(Default)]
struct FakeProgress(Vec<ProgressEvent>);

impl ProgressSink for FakeProgress {
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

struct FakePeer {
    writes: Vec<(u16, u32, Vec<u8>)>,
    sequences: Vec<u16>,
    aborts: usize,
    short_write_index: Option<usize>,
    list_polls: usize,
    visible_after_poll: Option<usize>,
    list_error_until_poll: Option<usize>,
    fault: Option<ResponseFault>,
}

#[derive(Clone, Copy)]
enum ResponseFault {
    WrongSequence,
    WrongOpcode,
    NonOk,
    CommitCrc,
}

impl FakePeer {
    fn response(opcode: Opcode, sequence: u16, status: Status, payload: &[u8]) -> Vec<u8> {
        let header = FrameHeader {
            kind: FrameKind::Response,
            opcode,
            status,
            sequence,
            payload_len: payload.len() as u16,
        };
        let mut frame = [0u8; MAX_FRAME_BYTES];
        let len = encode_frame(&header, payload, &mut frame).unwrap();
        frame[..len].to_vec()
    }
}

impl UploadTransport for FakePeer {
    fn exchange(
        &mut self,
        request: &[u8],
        opcode: Opcode,
        sequence: u16,
    ) -> Result<Vec<u8>, UploadError> {
        self.sequences.push(sequence);
        let mut payload = [0u8; MAX_PAYLOAD_BYTES];
        let (_, payload_len) = decode_frame(request, &mut payload).unwrap();
        let mut response_payload = [0u8; MAX_PAYLOAD_BYTES];
        let response_len = match opcode {
            Opcode::StoreUploadBegin => {
                encode_store_upload_begin_response(7, &mut response_payload).unwrap()
            }
            Opcode::StoreUploadWrite => {
                let decoded = decode_store_upload_write_request(&payload[..payload_len]).unwrap();
                let write_index = self.writes.len();
                self.writes
                    .push((sequence, decoded.offset, decoded.data.to_vec()));
                let accepted = if self.short_write_index == Some(write_index) {
                    decoded.data.len() - 1
                } else {
                    decoded.data.len()
                };
                encode_store_upload_write_response(accepted as u32, &mut response_payload).unwrap()
            }
            Opcode::StoreUploadCommit => encode_store_upload_commit_response(
                if matches!(self.fault, Some(ResponseFault::CommitCrc)) {
                    StoreUploadCommitResult::CrcMismatch
                } else {
                    StoreUploadCommitResult::Ok
                },
                if matches!(self.fault, Some(ResponseFault::CommitCrc)) {
                    0xDEAD_BEEF
                } else {
                    0xDB8A_CB75
                },
                &mut response_payload,
            )
            .unwrap(),
            Opcode::StoreAbort => {
                self.aborts += 1;
                0
            }
            Opcode::StoreList => {
                self.list_polls += 1;
                if self
                    .visible_after_poll
                    .is_some_and(|poll| self.list_polls >= poll)
                {
                    response_payload[..2].copy_from_slice(&1u16.to_le_bytes());
                    2 + encode_store_entry(b"fixture.binbook", 8_000, 0, &mut response_payload[2..])
                        .unwrap()
                } else {
                    response_payload[..2].copy_from_slice(&0u16.to_le_bytes());
                    2
                }
            }
            _ => unreachable!(),
        };
        let faulted_write = opcode == Opcode::StoreUploadWrite && self.writes.len() == 2;
        let response_opcode =
            if faulted_write && matches!(self.fault, Some(ResponseFault::WrongOpcode)) {
                Opcode::Status
            } else {
                opcode
            };
        let response_sequence =
            if faulted_write && matches!(self.fault, Some(ResponseFault::WrongSequence)) {
                sequence.wrapping_add(1)
            } else {
                sequence
            };
        let transient_list_error = opcode == Opcode::StoreList
            && self
                .list_error_until_poll
                .is_some_and(|poll| self.list_polls <= poll);
        let response_status = if transient_list_error {
            Status::Error
        } else if faulted_write && matches!(self.fault, Some(ResponseFault::NonOk)) {
            Status::InternalError
        } else {
            Status::Ok
        };
        Ok(Self::response(
            response_opcode,
            response_sequence,
            response_status,
            &response_payload[..response_len],
        ))
    }
}

fn source() -> BoundedReader {
    BoundedReader {
        inner: Cursor::new((0..8_000).map(|value| value as u8).collect()),
        largest_read: 0,
    }
}

struct WindowPeer {
    inner: FakePeer,
    window: usize,
    batch_sizes: Vec<usize>,
    fail_first_in_batch: bool,
}

impl UploadTransport for WindowPeer {
    fn upload_window(&mut self) -> Result<usize, UploadError> {
        Ok(self.window)
    }

    fn exchange(
        &mut self,
        request: &[u8],
        opcode: Opcode,
        sequence: u16,
    ) -> Result<Vec<u8>, UploadError> {
        self.inner.exchange(request, opcode, sequence)
    }

    fn exchange_batch(&mut self, requests: &[UploadPacket]) -> Result<Vec<Vec<u8>>, UploadError> {
        self.batch_sizes.push(requests.len());
        let mut responses = requests
            .iter()
            .map(|request| {
                self.inner
                    .exchange(&request.frame, request.opcode, request.sequence)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if self.fail_first_in_batch && requests.len() == 2 {
            responses[0] = FakePeer::response(
                Opcode::StoreUploadWrite,
                requests[0].sequence,
                Status::InternalError,
                &[],
            );
            self.fail_first_in_batch = false;
        }
        Ok(responses)
    }
}

fn window_peer(window: usize) -> WindowPeer {
    WindowPeer {
        inner: FakePeer {
            writes: Vec::new(),
            sequences: Vec::new(),
            aborts: 0,
            short_write_index: None,
            list_polls: 0,
            visible_after_poll: None,
            list_error_until_poll: None,
            fault: None,
        },
        window,
        batch_sizes: Vec::new(),
        fail_first_in_batch: false,
    }
}

#[test]
fn negotiated_window_sends_two_ascending_writes_before_waiting() {
    let mut source = source();
    let mut peer = window_peer(2);
    let mut progress = FakeProgress::default();

    upload_stream(
        &mut source,
        UploadRequest::new("fixture.binbook", true),
        &mut peer,
        &mut progress,
        &FakeClock,
    )
    .unwrap();

    assert_eq!(peer.batch_sizes, [2, 1]);
    assert_eq!(
        peer.inner
            .writes
            .iter()
            .map(|write| (write.0, write.1))
            .collect::<Vec<_>>(),
        [(2, 0), (3, 3_584), (4, 7_168)]
    );
    assert_eq!(
        progress
            .0
            .iter()
            .filter_map(|event| match event {
                ProgressEvent::Transfer { acknowledged, .. } => Some(*acknowledged),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [3_584, 7_168, 8_000]
    );
}

#[test]
fn legacy_window_never_sends_more_than_one_write() {
    let mut source = source();
    let mut peer = window_peer(1);

    upload_stream(
        &mut source,
        UploadRequest::new("fixture.binbook", true),
        &mut peer,
        &mut FakeProgress::default(),
        &FakeClock,
    )
    .unwrap();

    assert_eq!(peer.batch_sizes, [1, 1, 1]);
}

#[test]
fn failed_first_response_collects_window_then_aborts_without_progress() {
    let mut source = source();
    let mut peer = window_peer(2);
    peer.fail_first_in_batch = true;
    let mut progress = FakeProgress::default();

    let error = upload_stream(
        &mut source,
        UploadRequest::new("fixture.binbook", true),
        &mut peer,
        &mut progress,
        &FakeClock,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        UploadError::DeviceStatus {
            cleanup_confirmed: true,
            ..
        }
    ));
    assert_eq!(peer.batch_sizes, [2]);
    assert_eq!(peer.inner.writes.len(), 2);
    assert_eq!(peer.inner.aborts, 1);
    assert!(!progress
        .0
        .iter()
        .any(|event| matches!(event, ProgressEvent::Transfer { .. })));
}

#[test]
fn source_failure_after_two_acknowledgements_aborts_upload() {
    let mut source = FailingTransferReader {
        inner: Cursor::new((0..12_000).map(|value| value as u8).collect()),
        pass: 0,
        transferred: 0,
    };
    let mut peer = window_peer(1);
    let mut progress = FakeProgress::default();

    let error = upload_stream(
        &mut source,
        UploadRequest::new("fixture.binbook", true),
        &mut peer,
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
    assert_eq!(peer.inner.aborts, 1);
    assert_eq!(
        progress
            .0
            .iter()
            .filter(|event| matches!(event, ProgressEvent::Transfer { .. }))
            .count(),
        2
    );
}

#[test]
fn upload_streams_twice_with_aligned_chunks_and_acknowledged_progress() {
    let mut source = source();
    let mut peer = FakePeer {
        writes: Vec::new(),
        sequences: Vec::new(),
        aborts: 0,
        short_write_index: None,
        list_polls: 0,
        visible_after_poll: None,
        list_error_until_poll: None,
        fault: None,
    };
    let mut progress = FakeProgress::default();

    let summary = upload_stream(
        &mut source,
        UploadRequest::new("fixture.binbook", true),
        &mut peer,
        &mut progress,
        &FakeClock,
    )
    .unwrap();

    assert_eq!(UPLOAD_CHUNK_BYTES, 3_584);
    assert_eq!(source.largest_read, UPLOAD_CHUNK_BYTES);
    assert_eq!(
        peer.writes.iter().map(|write| write.1).collect::<Vec<_>>(),
        [0, 3_584, 7_168]
    );
    assert_eq!(
        peer.writes
            .iter()
            .map(|write| write.2.len())
            .collect::<Vec<_>>(),
        [3_584, 3_584, 832]
    );
    assert_eq!(peer.sequences, [1, 2, 3, 4, 5]);
    assert_eq!(summary.bytes, 8_000);
    assert_eq!(summary.crc32, 0xDB8A_CB75);
    assert_eq!(summary.elapsed_ms, 2_000);
    assert_eq!(summary.bytes_per_second, 4_000);
    assert_eq!(peer.aborts, 0);

    let acknowledged = progress
        .0
        .iter()
        .filter_map(|event| match event {
            ProgressEvent::Transfer { acknowledged, .. } => Some(*acknowledged),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(acknowledged, [3_584, 7_168, 8_000]);
    assert!(progress.0.contains(&ProgressEvent::Verifying));
}

#[test]
fn short_acknowledgement_aborts_without_reporting_completion() {
    let mut source = source();
    let mut peer = FakePeer {
        writes: Vec::new(),
        sequences: Vec::new(),
        aborts: 0,
        short_write_index: Some(1),
        list_polls: 0,
        visible_after_poll: None,
        list_error_until_poll: None,
        fault: None,
    };
    let mut progress = FakeProgress::default();

    let error = upload_stream(
        &mut source,
        UploadRequest::new("fixture.binbook", true),
        &mut peer,
        &mut progress,
        &FakeClock,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        UploadError::AcceptedLength {
            expected: 3_584,
            actual: 3_583,
            cleanup_confirmed: true
        }
    ));
    assert_eq!(peer.aborts, 1);
    assert!(!progress
        .0
        .iter()
        .any(|event| matches!(event, ProgressEvent::Complete(_))));
    assert!(!progress.0.iter().any(|event| matches!(
        event,
        ProgressEvent::Transfer {
            acknowledged: 8_000,
            ..
        }
    )));
}

#[test]
fn library_wait_reports_exact_publication_after_bounded_polls() {
    let mut source = source();
    let mut peer = FakePeer {
        writes: Vec::new(),
        sequences: Vec::new(),
        aborts: 0,
        short_write_index: None,
        list_polls: 0,
        visible_after_poll: Some(2),
        list_error_until_poll: None,
        fault: None,
    };
    let mut progress = FakeProgress::default();

    let summary = upload_stream(
        &mut source,
        UploadRequest::new("fixture.binbook", false),
        &mut peer,
        &mut progress,
        &FakeClock,
    )
    .unwrap();

    assert!(summary.library_visible);
    assert_eq!(peer.list_polls, 2);
    assert!(progress.0.contains(&ProgressEvent::UpdatingLibrary));
}

#[test]
fn library_wait_retries_transient_error_responses_until_catalog_settles() {
    let mut source = source();
    let mut peer = FakePeer {
        writes: Vec::new(),
        sequences: Vec::new(),
        aborts: 0,
        short_write_index: None,
        list_polls: 0,
        visible_after_poll: Some(3),
        list_error_until_poll: Some(2),
        fault: None,
    };
    let mut progress = FakeProgress::default();

    let summary = upload_stream(
        &mut source,
        UploadRequest::new("fixture.binbook", false),
        &mut peer,
        &mut progress,
        &FakeClock,
    )
    .unwrap();

    assert!(summary.library_visible);
    assert_eq!(peer.list_polls, 3);
}

#[test]
fn library_wait_timeout_preserves_committed_success_without_false_visibility() {
    let mut source = source();
    let mut peer = FakePeer {
        writes: Vec::new(),
        sequences: Vec::new(),
        aborts: 0,
        short_write_index: None,
        list_polls: 0,
        visible_after_poll: None,
        list_error_until_poll: None,
        fault: None,
    };
    let mut progress = FakeProgress::default();

    let summary = upload_stream(
        &mut source,
        UploadRequest::new("fixture.binbook", false),
        &mut peer,
        &mut progress,
        &FakeClock,
    )
    .unwrap();

    assert!(!summary.library_visible);
    assert_eq!(peer.list_polls, 10);
}

#[test]
fn malformed_write_responses_abort_and_never_complete() {
    for fault in [
        ResponseFault::WrongSequence,
        ResponseFault::WrongOpcode,
        ResponseFault::NonOk,
    ] {
        let mut source = source();
        let mut peer = FakePeer {
            writes: Vec::new(),
            sequences: Vec::new(),
            aborts: 0,
            short_write_index: None,
            list_polls: 0,
            visible_after_poll: None,
            list_error_until_poll: None,
            fault: Some(fault),
        };
        let mut progress = FakeProgress::default();

        let result = upload_stream(
            &mut source,
            UploadRequest::new("fixture.binbook", true),
            &mut peer,
            &mut progress,
            &FakeClock,
        );

        assert!(result.is_err());
        assert_eq!(peer.aborts, 1);
        assert!(!progress
            .0
            .iter()
            .any(|event| matches!(event, ProgressEvent::Complete(_))));
    }
}

#[test]
fn commit_crc_mismatch_aborts_without_success() {
    let mut source = source();
    let mut peer = FakePeer {
        writes: Vec::new(),
        sequences: Vec::new(),
        aborts: 0,
        short_write_index: None,
        list_polls: 0,
        visible_after_poll: None,
        list_error_until_poll: None,
        fault: Some(ResponseFault::CommitCrc),
    };
    let mut progress = FakeProgress::default();

    let error = upload_stream(
        &mut source,
        UploadRequest::new("fixture.binbook", true),
        &mut peer,
        &mut progress,
        &FakeClock,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        UploadError::Crc {
            expected: 0xDB8A_CB75,
            actual: 0xDEAD_BEEF,
            cleanup_confirmed: true
        }
    ));
    assert_eq!(peer.aborts, 1);
    assert!(!progress
        .0
        .iter()
        .any(|event| matches!(event, ProgressEvent::Complete(_))));
}

#[test]
fn non_tty_progress_is_stable_and_reports_rate_elapsed_and_eta() {
    let mut output = Vec::new();
    let mut progress = TextProgress::new(&mut output, ProgressMode::Auto, false, "fixture.binbook");

    progress.report(ProgressEvent::Preparing { total: 8_000 });
    progress.report(ProgressEvent::Transfer {
        acknowledged: 4_000,
        total: 8_000,
        elapsed_ms: 2_000,
    });
    progress.report(ProgressEvent::Verifying);
    progress.report(ProgressEvent::UpdatingLibrary);

    let text = String::from_utf8(output).unwrap();
    assert_eq!(
        text,
        "Preparing storage for fixture.binbook (8000 bytes)\n\
Uploading 4000/8000 bytes (50.0%) at 2000 B/s; elapsed 2.0s; ETA 2.0s\n\
Verifying CRC32\n\
Updating library\n"
    );
    assert!(!text.contains('\r'));
    assert!(!text.contains("\u{1b}["));
}

#[test]
fn tty_progress_updates_one_line_and_zero_duration_is_safe() {
    let mut output = Vec::new();
    let mut progress = TextProgress::new(&mut output, ProgressMode::Always, true, "book.binbook");

    progress.report(ProgressEvent::Transfer {
        acknowledged: 1,
        total: 2,
        elapsed_ms: 0,
    });
    progress.report(ProgressEvent::Transfer {
        acknowledged: 2,
        total: 2,
        elapsed_ms: 0,
    });
    progress.report(ProgressEvent::Verifying);

    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("\rUploading 1/2 bytes (50.0%) at 0 B/s; elapsed 0.0s; ETA unknown"));
    assert!(text.contains("\rUploading 2/2 bytes (100.0%) at 0 B/s; elapsed 0.0s; ETA 0.0s"));
    assert!(text.ends_with("\nVerifying CRC32\n"));
}

#[test]
fn never_progress_suppresses_every_phase() {
    let mut output = Vec::new();
    let mut progress = TextProgress::new(&mut output, ProgressMode::Never, true, "book.binbook");
    progress.report(ProgressEvent::Preparing { total: 2 });
    progress.report(ProgressEvent::Transfer {
        acknowledged: 2,
        total: 2,
        elapsed_ms: 1,
    });
    progress.report(ProgressEvent::Verifying);
    progress.report(ProgressEvent::UpdatingLibrary);
    assert!(output.is_empty());
}
