#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::{
    Status, StoreBulkData, StoreBulkState, StoreUploadCommitResult, TransferDirection,
};
use binbook_fw::{
    bulk_slots::BulkSlotError,
    bulk_upload_pipeline::{BulkUploadError, BulkUploadPipeline},
};

fn data<'a>(stream_id: u32, offset: u32, bytes: &'a [u8], end: bool) -> StoreBulkData<'a> {
    StoreBulkData {
        stream_id,
        offset,
        end_of_data: end,
        data: bytes,
    }
}

#[test]
fn upload_admits_four_slots_and_emits_cumulative_durable_credits() {
    let mut pipeline = Box::new(BulkUploadPipeline::new());
    pipeline.open(7, 5 * 3_584, 0, 3_584, 4).unwrap();
    let first = pipeline.admit(data(7, 0, &[1; 3_584], false)).unwrap();
    let second = pipeline.admit(data(7, 3_584, &[2; 3_584], false)).unwrap();
    let third = pipeline.admit(data(7, 7_168, &[3; 3_584], false)).unwrap();
    let fourth = pipeline.admit(data(7, 10_752, &[4; 3_584], false)).unwrap();
    assert_eq!(pipeline.admitted_offset(), 14_336);
    assert_eq!(pipeline.durable_offset(), 0);

    pipeline.take_for_storage(first).unwrap();
    assert_eq!(pipeline.complete_storage(first, &[1; 3_584]).unwrap(), None);
    assert_eq!(pipeline.durable_offset(), 3_584);
    pipeline.take_for_storage(second).unwrap();
    assert_eq!(
        pipeline.complete_storage(second, &[2; 3_584]).unwrap(),
        None
    );
    pipeline.take_for_storage(third).unwrap();
    assert_eq!(pipeline.complete_storage(third, &[3; 3_584]).unwrap(), None);
    pipeline.take_for_storage(fourth).unwrap();
    let credit = pipeline
        .complete_storage(fourth, &[4; 3_584])
        .unwrap()
        .unwrap();
    assert_eq!(credit.stream_id, 7);
    assert_eq!(credit.durable_offset, 14_336);
    assert_eq!(credit.credits, 4);
    assert_eq!(credit.status, Status::Ok);
}

#[test]
fn upload_finish_requires_exact_length_end_flag_and_crc() {
    let first = [1; 3_584];
    let final_bytes = [2; 512];
    let mut crc = u32::MAX;
    for byte in first.iter().chain(final_bytes.iter()) {
        crc = binbook_fw::crc32::update_byte(crc, *byte);
    }
    let expected_crc = !crc;
    let mut pipeline = Box::new(BulkUploadPipeline::new());
    pipeline.open(7, 4_096, expected_crc, 3_584, 4).unwrap();
    let a = pipeline.admit(data(7, 0, &first, false)).unwrap();
    let b = pipeline.admit(data(7, 3_584, &final_bytes, true)).unwrap();
    pipeline.take_for_storage(a).unwrap();
    pipeline.complete_storage(a, &first).unwrap();
    pipeline.take_for_storage(b).unwrap();
    pipeline.complete_storage(b, &final_bytes).unwrap();
    let finish = pipeline.finish(7).unwrap();
    assert_eq!(finish.result, StoreUploadCommitResult::Ok);
    assert_eq!(finish.total_bytes, 4_096);
    assert_eq!(finish.computed_crc32, expected_crc);

    let mut mismatch = Box::new(BulkUploadPipeline::new());
    mismatch.open(8, 512, 1, 3_584, 1).unwrap();
    let only = mismatch.admit(data(8, 0, &[3; 512], true)).unwrap();
    mismatch.take_for_storage(only).unwrap();
    mismatch.complete_storage(only, &[3; 512]).unwrap();
    assert_eq!(
        mismatch.finish(8).unwrap().result,
        StoreUploadCommitResult::CrcMismatch
    );
}

#[test]
fn upload_rejects_offsets_end_flags_wrong_stream_and_pool_overflow() {
    let mut pipeline = Box::new(BulkUploadPipeline::new());
    pipeline.open(7, 5 * 512, 0, 512, 4).unwrap();
    assert_eq!(
        pipeline.admit(data(8, 0, &[0; 512], false)),
        Err(BulkUploadError::WrongStream)
    );
    assert_eq!(
        pipeline.admit(data(7, 512, &[0; 512], false)),
        Err(BulkUploadError::WrongOffset)
    );
    assert_eq!(
        pipeline.admit(data(7, 0, &[0; 512], true)),
        Err(BulkUploadError::EndFlag)
    );
    for index in 0..4 {
        pipeline
            .admit(data(7, index * 512, &[index as u8; 512], false))
            .unwrap();
    }
    assert_eq!(
        pipeline.admit(data(7, 2_048, &[0; 512], true)),
        Err(BulkUploadError::Slot(BulkSlotError::Full))
    );
}

#[test]
fn status_is_a_quiescent_barrier_with_full_credits() {
    let mut pipeline = Box::new(BulkUploadPipeline::new());
    pipeline.open(7, 1024, 0, 512, 4).unwrap();
    let descriptor = pipeline.admit(data(7, 0, &[1; 512], false)).unwrap();
    assert_eq!(pipeline.status(7), Err(BulkUploadError::Busy));
    pipeline.take_for_storage(descriptor).unwrap();
    pipeline.complete_storage(descriptor, &[1; 512]).unwrap();
    let status = pipeline.status(7).unwrap();
    assert_eq!(status.direction, TransferDirection::Upload);
    assert_eq!(status.state, StoreBulkState::Receiving);
    assert_eq!(status.available_credits, 4);
    assert_eq!(status.durable_offset, 512);
}

#[test]
fn abort_invalidates_descriptors_and_is_idempotent_for_last_stream() {
    let mut pipeline = Box::new(BulkUploadPipeline::new());
    pipeline.open(7, 1024, 0, 512, 4).unwrap();
    let stale = pipeline.admit(data(7, 0, &[1; 512], false)).unwrap();
    assert_eq!(pipeline.abort(7), Ok(()));
    assert_eq!(pipeline.abort(7), Ok(()));
    assert_eq!(
        pipeline.take_for_storage(stale),
        Err(BulkUploadError::Inactive)
    );
    assert_eq!(pipeline.abort(8), Err(BulkUploadError::WrongStream));
    pipeline.open(8, 512, 0, 512, 1).unwrap();
    assert_eq!(pipeline.abort(7), Err(BulkUploadError::WrongStream));
}
