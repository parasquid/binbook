use binbook_diagnostic_protocol::{encode_crash_response, Status, CRASH_SUMMARY_BYTES};
use binbook_fw::{
    async_refresh::{DisplayProbeKind, DisplayRequest},
    diag::{
        complete_pending_command, poll_runtime_command, queue_runtime_response, PendingAction,
        RuntimeCommand, SerialState,
    },
    diag_flash::CrashStore,
    runtime_engine::{RuntimeCompletionStatus, RuntimeEvent, RuntimeEventKind},
};

use super::{
    diagnostic_aggregator::query_aggregator, AggregatorQuery, AggregatorResponse,
    AGGREGATOR_COMPLETION_CHANNEL, RUNTIME_EVENT_CHANNEL,
};

#[embassy_executor::task]
pub(super) async fn diagnostic_task(
    usb_device: esp_hal::peripherals::USB_DEVICE<'static>,
    flash: esp_hal::peripherals::FLASH<'static>,
) {
    use embassy_futures::select::{select3, Either3};
    #[cfg(all(feature = "sd-storage", feature = "bulk-transfer-runtime"))]
    use embassy_futures::select::{select4, Either4};
    #[cfg(not(feature = "usb-rx-isr-buffer"))]
    let (mut usb_rx, mut usb_tx) = esp_hal::usb_serial_jtag::UsbSerialJtag::new(usb_device)
        .into_async()
        .split();
    #[cfg(feature = "usb-rx-isr-buffer")]
    let (mut usb_rx, mut usb_tx) = super::usb_rx_isr::install(usb_device);
    let mut serial = SerialState::new();
    let mut crash_store = CrashStore::new(esp_storage::FlashStorage::new(flash));
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    #[cfg(feature = "sd-storage")]
    static OWNER_TRANSFER: static_cell::StaticCell<
        [u8; binbook_fw::storage_owner::MAX_BOUNDED_IO_BYTES as usize],
    > = static_cell::StaticCell::new();
    #[cfg(feature = "sd-storage")]
    static OWNER_RESPONSE: static_cell::StaticCell<
        [u8; binbook_diagnostic_protocol::MAX_PAYLOAD_BYTES],
    > = static_cell::StaticCell::new();
    #[cfg(feature = "sd-storage")]
    let mut owner_transfer =
        OWNER_TRANSFER.init([0; binbook_fw::storage_owner::MAX_BOUNDED_IO_BYTES as usize]);
    #[cfg(feature = "sd-storage")]
    let owner_response = OWNER_RESPONSE.init([0; binbook_diagnostic_protocol::MAX_PAYLOAD_BYTES]);
    #[cfg(feature = "sd-storage")]
    let mut owner_runtime = super::diagnostic_owner_storage::DiagnosticOwnerRuntime::new();
    #[cfg(all(feature = "sd-storage", feature = "bulk-transfer-runtime"))]
    static BULK_BUFFERS: static_cell::StaticCell<
        [[u8; binbook_fw::bulk_slots::TRANSFER_DATA_BYTES];
            binbook_fw::bulk_slots::TRANSFER_SLOT_COUNT],
    > = static_cell::StaticCell::new();
    #[cfg(all(feature = "sd-storage", feature = "bulk-transfer-runtime"))]
    let mut bulk_runtime = super::diagnostic_bulk::DiagnosticBulkRuntime::new(
        BULK_BUFFERS
            .init(
                [[0; binbook_fw::bulk_slots::TRANSFER_DATA_BYTES];
                    binbook_fw::bulk_slots::TRANSFER_SLOT_COUNT],
            )
            .each_mut(),
    );
    let completion_rx = AGGREGATOR_COMPLETION_CHANNEL.receiver();
    let mut published_protocol_errors = 0;
    let mut transfer_trace = binbook_fw::transfer_trace_command::TransferTraceCommand::new();

    loop {
        let mut usb_read_buf = [0u8; 64];
        #[cfg(feature = "usb-rx-isr-buffer")]
        if usb_rx.take_overflowed() {
            serial.note_rx_overflow();
        }
        if !serial.needs_service(false) {
            #[cfg(all(feature = "sd-storage", feature = "bulk-transfer-runtime"))]
            let mut bulk_wait_serviced = false;
            #[cfg(all(feature = "sd-storage", feature = "bulk-transfer-runtime"))]
            if bulk_runtime.pending_owner() != 0 {
                let timeout_wait =
                    bulk_runtime.timeout_wait_ms(embassy_time::Instant::now().as_millis());
                match select4(
                    embedded_io_async::Read::read(&mut usb_rx, &mut usb_read_buf),
                    completion_rx.receive(),
                    super::STORAGE_DIAGNOSTIC_RESPONSE_CHANNEL.receive(),
                    embassy_time::Timer::after_millis(timeout_wait),
                )
                .await
                {
                    Either4::First(Ok(received)) => {
                        if received == 0 {
                            embassy_time::Timer::after_millis(1).await;
                        } else {
                            feed_profiled(
                                &mut serial,
                                &mut transfer_trace,
                                &usb_read_buf[..received],
                            );
                        }
                    }
                    Either4::First(Err(error)) => match error {},
                    Either4::Second(committed) => {
                        let status = match committed.completion.status {
                            RuntimeCompletionStatus::Ok => Status::Ok,
                            RuntimeCompletionStatus::Error => Status::Error,
                        };
                        let _ = complete_pending_command(
                            &mut serial,
                            committed.pending,
                            status,
                            committed.completion.page,
                            &[],
                        );
                    }
                    Either4::Third(response) => {
                        bulk_runtime
                            .mark_commit_timing(response.envelope.sequence, &mut transfer_trace);
                        drain_transfer_timings(&mut transfer_trace);
                        bulk_runtime
                            .handle_owner(
                                &mut serial,
                                response,
                                embassy_time::Instant::now().as_millis(),
                                &mut transfer_trace,
                            )
                            .await;
                    }
                    Either4::Fourth(_) => {}
                }
                bulk_wait_serviced = true;
            }
            #[cfg(not(all(feature = "sd-storage", feature = "bulk-transfer-runtime")))]
            let bulk_wait_serviced = false;
            if !bulk_wait_serviced {
                #[cfg(all(feature = "sd-storage", feature = "bulk-transfer-runtime"))]
                let timeout_wait =
                    bulk_runtime.timeout_wait_ms(embassy_time::Instant::now().as_millis());
                #[cfg(not(all(feature = "sd-storage", feature = "bulk-transfer-runtime")))]
                let timeout_wait = 60_000;
                match select3(
                    embedded_io_async::Read::read(&mut usb_rx, &mut usb_read_buf),
                    completion_rx.receive(),
                    embassy_time::Timer::after_millis(timeout_wait),
                )
                .await
                {
                    Either3::First(Ok(received)) => {
                        if received == 0 {
                            embassy_time::Timer::after_millis(1).await;
                        } else {
                            feed_profiled(
                                &mut serial,
                                &mut transfer_trace,
                                &usb_read_buf[..received],
                            );
                        }
                    }
                    Either3::First(Err(error)) => match error {},
                    Either3::Second(committed) => {
                        let status = match committed.completion.status {
                            RuntimeCompletionStatus::Ok => Status::Ok,
                            RuntimeCompletionStatus::Error => Status::Error,
                        };
                        let _ = complete_pending_command(
                            &mut serial,
                            committed.pending,
                            status,
                            committed.completion.page,
                            &[],
                        );
                    }
                    Either3::Third(_) => {}
                }
            }
        }
        #[cfg(all(feature = "sd-storage", feature = "bulk-transfer-runtime"))]
        bulk_runtime
            .handle_idle_timeout(embassy_time::Instant::now().as_millis())
            .await;
        #[cfg(feature = "sd-storage")]
        drain_transfer_timings(&mut transfer_trace);
        while !serial.frame_ready() {
            let received = usb_rx.drain_rx_fifo(&mut usb_read_buf);
            if received == 0 {
                break;
            }
            feed_profiled(&mut serial, &mut transfer_trace, &usb_read_buf[..received]);
        }

        while let Ok(committed) = AGGREGATOR_COMPLETION_CHANNEL.receiver().try_receive() {
            let status = match committed.completion.status {
                RuntimeCompletionStatus::Ok => Status::Ok,
                RuntimeCompletionStatus::Error => Status::Error,
            };
            let _ = complete_pending_command(
                &mut serial,
                committed.pending,
                status,
                committed.completion.page,
                &[],
            );
        }

        if serial.frame_ready() {
            let snapshot = match query_aggregator(AggregatorQuery::Status).await {
                AggregatorResponse::Status(snapshot) => snapshot,
                AggregatorResponse::Reserve(_)
                | AggregatorResponse::Log { .. }
                | AggregatorResponse::Ack => {
                    continue;
                }
            };
            #[cfg(feature = "sd-storage")]
            let mut observe_decode = |event| {
                let now_us = embassy_time::Instant::now().as_micros();
                match event {
                    binbook_fw::diag::RuntimeProfileEvent::Frame(event) => {
                        transfer_trace.observe_decode(event, now_us);
                    }
                    binbook_fw::diag::RuntimeProfileEvent::Bulk(
                        binbook_fw::bulk_protocol_dispatch::BulkDispatchEvent::CrcStart,
                    ) => transfer_trace
                        .enter_phase(binbook_fw::transfer_metrics::TransferPhase::Crc, now_us),
                    binbook_fw::diag::RuntimeProfileEvent::Bulk(
                        binbook_fw::bulk_protocol_dispatch::BulkDispatchEvent::CrcEnd,
                    ) => transfer_trace
                        .exit_phase(binbook_fw::transfer_metrics::TransferPhase::Crc, now_us),
                }
            };
            #[cfg(feature = "bulk-transfer-runtime")]
            let command = binbook_fw::diag::poll_runtime_command_bulk_owner_storage_profiled(
                &mut serial,
                snapshot,
                &mut storage,
                owner_transfer,
                bulk_runtime.coordinator(),
                &mut observe_decode,
            );
            #[cfg(all(feature = "sd-storage", not(feature = "bulk-transfer-runtime")))]
            let command = binbook_fw::diag::poll_runtime_command_owner_storage_profiled(
                &mut serial,
                snapshot,
                &mut storage,
                owner_transfer,
                &mut observe_decode,
            );
            #[cfg(not(feature = "sd-storage"))]
            let command = poll_runtime_command(&mut serial, snapshot, &mut storage);
            if let Some(command) = command {
                let header = command.header();
                if header.opcode != binbook_diagnostic_protocol::Opcode::StoreSdWriteTrace {
                    RUNTIME_EVENT_CHANNEL
                        .sender()
                        .send(RuntimeEvent {
                            timestamp_ms: embassy_time::Instant::now().as_millis(),
                            kind: RuntimeEventKind::ProtocolCommand {
                                opcode: header.opcode as u8,
                                sequence: header.sequence,
                            },
                        })
                        .await;
                }
                #[cfg(feature = "sd-storage")]
                match command {
                    #[cfg(feature = "bulk-transfer-runtime")]
                    RuntimeCommand::Bulk { header, action } => {
                        bulk_runtime
                            .handle_action(
                                &mut serial,
                                header,
                                action,
                                embassy_time::Instant::now().as_millis(),
                                &mut transfer_trace,
                            )
                            .await;
                    }
                    #[cfg(not(feature = "bulk-transfer-runtime"))]
                    RuntimeCommand::Bulk { .. } => {}
                    RuntimeCommand::OwnerStorage { header, command } => {
                        owner_transfer =
                            super::diagnostic_owner_storage::handle_owner_storage_command(
                                &mut serial,
                                &mut owner_runtime,
                                header,
                                command,
                                owner_transfer,
                                owner_response,
                                &mut transfer_trace,
                            )
                            .await;
                    }
                    command => {
                        handle_command(
                            &mut serial,
                            &mut crash_store,
                            snapshot,
                            command,
                            &mut transfer_trace,
                        )
                        .await;
                    }
                }
                #[cfg(not(feature = "sd-storage"))]
                handle_command(
                    &mut serial,
                    &mut crash_store,
                    snapshot,
                    command,
                    &mut transfer_trace,
                )
                .await;
            }
        }

        if !serial.pending_tx().is_empty() {
            let pending_len = serial.pending_tx().len();
            transfer_trace.enter_phase(
                binbook_fw::transfer_metrics::TransferPhase::UsbTx,
                embassy_time::Instant::now().as_micros(),
            );
            // ESP HAL's async write already waits for each 64-byte IN packet to
            // empty. A subsequent flush can wait forever if the host closes the
            // port immediately after reading the response.
            let sent = embedded_io_async::Write::write_all(&mut usb_tx, serial.pending_tx())
                .await
                .is_ok();
            transfer_trace.exit_phase(
                binbook_fw::transfer_metrics::TransferPhase::UsbTx,
                embassy_time::Instant::now().as_micros(),
            );
            if sent {
                transfer_trace.record_usb_tx(pending_len);
                serial.consume_tx(pending_len);
            }
        }

        let protocol_errors = serial.protocol_error_count();
        if protocol_errors != published_protocol_errors {
            let _ = query_aggregator(AggregatorQuery::ProtocolErrors(protocol_errors)).await;
            published_protocol_errors = protocol_errors;
        }
    }
}

