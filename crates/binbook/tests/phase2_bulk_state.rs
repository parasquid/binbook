use binbook::bulk_state::{BulkReadbackState, BulkStateError, BulkUploadState, UploadGrant};
use binbook::transfer_negotiation::{
    negotiate_transfer, RequestedMode, RequestedWindow, SelectedMode, TransferNegotiationError,
};
use binbook_diagnostic_protocol::{
    Status, StoreBulkCredit, StoreBulkData, StoreBulkState, StoreBulkStatusResponse,
    TransferDirection, TransferLimits, CAP_STORAGE_BULK_V1, CAP_STORAGE_TRANSFER_LIMITS,
    CAP_STORAGE_UPLOAD_WINDOW_2, TRANSFER_LIMIT_READ_BULK, TRANSFER_LIMIT_UPLOAD_BULK,
};

fn limits() -> TransferLimits {
    TransferLimits {
        flags: TRANSFER_LIMIT_UPLOAD_BULK | TRANSFER_LIMIT_READ_BULK,
        max_data_bytes: 3584,
        legacy_upload_window: 2,
        legacy_read_window: 2,
        bulk_slots: 4,
        rx_capacity_bytes: 16_504,
        max_frame_bytes: 4126,
        idle_timeout_ms: 60_000,
    }
}

#[test]
fn negotiation_preserves_old_peer_fallback_and_requires_bulk_contract() {
    let legacy_one = negotiate_transfer(
        0,
        None,
        TransferDirection::Upload,
        RequestedMode::Auto,
        RequestedWindow::Auto,
    )
    .unwrap();
    assert_eq!(legacy_one.mode, SelectedMode::Legacy);
    assert_eq!(legacy_one.window, 1);

    let legacy_two = negotiate_transfer(
        CAP_STORAGE_UPLOAD_WINDOW_2,
        None,
        TransferDirection::Upload,
        RequestedMode::Auto,
        RequestedWindow::Auto,
    )
    .unwrap();
    assert_eq!(legacy_two.mode, SelectedMode::Legacy);
    assert_eq!(legacy_two.window, 2);

    let capabilities = CAP_STORAGE_TRANSFER_LIMITS | CAP_STORAGE_BULK_V1;
    let bulk = negotiate_transfer(
        capabilities,
        Some(limits()),
        TransferDirection::Upload,
        RequestedMode::Auto,
        RequestedWindow::Eight,
    )
    .unwrap();
    assert_eq!(bulk.mode, SelectedMode::Bulk);
    assert_eq!(bulk.window, 4);
    assert_eq!(bulk.chunk_bytes, 3584);

    assert_eq!(
        negotiate_transfer(
            CAP_STORAGE_TRANSFER_LIMITS,
            Some(limits()),
            TransferDirection::Upload,
            RequestedMode::Bulk,
            RequestedWindow::Auto,
        ),
        Err(TransferNegotiationError::BulkUnsupported)
    );
}

#[test]
fn negotiation_rejects_missing_or_inconsistent_limits() {
    let capabilities = CAP_STORAGE_TRANSFER_LIMITS | CAP_STORAGE_BULK_V1;
    assert_eq!(
        negotiate_transfer(
            capabilities,
            None,
            TransferDirection::Upload,
            RequestedMode::Auto,
            RequestedWindow::Auto,
        ),
        Err(TransferNegotiationError::LimitsMissing)
    );

    let read_only = TransferLimits {
        flags: TRANSFER_LIMIT_READ_BULK,
        ..limits()
    };
    let selected = negotiate_transfer(
        capabilities,
        Some(read_only),
        TransferDirection::Upload,
        RequestedMode::Auto,
        RequestedWindow::Auto,
    )
    .unwrap();
    assert_eq!(selected.mode, SelectedMode::Legacy);
}

#[test]
fn upload_state_never_advances_progress_beyond_durable_credit() {
    let mut state = BulkUploadState::new(7, 8_000, 3_584, 4, 4).unwrap();
    assert_eq!(
        state.grant(3_584),
        Ok(UploadGrant {
            offset: 0,
            end_of_data: false
        })
    );
    assert_eq!(
        state.grant(3_584),
        Ok(UploadGrant {
            offset: 3_584,
            end_of_data: false
        })
    );
    assert_eq!(state.sent_offset(), 7_168);
    assert_eq!(state.durable_offset(), 0);

    state
        .apply_credit(StoreBulkCredit {
            stream_id: 7,
            durable_offset: 3_584,
            credits: 1,
            status: Status::Ok,
        })
        .unwrap();
    assert_eq!(state.durable_offset(), 3_584);
    assert_eq!(state.available_credits(), 3);
    assert_eq!(
        state.grant(832),
        Ok(UploadGrant {
            offset: 7_168,
            end_of_data: true
        })
    );
}

