use binbook_diagnostic_protocol::{
    TransferDirection, TransferTraceSnapshot, TransferTraceState, TRANSFER_TRACE_FLAG_SATURATED,
};

pub(crate) fn add(field: &mut u32, value: u32, flags: &mut u8) {
    let Some(sum) = field.checked_add(value) else {
        *field = u32::MAX;
        *flags |= TRANSFER_TRACE_FLAG_SATURATED;
        return;
    };
    *field = sum;
}

pub(crate) fn add_u64(field: &mut u32, value: u64, flags: &mut u8) {
    let value = saturating_u32(value, flags);
    add(field, value, flags);
}

pub(crate) fn saturating_u32(value: u64, flags: &mut u8) -> u32 {
    if value > u64::from(u32::MAX) {
        *flags |= TRANSFER_TRACE_FLAG_SATURATED;
        u32::MAX
    } else {
        value as u32
    }
}

pub(crate) fn empty_snapshot(
    trace_id: u32,
    direction: TransferDirection,
    selected_window: u8,
    chunk_size: u16,
    protocol_errors: u32,
) -> TransferTraceSnapshot {
    TransferTraceSnapshot {
        direction,
        state: TransferTraceState::Active,
        selected_window,
        trace_id,
        chunk_size,
        peak_rx_bytes: 0,
        logical_bytes: 0,
        frames: 0,
        usb_rx_bytes: 0,
        usb_tx_bytes: 0,
        usb_rx_us: 0,
        usb_tx_us: 0,
        framing_us: 0,
        frame_crc_us: 0,
        owner_queue_us: 0,
        owner_execute_us: 0,
        sd_read_calls: 0,
        sd_write_calls: 0,
        sd_read_blocks: 0,
        sd_write_blocks: 0,
        sd_read_logical_bytes: 0,
        sd_write_logical_bytes: 0,
        sd_read_us: 0,
        sd_write_us: 0,
        fat_allocation_us: 0,
        upload_prepare_us: 0,
        data_io_us: 0,
        crc_us: 0,
        commit_us: 0,
        library_refresh_us: 0,
        data_phase_us: 0,
        end_to_end_us: 0,
        usb_sd_overlap_us: 0,
        peak_slots: 0,
        flags: 0,
        queue_stalls: 0,
        protocol_errors_start: protocol_errors,
        protocol_errors_end: protocol_errors,
    }
}