fn feed_profiled(
    serial: &mut SerialState,
    transfer_trace: &mut binbook_fw::transfer_trace_command::TransferTraceCommand,
    bytes: &[u8],
) {
    transfer_trace.enter_phase(
        binbook_fw::transfer_metrics::TransferPhase::UsbRx,
        embassy_time::Instant::now().as_micros(),
    );
    serial.feed_rx(bytes);
    transfer_trace.record_rx_buffered(serial.buffered_rx_bytes());
    transfer_trace.exit_phase(
        binbook_fw::transfer_metrics::TransferPhase::UsbRx,
        embassy_time::Instant::now().as_micros(),
    );
}

#[cfg(feature = "sd-storage")]
fn drain_transfer_timings(
    transfer_trace: &mut binbook_fw::transfer_trace_command::TransferTraceCommand,
) {
    while let Ok(event) = super::TRANSFER_OWNER_TIMING_CHANNEL
        .receiver()
        .try_receive()
    {
        transfer_trace.observe_owner_timing(event);
    }
    transfer_trace.observe_block_io_aggregate(super::TRANSFER_BLOCK_IO.take());
    while let Ok(timing) = super::TRANSFER_LIBRARY_TIMING_CHANNEL
        .receiver()
        .try_receive()
    {
        transfer_trace.complete_library_refresh(timing.trace_id, timing.elapsed_us);
    }
    transfer_trace.record_instrumentation_drops(
        super::TRANSFER_OWNER_TIMING_DROPS.swap(0, portable_atomic::Ordering::Relaxed),
    );
}

