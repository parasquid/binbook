use portable_atomic::Ordering;

use binbook_diagnostic_protocol::{
    encode_log_response_header, LogResponseHeader, MAX_PAYLOAD_BYTES,
};
use binbook_fw::{
    async_refresh::PAGE_TURN_QUEUE_CAPACITY,
    diag::{resolve_log_get, DiagnosticSnapshot},
    runtime_aggregator::RuntimeAggregator,
    runtime_engine::{RuntimeEvent, RuntimeEventKind},
};

use crate::{BINBOOK_SCRATCH_BYTES, PROBE_BOOK};

use super::{
    AggregatorQuery, AggregatorResponse, AGGREGATOR_COMPLETION_CHANNEL, AGGREGATOR_QUERY_CHANNEL,
    AGGREGATOR_RESPONSE_CHANNEL, REQUEST_CHANNEL, REQUEST_EPOCH, RUNTIME_EVENT_CHANNEL,
};

#[embassy_executor::task]
pub(super) async fn runtime_event_aggregator_task() {
    use embassy_futures::select::{select, Either};

    let mut scratch = [0u8; BINBOOK_SCRATCH_BYTES];
    let book = binbook_core::Book::open(binbook_core::SliceSource::new(PROBE_BOOK), &mut scratch)
        .expect("failed to open embedded BinBook for diagnostics");
    let mut aggregator = RuntimeAggregator::<
        { PAGE_TURN_QUEUE_CAPACITY },
        { binbook_fw::diag_log::DEFAULT_LOG_CAPACITY },
    >::new(DiagnosticSnapshot {
        current_page: 0,
        page_count: book.page_count(),
        panel_mode: binbook_diagnostic_protocol::PanelModeCode::Unknown,
        dropped_log_count: 0,
        protocol_error_count: 0,
        last_error: 0,
    });
    aggregator.commit(RuntimeEvent {
        timestamp_ms: embassy_time::Instant::now().as_millis(),
        kind: RuntimeEventKind::FirmwareStarted {
            page_count: book.page_count(),
        },
    });
    let event_rx = RUNTIME_EVENT_CHANNEL.receiver();
    let query_rx = AGGREGATOR_QUERY_CHANNEL.receiver();
    let response_tx = AGGREGATOR_RESPONSE_CHANNEL.sender();
    let completion_tx = AGGREGATOR_COMPLETION_CHANNEL.sender();

    loop {
        match select(event_rx.receive(), query_rx.receive()).await {
            Either::First(event) => {
                if let Some(completion) = aggregator.commit(event) {
                    completion_tx.send(completion).await;
                }
            }
            Either::Second(query) => {
                let response = match query {
                    AggregatorQuery::Enqueue { pending, request } => {
                        AggregatorResponse::Reserve(aggregator.reserve_and_enqueue(pending, || {
                            if REQUEST_CHANNEL.sender().try_send(request).is_ok() {
                                REQUEST_EPOCH.fetch_add(1, Ordering::AcqRel);
                                true
                            } else {
                                false
                            }
                        }))
                    }
                    AggregatorQuery::Status => AggregatorResponse::Status(aggregator.snapshot()),
                    AggregatorQuery::LogGet { cursor, max_bytes } => {
                        let mut payload = [0u8; MAX_PAYLOAD_BYTES];
                        let len =
                            resolve_log_get(aggregator.log(), cursor, max_bytes, &mut payload);
                        AggregatorResponse::Log { payload, len }
                    }
                    AggregatorQuery::LogClear => {
                        let next_cursor = aggregator.clear_log();
                        let mut payload = [0u8; MAX_PAYLOAD_BYTES];
                        let len = encode_log_response_header(
                            LogResponseHeader {
                                next_cursor,
                                dropped_log_count: 0,
                                record_count: 0,
                            },
                            &mut payload,
                        )
                        .unwrap_or(0);
                        AggregatorResponse::Log { payload, len }
                    }
                    AggregatorQuery::ProtocolErrors(count) => {
                        aggregator.set_protocol_error_count(count);
                        AggregatorResponse::Ack
                    }
                };
                response_tx.send(response).await;
            }
        }
    }
}

pub(super) async fn query_aggregator(query: AggregatorQuery) -> AggregatorResponse {
    AGGREGATOR_QUERY_CHANNEL.sender().send(query).await;
    AGGREGATOR_RESPONSE_CHANNEL.receiver().receive().await
}
