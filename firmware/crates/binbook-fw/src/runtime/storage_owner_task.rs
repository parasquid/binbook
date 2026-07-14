use binbook_fw::storage_owner::{
    FilesystemOwnerBackend, OwnedResponse, StorageClient, StorageOwner, StorageOwnerPhase,
    StorageOwnerPump,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Receiver};

use super::{
    RuntimeSdFilesystem, StorageOwnedRequest, STORAGE_DIAGNOSTIC_RESPONSE_CHANNEL,
    STORAGE_LIBRARY_RESPONSE_CHANNEL, STORAGE_OWNER_PHASE, STORAGE_QUEUE_CAPACITY,
    STORAGE_READER_RESPONSE_CHANNEL, STORAGE_STATE_RESPONSE_CHANNEL,
};

type RuntimeHandle = binbook_fw::storage_access::SdHandle<'static, RuntimeSdFilesystem>;

#[embassy_executor::task]
pub(super) async fn storage_owner_task(
    filesystem: RuntimeHandle,
    requests: Receiver<
        'static,
        CriticalSectionRawMutex,
        StorageOwnedRequest,
        STORAGE_QUEUE_CAPACITY,
    >,
) {
    let owner = StorageOwner::new(FilesystemOwnerBackend::new(filesystem));
    let mut pump = StorageOwnerPump::<_, STORAGE_QUEUE_CAPACITY>::new(owner);

    loop {
        let first = requests.receive().await;
        pump.enqueue(first, record_phase)
            .unwrap_or_else(|_| unreachable!("empty owner mailbox rejected first request"));
        while let Ok(request) = requests.try_receive() {
            if let Err(request) = pump.enqueue(request, record_phase) {
                route(pump.execute_immediate(request, record_phase)).await;
            }
        }
        while let Some(response) = pump.execute_next(record_phase) {
            route(response).await;
            while let Ok(request) = requests.try_receive() {
                if let Err(_request) = pump.enqueue(request, record_phase) {
                    unreachable!("owner mailbox cannot overflow after one completion");
                }
            }
        }
    }
}

#[cfg(feature = "mounted-storage-owner")]
#[embassy_executor::task]
pub(super) async fn mounted_storage_owner_task(
    filesystem: RuntimeSdFilesystem,
    requests: Receiver<
        'static,
        CriticalSectionRawMutex,
        StorageOwnedRequest,
        STORAGE_QUEUE_CAPACITY,
    >,
) {
    let storage = match filesystem.into_owned_mount() {
        Ok(storage) => storage,
        Err(_) => {
            super::RUNTIME_EVENT_CHANNEL
                .send(binbook_fw::runtime_engine::RuntimeEvent {
                    timestamp_ms: embassy_time::Instant::now().as_millis(),
                    kind: binbook_fw::runtime_engine::RuntimeEventKind::StorageOwnerProbe {
                        phase: 20,
                        status: 1,
                        elapsed_ms: 0,
                    },
                })
                .await;
            loop {
                let request = requests.receive().await;
                route(binbook_fw::storage_owner::reject_owned_request(
                    request,
                    binbook_fw::storage_owner::CompletionError::Backend,
                ))
                .await;
            }
        }
    };
    super::RUNTIME_EVENT_CHANNEL
        .send(binbook_fw::runtime_engine::RuntimeEvent {
            timestamp_ms: embassy_time::Instant::now().as_millis(),
            kind: binbook_fw::runtime_engine::RuntimeEventKind::StorageOwnerProbe {
                phase: 20,
                status: 0,
                elapsed_ms: 0,
            },
        })
        .await;
    let owner = StorageOwner::new(binbook_fw::storage_owner::MountedStorageOwnerBackend::new(
        &storage,
    ));
    let mut pump = StorageOwnerPump::<_, STORAGE_QUEUE_CAPACITY>::new(owner);

    let mut trace_first = true;
    loop {
        let first = requests.receive().await;
        if trace_first {
            super::RUNTIME_EVENT_CHANNEL
                .send(binbook_fw::runtime_engine::RuntimeEvent {
                    timestamp_ms: embassy_time::Instant::now().as_millis(),
                    kind: binbook_fw::runtime_engine::RuntimeEventKind::StorageOwnerProbe {
                        phase: 21,
                        status: first.envelope.client as u8,
                        elapsed_ms: first.envelope.sequence,
                    },
                })
                .await;
        }
        pump.enqueue(first, record_phase)
            .unwrap_or_else(|_| unreachable!("empty owner mailbox rejected first request"));
        while let Ok(request) = requests.try_receive() {
            if let Err(request) = pump.enqueue(request, record_phase) {
                route(pump.execute_immediate(request, record_phase)).await;
            }
        }
        while let Some(response) = pump.execute_next(record_phase) {
            if trace_first {
                super::RUNTIME_EVENT_CHANNEL
                    .send(binbook_fw::runtime_engine::RuntimeEvent {
                        timestamp_ms: embassy_time::Instant::now().as_millis(),
                        kind: binbook_fw::runtime_engine::RuntimeEventKind::StorageOwnerProbe {
                            phase: 22,
                            status: u8::from(response.envelope.response.is_err()),
                            elapsed_ms: response.envelope.sequence,
                        },
                    })
                    .await;
                trace_first = false;
            }
            route(response).await;
            while let Ok(request) = requests.try_receive() {
                if let Err(_request) = pump.enqueue(request, record_phase) {
                    unreachable!("owner mailbox cannot overflow after one completion");
                }
            }
        }
    }
}