#[cfg(all(feature = "sd-storage", target_arch = "riscv32"))]
#[embassy_executor::task]
pub(super) async fn diagnostic_task_with_sd(
    usb_device: esp_hal::peripherals::USB_DEVICE<'static>,
    flash: esp_hal::peripherals::FLASH<'static>,
    sd_fs: binbook_fw::storage_access::SdHandle<
        'static,
        binbook_fw::storage::SdFilesystem<
            binbook_fw::board::FreqManagedSpiDevice<'static, esp_hal::gpio::Output<'static>>,
            binbook_fw::board::DisplayDelay,
            binbook_fw::storage::FixedTime,
        >,
    >,
) {
    use embassy_futures::select::{select, Either};
    use esp_hal::usb_serial_jtag::UsbSerialJtag;

    let usb = UsbSerialJtag::new(usb_device).into_async();
    let (mut usb_rx, mut usb_tx) = usb.split();
    let mut serial = SerialState::new();
    let mut crash_store = CrashStore::new(esp_storage::FlashStorage::new(flash));
    let mut storage = binbook_fw::diag_storage::FilesystemStorage::new(sd_fs);
    let completion_rx = AGGREGATOR_COMPLETION_CHANNEL.receiver();
    let mut published_protocol_errors = 0;
    let mut transfer_trace = binbook_fw::transfer_trace_command::TransferTraceCommand::new();

    loop {
        let mut usb_read_buf = [0u8; 64];
        if !serial.needs_service(false) {
            match select(
                embedded_io_async::Read::read(&mut usb_rx, &mut usb_read_buf),
                completion_rx.receive(),
            )
            .await
            {
                Either::First(Ok(received)) => {
                    if received == 0 {
                        embassy_time::Timer::after_millis(1).await;
                    } else {
                        serial.feed_rx(&usb_read_buf[..received]);
                    }
                }
                Either::First(Err(error)) => match error {},
                Either::Second(committed) => {
                    let status = match committed.completion.status {
                        RuntimeCompletionStatus::Ok => Status::Ok,
                        RuntimeCompletionStatus::Error => Status::Error,
                    };
                    let _ = complete_pending_command(
                        &mut serial,
                        committed.pending,
                        status,
                        committed.completion.page,
                        &[],
                    );
                }
            }
        }
        loop {
            let received = usb_rx.drain_rx_fifo(&mut usb_read_buf);
            if received == 0 {
                break;
            }
            serial.feed_rx(&usb_read_buf[..received]);
        }

        while let Ok(committed) = AGGREGATOR_COMPLETION_CHANNEL.receiver().try_receive() {
            let status = match committed.completion.status {
                RuntimeCompletionStatus::Ok => Status::Ok,
                RuntimeCompletionStatus::Error => Status::Error,
            };
            let _ = complete_pending_command(
                &mut serial,
                committed.pending,
                status,
                committed.completion.page,
                &[],
            );
        }

        if serial.frame_ready() {
            let snapshot = match query_aggregator(AggregatorQuery::Status).await {
                AggregatorResponse::Status(snapshot) => snapshot,
                AggregatorResponse::Reserve(_)
                | AggregatorResponse::Log { .. }
                | AggregatorResponse::Ack => {
                    continue;
                }
            };
            if let Some(command) = poll_runtime_command(&mut serial, snapshot, &mut storage) {
                let header = command.header();
                RUNTIME_EVENT_CHANNEL
                    .sender()
                    .send(RuntimeEvent {
                        timestamp_ms: embassy_time::Instant::now().as_millis(),
                        kind: RuntimeEventKind::ProtocolCommand {
                            opcode: header.opcode as u8,
                            sequence: header.sequence,
                        },
                    })
                    .await;
                handle_command(
                    &mut serial,
                    &mut crash_store,
                    snapshot,
                    command,
                    &mut transfer_trace,
                )
                .await;
            }
        }

        if !serial.pending_tx().is_empty() {
            let pending_len = serial.pending_tx().len();
            if embedded_io_async::Write::write_all(&mut usb_tx, serial.pending_tx())
                .await
                .is_ok()
            {
                serial.consume_tx(pending_len);
            }
        }

        let protocol_errors = serial.protocol_error_count();
        if protocol_errors != published_protocol_errors {
            let _ = query_aggregator(AggregatorQuery::ProtocolErrors(protocol_errors)).await;
            published_protocol_errors = protocol_errors;
        }
    }
}

