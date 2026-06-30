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
    use esp_hal::usb_serial_jtag::UsbSerialJtag;

    let usb = UsbSerialJtag::new(usb_device);
    let (mut usb_rx, mut usb_tx) = usb.split();
    let mut serial = SerialState::new();
    let mut crash_store = CrashStore::new(esp_storage::FlashStorage::new(flash));

    loop {
        let mut usb_read_buf = [0u8; 64];
        let received = usb_rx.drain_rx_fifo(&mut usb_read_buf);
        if received > 0 {
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

        let snapshot = match query_aggregator(AggregatorQuery::Status).await {
            AggregatorResponse::Status(snapshot) => snapshot,
            AggregatorResponse::Reserve(_)
            | AggregatorResponse::Log { .. }
            | AggregatorResponse::Ack => {
                continue;
            }
        };
        if let Some(command) = poll_runtime_command(&mut serial, snapshot) {
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

        if !serial.pending_tx().is_empty() {
            let pending_len = serial.pending_tx().len();
            if usb_tx.write(serial.pending_tx()).is_ok() {
                serial.consume_tx(pending_len);
            }
        }

        let _ = query_aggregator(AggregatorQuery::ProtocolErrors(
            serial.protocol_error_count(),
        ))
        .await;
        embassy_time::Timer::after_millis(10).await;
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
        RuntimeCommand::Immediate {
            header,
            status,
            payload,
            payload_len,
        } => queue_runtime_response(serial, header, status, &payload[..payload_len]),
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
        PendingAction::RenderTurn { .. }
        | PendingAction::RenderPage { .. }
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
        PendingAction::RenderTurn { turn } => DisplayRequest::Turn {
            turn,
            completion_sequence: Some(pending.header.sequence),
        },
        PendingAction::RenderPage { target_page } => DisplayRequest::Goto {
            page: target_page,
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