fn record_phase(phase: StorageOwnerPhase) {
    let (kind, client, sequence) = match phase {
        StorageOwnerPhase::Queued { client, sequence } => (1, client, sequence),
        StorageOwnerPhase::Executing { client, sequence } => (2, client, sequence),
        StorageOwnerPhase::Completed { client, sequence } => (3, client, sequence),
    };
    let client = match client {
        StorageClient::Reader => 0,
        StorageClient::State => 1,
        StorageClient::Diagnostic => 2,
        StorageClient::Library => 3,
    };
    STORAGE_OWNER_PHASE.store(
        (kind << 30) | (client << 28) | (sequence & 0x0fff_ffff),
        portable_atomic::Ordering::Relaxed,
    );
}

async fn route(response: OwnedResponse<'static>) {
    match response.envelope.client {
        StorageClient::Reader => STORAGE_READER_RESPONSE_CHANNEL.send(response).await,
        StorageClient::State => STORAGE_STATE_RESPONSE_CHANNEL.send(response).await,
        StorageClient::Diagnostic => STORAGE_DIAGNOSTIC_RESPONSE_CHANNEL.send(response).await,
        StorageClient::Library => STORAGE_LIBRARY_RESPONSE_CHANNEL.send(response).await,
    }
}

#[cfg(feature = "storage-owner-probe")]
#[embassy_executor::task]
pub(super) async fn storage_owner_probe_task(name: binbook_fw::storage_owner::StorageName) {
    use binbook_fw::{
        runtime_engine::{RuntimeEvent, RuntimeEventKind},
        storage_owner::{
            BufferLease, BufferSlot, OwnedRequest, Request, RequestEnvelope, Response,
        },
    };
    use static_cell::StaticCell;

    static BUFFER: StaticCell<[u8; binbook_fw::storage_owner::MAX_BOUNDED_IO_BYTES as usize]> =
        StaticCell::new();
    let buffer = BUFFER.init([0; binbook_fw::storage_owner::MAX_BOUNDED_IO_BYTES as usize]);
    super::RUNTIME_EVENT_CHANNEL
        .send(RuntimeEvent {
            timestamp_ms: embassy_time::Instant::now().as_millis(),
            kind: RuntimeEventKind::StorageOwnerProbe {
                phase: 0,
                status: 0,
                elapsed_ms: 0,
            },
        })
        .await;
    embassy_time::Timer::after_secs(5).await;
    let started = embassy_time::Instant::now().as_millis();
    super::RUNTIME_EVENT_CHANNEL
        .send(RuntimeEvent {
            timestamp_ms: started,
            kind: RuntimeEventKind::StorageOwnerProbe {
                phase: 1,
                status: 0,
                elapsed_ms: 0,
            },
        })
        .await;
    super::STORAGE_REQUEST_CHANNEL
        .send(OwnedRequest {
            envelope: RequestEnvelope {
                client: StorageClient::Diagnostic,
                sequence: u32::MAX,
                request: Request::read(
                    name,
                    0,
                    binbook_fw::storage_owner::MAX_BOUNDED_IO_BYTES,
                    BufferSlot::Diagnostic,
                )
                .expect("probe read is bounded"),
            },
            buffer: Some(BufferLease::new(BufferSlot::Diagnostic, buffer)),
        })
        .await;
    let response = super::STORAGE_DIAGNOSTIC_RESPONSE_CHANNEL.receive().await;
    let status = if matches!(response.envelope.response, Ok(Response::Read(4096))) {
        0
    } else {
        1
    };
    let ended = embassy_time::Instant::now().as_millis();
    super::RUNTIME_EVENT_CHANNEL
        .send(RuntimeEvent {
            timestamp_ms: ended,
            kind: RuntimeEventKind::StorageOwnerProbe {
                phase: 3,
                status,
                elapsed_ms: ended.saturating_sub(started).min(u32::MAX as u64) as u32,
            },
        })
        .await;
}