async fn handle_command<F>(
    serial: &mut SerialState,
    crash_store: &mut CrashStore<F>,
    snapshot: binbook_fw::diag::DiagnosticSnapshot,
    command: RuntimeCommand,
    transfer_trace: &mut binbook_fw::transfer_trace_command::TransferTraceCommand,
) where
    F: embedded_storage::nor_flash::NorFlash,
{
    match command {
        RuntimeCommand::Immediate { .. } => {}
        RuntimeCommand::TransferTrace { header, request } => {
            let now_us = embassy_time::Instant::now().as_micros();
            match transfer_trace.handle(request, serial.protocol_error_count(), now_us) {
                Ok(Some(payload)) => {
                    queue_runtime_response(serial, header, Status::Ok, payload.as_slice());
                }
                Ok(None) => queue_runtime_response(serial, header, Status::Ok, &[]),
                Err(status) => queue_runtime_response(serial, header, status, &[]),
            }
        }
        #[cfg(feature = "sd-storage")]
        RuntimeCommand::SdWriteTrace { header, request } => {
            handle_sd_write_trace(serial, header, request);
        }
        RuntimeCommand::LogGet {
            header,
            cursor,
            max_bytes,
        } => {
            if let AggregatorResponse::Log { payload, len } =
                query_aggregator(AggregatorQuery::LogGet { cursor, max_bytes }).await
            {
                queue_runtime_response(serial, header, Status::Ok, &payload[..len]);
            }
        }
        RuntimeCommand::LogClear { header } => {
            if let AggregatorResponse::Log { payload, len } =
                query_aggregator(AggregatorQuery::LogClear).await
            {
                queue_runtime_response(serial, header, Status::Ok, &payload[..len]);
            }
        }
        RuntimeCommand::Hardware(pending) => {
            if matches!(
                pending.action,
                PendingAction::CrashGet | PendingAction::CrashClear
            ) {
                handle_crash_command(serial, crash_store, snapshot.current_page, pending).await;
            } else {
                handle_display_command(serial, snapshot.current_page, pending).await;
            }
        }
        #[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
        RuntimeCommand::OwnerStorage { .. } => {}
        #[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
        RuntimeCommand::Bulk { .. } => {}
    }
}

