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
    use embassy_futures::select::{select, Either};
    use esp_hal::usb_serial_jtag::UsbSerialJtag;

    let usb = UsbSerialJtag::new(usb_device).into_async();
    let (mut usb_rx, mut usb_tx) = usb.split();
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
    let completion_rx = AGGREGATOR_COMPLETION_CHANNEL.receiver();
    let mut published_protocol_errors = 0;

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
                    serial.feed_rx(&usb_read_buf[..received]);
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
            #[cfg(feature = "sd-storage")]
            let command = binbook_fw::diag::poll_runtime_command_owner_storage(
                &mut serial,
                snapshot,
                &mut storage,
                owner_transfer,
            );
            #[cfg(not(feature = "sd-storage"))]
            let command = poll_runtime_command(&mut serial, snapshot, &mut storage);
            if let Some(command) = command {
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
                #[cfg(feature = "sd-storage")]
                match command {
                    RuntimeCommand::OwnerStorage { header, command } => {
                        owner_transfer =
                            super::diagnostic_owner_storage::handle_owner_storage_command(
                                &mut serial,
                                &mut owner_runtime,
                                header,
                                command,
                                owner_transfer,
                                owner_response,
                            )
                            .await;
                    }
                    command => {
                        handle_command(&mut serial, &mut crash_store, snapshot, command).await;
                    }
                }
                #[cfg(not(feature = "sd-storage"))]
                handle_command(&mut serial, &mut crash_store, snapshot, command).await;
            }
        }

        if !serial.pending_tx().is_empty() {
            let pending_len = serial.pending_tx().len();
            if embedded_io_async::Write::write_all(&mut usb_tx, serial.pending_tx())
                .await
                .is_ok()
                && embedded_io_async::Write::flush(&mut usb_tx).await.is_ok()
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
                    serial.feed_rx(&usb_read_buf[..received]);
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
                handle_command(&mut serial, &mut crash_store, snapshot, command).await;
            }
        }

        if !serial.pending_tx().is_empty() {
            let pending_len = serial.pending_tx().len();
            if embedded_io_async::Write::write_all(&mut usb_tx, serial.pending_tx())
                .await
                .is_ok()
                && embedded_io_async::Write::flush(&mut usb_tx).await.is_ok()
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
) where
    F: embedded_storage::nor_flash::NorFlash,
{
    match command {
        RuntimeCommand::Immediate { .. } => {}
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
    }
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
