#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::{Status, StoreBulkCredit, StoreUploadCommitResult};
use binbook_fw::bulk_readback_pipeline::{BulkReadbackError, BulkReadbackPipeline};
use binbook_fw::bulk_slots::BulkSlotError;

fn fill_and_send(
    pipeline: &mut BulkReadbackPipeline,
    byte: u8,
) -> (binbook_fw::bulk_slots::BulkSlotDescriptor, Vec<u8>) {
    let descriptor = pipeline.reserve_next().unwrap();
    let bytes = vec![byte; usize::from(descriptor.len)];
    pipeline.complete_read(descriptor).unwrap();
    let frame = pipeline.mark_sent(descriptor, &bytes).unwrap();
    assert_eq!(frame.data, vec![byte; frame.data.len()]);
    (descriptor, bytes)
}

#[test]
fn readback_holds_four_sent_slots_until_host_durable_credit() {
    let mut pipeline = BulkReadbackPipeline::new();
    assert_eq!(pipeline.open(11, 15_000, 3_584, 4), Ok(4));

    let sent = (1..=4)
        .map(|byte| fill_and_send(&mut pipeline, byte))
        .collect::<Vec<_>>();
    assert_eq!(pipeline.produced_offset(), 14_336);
    assert_eq!(pipeline.durable_offset(), 0);
    assert_eq!(
        pipeline.reserve_next(),
        Err(BulkReadbackError::Slot(BulkSlotError::Full))
    );

    pipeline
        .apply_credit(
            StoreBulkCredit {
                stream_id: 11,
                durable_offset: 7_168,
                credits: 2,
                status: Status::Ok,
            },
            &[sent[0].1.as_slice(), sent[1].1.as_slice()],
        )
        .unwrap();
    assert_eq!(pipeline.durable_offset(), 7_168);
    assert!(pipeline.reserve_next().is_ok());
}

#[test]
fn readback_validates_credit_stream_count_offset_and_status() {
    let mut pipeline = BulkReadbackPipeline::new();
    pipeline.open(11, 8_000, 3_584, 4).unwrap();
    let first = fill_and_send(&mut pipeline, 1);
    let second = fill_and_send(&mut pipeline, 2);

    for credit in [
        StoreBulkCredit {
            stream_id: 12,
            durable_offset: 7_168,
            credits: 2,
            status: Status::Ok,
        },
        StoreBulkCredit {
            stream_id: 11,
            durable_offset: 7_168,
            credits: 1,
            status: Status::Ok,
        },
        StoreBulkCredit {
            stream_id: 11,
            durable_offset: 7_169,
            credits: 2,
            status: Status::Ok,
        },
        StoreBulkCredit {
            stream_id: 11,
            durable_offset: 7_168,
            credits: 2,
            status: Status::Error,
        },
    ] {
        assert!(pipeline
            .apply_credit(credit, &[first.1.as_slice(), second.1.as_slice()])
            .is_err());
    }
    assert_eq!(pipeline.durable_offset(), 0);
}

#[test]
fn readback_status_discards_unacknowledged_slots_and_restarts_at_durable_offset() {
    let mut pipeline = BulkReadbackPipeline::new();
    pipeline.open(11, 12_000, 3_584, 4).unwrap();
    let first = fill_and_send(&mut pipeline, 1);
    let _second = fill_and_send(&mut pipeline, 2);
    pipeline
        .apply_credit(
            StoreBulkCredit {
                stream_id: 11,
                durable_offset: 3_584,
                credits: 1,
                status: Status::Ok,
            },
            &[first.1.as_slice()],
        )
        .unwrap();
    fill_and_send(&mut pipeline, 3);

    let status = pipeline.status(11).unwrap();
    assert_eq!(status.durable_offset, 3_584);
    assert_eq!(status.available_credits, 4);
    assert_eq!(pipeline.produced_offset(), 3_584);
    let resumed = pipeline.reserve_next().unwrap();
    assert_eq!(resumed.offset, 3_584);
}

#[test]
fn readback_eof_finish_reports_crc_and_abort_is_idempotent() {
    let mut pipeline = BulkReadbackPipeline::new();
    pipeline.open(11, 4, 512, 1).unwrap();
    let descriptor = pipeline.reserve_next().unwrap();
    pipeline.complete_read(descriptor).unwrap();
    let frame = pipeline.mark_sent(descriptor, b"book").unwrap();
    assert!(frame.end_of_data);
    pipeline
        .apply_credit(
            StoreBulkCredit {
                stream_id: 11,
                durable_offset: 4,
                credits: 1,
                status: Status::Ok,
            },
            &[b"book".as_slice()],
        )
        .unwrap();
    let finish = pipeline.finish(11).unwrap();
    assert_eq!(finish.result, StoreUploadCommitResult::Ok);
    assert_eq!(finish.total_bytes, 4);

    pipeline.open(12, 4, 512, 1).unwrap();
    pipeline.abort(12).unwrap();
    pipeline.abort(12).unwrap();
    assert_eq!(pipeline.abort(13), Err(BulkReadbackError::WrongStream));
}