#[cfg(feature = "sd-storage")]
fn handle_sd_write_trace(
    serial: &mut SerialState,
    header: binbook_diagnostic_protocol::FrameHeader,
    request: binbook_diagnostic_protocol::SdWriteTraceRequest,
) {
    use binbook_diagnostic_protocol::{
        SdWriteTraceAction, SD_WRITE_TRACE_FLAG_OVERFLOW, SD_WRITE_TRACE_RECORD_BYTES,
        SD_WRITE_TRACE_RESPONSE_HEADER_BYTES,
    };

    if matches!(
        request.action,
        SdWriteTraceAction::Clear | SdWriteTraceAction::Freeze
    ) {
        super::SD_WRITE_CAPTURE.lock(|capture| {
            let mut capture = capture.borrow_mut();
            if request.action == SdWriteTraceAction::Clear {
                capture.clear();
            } else {
                capture.freeze();
            }
        });
        queue_runtime_response(serial, header, Status::Ok, &[]);
        return;
    }

    let (record_len, capacity, overflowed, anomalies, start, record) = super::SD_WRITE_CAPTURE
        .lock(|capture| {
            let capture = capture.borrow();
            let record_len = capture.len();
            let start = usize::from(request.start_index).min(record_len);
            (
                record_len,
                capture.capacity(),
                capture.overflowed(),
                capture.anomalies(),
                start,
                capture.record(start),
            )
        });
    let count = u16::from(record.is_some());
    let mut payload = [0; SD_WRITE_TRACE_RESPONSE_HEADER_BYTES + SD_WRITE_TRACE_RECORD_BYTES];
    payload[0] = 1;
    payload[1] = if overflowed {
        SD_WRITE_TRACE_FLAG_OVERFLOW
    } else {
        0
    };
    payload[2..4].copy_from_slice(&(record_len as u16).to_le_bytes());
    payload[4..6].copy_from_slice(&(capacity as u16).to_le_bytes());
    payload[6..8].copy_from_slice(&(start as u16).to_le_bytes());
    payload[8..10].copy_from_slice(&count.to_le_bytes());
    payload[10..14].copy_from_slice(&anomalies.to_le_bytes());
    payload[14..16].fill(0);
    if let Some(record) = record {
        let bytes = &mut payload[16..48];
        bytes[0..4].copy_from_slice(&record.start_block.to_le_bytes());
        bytes[4..6].copy_from_slice(&record.blocks.to_le_bytes());
        bytes[6] = record.command;
        bytes[7] = u8::from(record.success) | (record.anomalies << 1);
        for (index, field) in [
            record.command_setup_us,
            record.data_spi_us,
            record.busy_wait_us,
            record.total_us,
            record.gap_us,
        ]
        .iter()
        .enumerate()
        {
            let offset = 8 + index * 4;
            bytes[offset..offset + 4].copy_from_slice(&field.to_le_bytes());
        }
        bytes[28] = record.purpose;
        bytes[29..32].fill(0);
    }
    let len =
        SD_WRITE_TRACE_RESPONSE_HEADER_BYTES + usize::from(count) * SD_WRITE_TRACE_RECORD_BYTES;
    queue_runtime_response(serial, header, Status::Ok, &payload[..len]);
}