#[test]
fn upload_state_rejects_wrong_stream_offsets_credit_overflow_and_early_short_chunk() {
    let mut state = BulkUploadState::new(7, 8_000, 3_584, 4, 1).unwrap();
    assert_eq!(state.grant(1), Err(BulkStateError::EarlyShortChunk));
    state.grant(3_584).unwrap();
    assert_eq!(state.grant(3_584), Err(BulkStateError::NoCredit));
    assert_eq!(
        state.apply_credit(StoreBulkCredit {
            stream_id: 8,
            durable_offset: 3_584,
            credits: 1,
            status: Status::Ok,
        }),
        Err(BulkStateError::WrongStream)
    );
    assert_eq!(
        state.apply_credit(StoreBulkCredit {
            stream_id: 7,
            durable_offset: 3_585,
            credits: 1,
            status: Status::Ok,
        }),
        Err(BulkStateError::InvalidDurableOffset)
    );
}

#[test]
fn upload_status_barrier_discards_uncertain_packets_and_resumes_exactly() {
    let mut state = BulkUploadState::new(7, 12_000, 3_584, 4, 4).unwrap();
    state.grant(3_584).unwrap();
    state.grant(3_584).unwrap();
    state.grant(3_584).unwrap();
    assert_eq!(state.sent_offset(), 10_752);

    state
        .apply_status(StoreBulkStatusResponse {
            stream_id: 7,
            direction: TransferDirection::Upload,
            state: StoreBulkState::Receiving,
            available_credits: 4,
            durable_offset: 3_584,
            total_size: 12_000,
            running_crc32: 1,
        })
        .unwrap();
    assert_eq!(state.sent_offset(), 3_584);
    assert_eq!(state.durable_offset(), 3_584);
    assert_eq!(state.available_credits(), 4);
    assert_eq!(state.grant(3_584).unwrap().offset, 3_584);
}

#[test]
fn readback_releases_batched_credit_only_after_bytes_are_durable() {
    let mut state = BulkReadbackState::new(9, 8_000, 3_584, 4).unwrap();
    let first = StoreBulkData {
        stream_id: 9,
        offset: 0,
        end_of_data: false,
        data: &[1; 3_584],
    };
    state.accept(&first).unwrap();
    assert_eq!(state.received_offset(), 3_584);
    assert_eq!(state.durable_offset(), 0);
    assert_eq!(state.mark_durable(0, 3_584).unwrap(), None);

    let second = StoreBulkData {
        stream_id: 9,
        offset: 3_584,
        end_of_data: false,
        data: &[2; 3_584],
    };
    state.accept(&second).unwrap();
    let credit = state.mark_durable(3_584, 3_584).unwrap().unwrap();
    assert_eq!(credit.stream_id, 9);
    assert_eq!(credit.durable_offset, 7_168);
    assert_eq!(credit.credits, 2);
    assert_eq!(credit.status, Status::Ok);

    let final_data = StoreBulkData {
        stream_id: 9,
        offset: 7_168,
        end_of_data: true,
        data: &[3; 832],
    };
    state.accept(&final_data).unwrap();
    let final_credit = state.mark_durable(7_168, 832).unwrap().unwrap();
    assert_eq!(final_credit.durable_offset, 8_000);
    assert_eq!(final_credit.credits, 1);
}

#[test]
fn readback_rejects_wrong_offsets_and_end_flags_then_recovers_at_status_barrier() {
    let mut state = BulkReadbackState::new(9, 8_000, 3_584, 4).unwrap();
    assert_eq!(
        state.accept(&StoreBulkData {
            stream_id: 9,
            offset: 1,
            end_of_data: false,
            data: &[1; 3_584],
        }),
        Err(BulkStateError::WrongOffset)
    );
    assert_eq!(
        state.accept(&StoreBulkData {
            stream_id: 9,
            offset: 0,
            end_of_data: true,
            data: &[1; 3_584],
        }),
        Err(BulkStateError::InvalidEndFlag)
    );
    state
        .accept(&StoreBulkData {
            stream_id: 9,
            offset: 0,
            end_of_data: false,
            data: &[1; 3_584],
        })
        .unwrap();
    state.mark_durable(0, 3_584).unwrap();
    state
        .accept(&StoreBulkData {
            stream_id: 9,
            offset: 3_584,
            end_of_data: false,
            data: &[2; 3_584],
        })
        .unwrap();

    state
        .apply_status(StoreBulkStatusResponse {
            stream_id: 9,
            direction: TransferDirection::Readback,
            state: StoreBulkState::Receiving,
            available_credits: 4,
            durable_offset: 3_584,
            total_size: 8_000,
            running_crc32: 1,
        })
        .unwrap();
    assert_eq!(state.received_offset(), 3_584);
    assert_eq!(state.durable_offset(), 3_584);
}