#[cfg(feature = "mounted-storage-owner")]
#[embassy_executor::task]
pub(super) async fn mounted_scan_probe_task() {
    use binbook_fw::{
        runtime_engine::{RuntimeEvent, RuntimeEventKind},
        storage_owner::{
            BufferLease, BufferSlot, OwnedRequest, Request, RequestEnvelope, Response,
            SCAN_BATCH_BYTES,
        },
    };
    use static_cell::StaticCell;

    static BUFFER: StaticCell<[u8; SCAN_BATCH_BYTES]> = StaticCell::new();
    let mut buffer: &'static mut [u8] = BUFFER.init([0; SCAN_BATCH_BYTES]);
    embassy_time::Timer::after_secs(2).await;
    let started = embassy_time::Instant::now().as_millis();
    super::RUNTIME_EVENT_CHANNEL
        .send(RuntimeEvent {
            timestamp_ms: started,
            kind: RuntimeEventKind::StorageOwnerProbe {
                phase: 0,
                status: 0,
                elapsed_ms: 0,
            },
        })
        .await;
    super::STORAGE_REQUEST_CHANNEL
        .send(OwnedRequest {
            envelope: RequestEnvelope {
                client: StorageClient::Library,
                sequence: 1,
                request: Request::ScanStart,
            },
            buffer: None,
        })
        .await;
    let response = super::STORAGE_LIBRARY_RESPONSE_CHANNEL.receive().await;
    let Ok(Response::ScanStarted(token)) = response.envelope.response else {
        return;
    };
    let mut sequence = 2u32;
    let mut total = 0u8;
    loop {
        super::STORAGE_REQUEST_CHANNEL
            .send(OwnedRequest {
                envelope: RequestEnvelope {
                    client: StorageClient::Library,
                    sequence,
                    request: Request::scan_step(token),
                },
                buffer: Some(BufferLease::new(BufferSlot::LibraryIndex, buffer)),
            })
            .await;
        let response = super::STORAGE_LIBRARY_RESPONSE_CHANNEL.receive().await;
        buffer = response
            .buffer
            .expect("mounted scan owner must return its caller buffer")
            .into_bytes();
        let Ok(Response::ScanProgress(progress)) = response.envelope.response else {
            return;
        };
        total = total.saturating_add(progress.entries);
        let now = embassy_time::Instant::now().as_millis();
        super::RUNTIME_EVENT_CHANNEL
            .send(RuntimeEvent {
                timestamp_ms: now,
                kind: RuntimeEventKind::StorageOwnerProbe {
                    phase: 2,
                    status: total,
                    elapsed_ms: now.saturating_sub(started).min(u32::MAX as u64) as u32,
                },
            })
            .await;
        sequence = sequence.wrapping_add(1);
        if progress.complete {
            break;
        }
        embassy_time::Timer::after_millis(10).await;
    }
    super::STORAGE_REQUEST_CHANNEL
        .send(OwnedRequest {
            envelope: RequestEnvelope {
                client: StorageClient::Library,
                sequence,
                request: Request::ScanFinish { token },
            },
            buffer: None,
        })
        .await;
    let response = super::STORAGE_LIBRARY_RESPONSE_CHANNEL.receive().await;
    let status = if matches!(response.envelope.response, Ok(Response::Ack)) {
        total
    } else {
        u8::MAX
    };
    let ended = embassy_time::Instant::now().as_millis();
    super::RUNTIME_EVENT_CHANNEL
        .send(RuntimeEvent {
            timestamp_ms: ended,
            kind: RuntimeEventKind::StorageOwnerProbe {
                phase: 3,
                status,
                elapsed_ms: ended.saturating_sub(started).min(u32::MAX as u64) as u32,
            },
        })
        .await;
}