async fn handle_crash_command<F>(
    serial: &mut SerialState,
    crash_store: &mut CrashStore<F>,
    current_page: u32,
    pending: binbook_fw::diag::PendingCommand,
) where
    F: embedded_storage::nor_flash::NorFlash,
{
    let mut status = Status::Ok;
    let mut payload = [0u8; 1 + CRASH_SUMMARY_BYTES];
    let mut payload_len = 0usize;
    match pending.action {
        PendingAction::CrashGet => match crash_store.read() {
            Ok(summary) => {
                payload_len = match summary {
                    Some(summary) => {
                        let mut encoded = [0u8; CRASH_SUMMARY_BYTES];
                        summary.encode(&mut encoded);
                        encode_crash_response(Some(&encoded), &mut payload).unwrap_or(0)
                    }
                    None => encode_crash_response(None, &mut payload).unwrap_or(0),
                };
            }
            Err(_) => status = Status::InternalError,
        },
        PendingAction::CrashClear => {
            if crash_store.clear().is_err() {
                status = Status::InternalError;
                RUNTIME_EVENT_CHANNEL
                    .sender()
                    .send(RuntimeEvent {
                        timestamp_ms: embassy_time::Instant::now().as_millis(),
                        kind: RuntimeEventKind::DisplayFailure {
                            error: binbook_fw::error::FirmwareError::Storage,
                            page: current_page,
                        },
                    })
                    .await;
            }
        }
        PendingAction::KeyPress { .. }
        | PendingAction::RenderTurn { .. }
        | PendingAction::RenderPage { .. }
        | PendingAction::SetRefreshMode { .. }
        | PendingAction::DisplayProbe(_) => {
            status = Status::BadRequest;
        }
    }
    let _ = complete_pending_command(
        serial,
        pending,
        status,
        current_page,
        &payload[..payload_len],
    );
}

async fn handle_display_command(
    serial: &mut SerialState,
    current_page: u32,
    pending: binbook_fw::diag::PendingCommand,
) {
    let request = match pending.action {
        PendingAction::KeyPress { button } => {
            let mode = super::DISPLAY_MODE.load(portable_atomic::Ordering::Relaxed);
            match binbook_fw::async_refresh::button_to_request(button, mode) {
                Some(DisplayRequest::MenuNext { .. }) => DisplayRequest::MenuNext {
                    completion_sequence: Some(pending.header.sequence),
                },
                Some(DisplayRequest::MenuPrev { .. }) => DisplayRequest::MenuPrev {
                    completion_sequence: Some(pending.header.sequence),
                },
                Some(DisplayRequest::MenuSelect { .. }) => DisplayRequest::MenuSelect {
                    completion_sequence: Some(pending.header.sequence),
                },
                Some(DisplayRequest::MenuBack { .. }) => DisplayRequest::MenuBack {
                    completion_sequence: Some(pending.header.sequence),
                },
                Some(DisplayRequest::Turn { turn, .. }) => DisplayRequest::Turn {
                    turn,
                    completion_sequence: Some(pending.header.sequence),
                },
                Some(request) => request,
                None => {
                    let _ =
                        complete_pending_command(serial, pending, Status::Ok, current_page, &[]);
                    return;
                }
            }
        }
        PendingAction::RenderTurn { turn } => DisplayRequest::Turn {
            turn,
            completion_sequence: Some(pending.header.sequence),
        },
        PendingAction::RenderPage { target_page } => DisplayRequest::Goto {
            page: target_page,
            completion_sequence: pending.header.sequence,
        },
        PendingAction::SetRefreshMode { mode } => DisplayRequest::SetRefreshMode {
            mode: match mode {
                binbook_diagnostic_protocol::RefreshModeCode::Balanced => {
                    binbook_fw::xteink_x4_display::events::PageRefreshMode::Balanced
                }
                binbook_diagnostic_protocol::RefreshModeCode::Fast => {
                    binbook_fw::xteink_x4_display::events::PageRefreshMode::Fast
                }
            },
            completion_sequence: pending.header.sequence,
        },
        PendingAction::DisplayProbe(probe) => DisplayRequest::Probe {
            kind: match probe {
                binbook_fw::diag::DisplayProbeKind::FullRefreshCurrent => {
                    DisplayProbeKind::FullRefreshCurrent
                }
                binbook_fw::diag::DisplayProbeKind::ClearWhite => DisplayProbeKind::ClearWhite,
                binbook_fw::diag::DisplayProbeKind::WindowCorners => {
                    DisplayProbeKind::WindowCorners
                }
                binbook_fw::diag::DisplayProbeKind::PartialWindowBusyMatrix => {
                    DisplayProbeKind::PartialWindowBusyMatrix
                }
                binbook_fw::diag::DisplayProbeKind::CustomLutFastA => {
                    DisplayProbeKind::CustomLutFastA
                }
                binbook_fw::diag::DisplayProbeKind::CustomLutFastB => {
                    DisplayProbeKind::CustomLutFastB
                }
            },
            completion_sequence: pending.header.sequence,
        },
        PendingAction::CrashGet | PendingAction::CrashClear => {
            let _ =
                complete_pending_command(serial, pending, Status::BadRequest, current_page, &[]);
            return;
        }
    };
    let enqueued = matches!(
        query_aggregator(AggregatorQuery::Enqueue { pending, request }).await,
        AggregatorResponse::Reserve(Ok(()))
    );
    if !enqueued {
        let _ = complete_pending_command(serial, pending, Status::Error, current_page, &[]);
    }
}