#[cfg(feature = "mounted-storage-owner")]
#[embassy_executor::task]
pub(super) async fn mounted_state_probe_task() {
    use binbook_fw::{
        reader_state::{ReaderState, MAX_STATE_BYTES},
        runtime_engine::{RuntimeEvent, RuntimeEventKind},
        state_owner::{OwnerStateSlots, StateLoadUpdate, StateSaveUpdate},
        storage_owner::{BufferLease, BufferSlot, OwnedRequest, Request},
    };
    use static_cell::StaticCell;

    static STATE: StaticCell<ReaderState> = StaticCell::new();
    static BUFFER: StaticCell<[u8; MAX_STATE_BYTES]> = StaticCell::new();
    let state = STATE.init(ReaderState::empty());
    let mut buffer: Option<&'static mut [u8]> = Some(BUFFER.init([0; MAX_STATE_BYTES]));
    let mut slots = OwnerStateSlots::new();
    // Keep the state persistence probe off the cache-to-book-open critical path.
    // Production will sequence these operations from the reader lifecycle.
    embassy_time::Timer::after_secs(20).await;
    let started = embassy_time::Instant::now().as_millis();
    let (mut load, mut request) = slots.begin_load(state, 20_000);
    let warning = loop {
        let needs_buffer = matches!(request.request, Request::Read { .. });
        let lease = needs_buffer.then(|| {
            BufferLease::new(
                BufferSlot::State,
                buffer.take().expect("state buffer must be returned"),
            )
        });
        super::STORAGE_REQUEST_CHANNEL
            .send(OwnedRequest {
                envelope: request,
                buffer: lease,
            })
            .await;
        let response = super::STORAGE_STATE_RESPONSE_CHANNEL.receive().await;
        if let Some(lease) = response.buffer {
            buffer = Some(lease.into_bytes());
        }
        match load.accept(
            response.envelope,
            buffer.as_deref_mut().expect("state buffer must be local"),
        ) {
            Ok(StateLoadUpdate::Request(next)) => request = next,
            Ok(StateLoadUpdate::Complete { warning }) => break warning,
            Err(_) => break true,
        }
    };
    drop(load);
    let loaded = embassy_time::Instant::now().as_millis();
    super::RUNTIME_EVENT_CHANNEL
        .send(RuntimeEvent {
            timestamp_ms: loaded,
            kind: RuntimeEventKind::StorageOwnerProbe {
                phase: 10,
                status: u8::from(warning),
                elapsed_ms: loaded.saturating_sub(started).min(u32::MAX as u64) as u32,
            },
        })
        .await;

    let (mut save, mut request) = match slots.begin_save(
        state,
        buffer.as_deref_mut().expect("state buffer must be local"),
        21_000,
    ) {
        Ok(value) => value,
        Err(_) => return,
    };
    let save_status = loop {
        let needs_buffer = matches!(
            request.request,
            Request::UploadWrite { .. } | Request::Read { .. }
        );
        let continuing_write = matches!(request.request, Request::UploadWrite { .. });
        let lease = needs_buffer.then(|| {
            BufferLease::new(
                BufferSlot::State,
                buffer.take().expect("state buffer must be returned"),
            )
        });
        super::STORAGE_REQUEST_CHANNEL
            .send(OwnedRequest {
                envelope: request,
                buffer: lease,
            })
            .await;
        let response = super::STORAGE_STATE_RESPONSE_CHANNEL.receive().await;
        if let Some(lease) = response.buffer {
            buffer = Some(lease.into_bytes());
        }
        match save.accept(
            response.envelope,
            buffer.as_deref_mut().expect("state buffer must be local"),
        ) {
            Ok(StateSaveUpdate::Request(next)) => {
                request = next;
                if continuing_write {
                    embassy_time::Timer::after_millis(10).await;
                }
            }
            Ok(StateSaveUpdate::Complete) => break 0,
            Err(_) => break 1,
        }
    };
    drop(save);
    let ended = embassy_time::Instant::now().as_millis();
    super::RUNTIME_EVENT_CHANNEL
        .send(RuntimeEvent {
            timestamp_ms: ended,
            kind: RuntimeEventKind::StorageOwnerProbe {
                phase: 11,
                status: save_status,
                elapsed_ms: ended.saturating_sub(loaded).min(u32::MAX as u64) as u32,
            },
        })
        .await;
}
