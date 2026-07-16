use binbook_fw::{
    library::{LibraryIndexSlot, LibraryStatus},
    library_external_sort::LibraryRefreshWorkspace,
    library_index_select::{LibraryIndexSelectClient, LibraryIndexSelectUpdate},
    library_publication::{LibraryPublicationClient, LibraryPublicationUpdate},
    library_run_generation::{LibraryRunGenerationClient, LibraryRunGenerationUpdate},
    storage_owner::{BufferLease, BufferSlot, OwnedRequest, Request, MAX_BOUNDED_IO_BYTES},
};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Receiver, Sender},
};
use static_cell::StaticCell;

use super::{
    LibraryRefreshCompletion, LibraryRefreshTrigger, STORAGE_LIBRARY_RESPONSE_CHANNEL,
    STORAGE_REQUEST_CHANNEL,
};
#[cfg(feature = "mounted-storage-owner")]
use super::{
    LIBRARY_REFRESH_COMPLETION_CHANNEL, LIBRARY_REFRESH_TRIGGER_CHANNEL, RUNTIME_EVENT_CHANNEL,
};

const PROBE_PUBLICATION_REQUEST: u8 = 44;
const PROBE_PUBLICATION_FAILURE: u8 = 45;
const PROBE_PUBLICATION_COMPLETE: u8 = 46;

#[embassy_executor::task]
pub(super) async fn library_refresh_task(
    workspace: &'static mut LibraryRefreshWorkspace,
    trigger: Receiver<'static, CriticalSectionRawMutex, LibraryRefreshTrigger, 1>,
    completion: Sender<'static, CriticalSectionRawMutex, LibraryRefreshCompletion, 1>,
) {
    static BUFFER: StaticCell<[u8; MAX_BOUNDED_IO_BYTES as usize]> = StaticCell::new();
    let mut buffer: Option<&'static mut [u8]> =
        Some(BUFFER.init([0; MAX_BOUNDED_IO_BYTES as usize]));
    let mut sequence = 1u32;
    probe_refresh_phase(40, 0, 0);
    loop {
        let trigger = trigger.receive().await;
        probe_refresh_phase(41, trigger.prefetch.len() as u8, sequence);
        let started = embassy_time::Instant::now().as_millis();
        let (mut generation, mut request) =
            LibraryRunGenerationClient::begin(workspace, &trigger.prefetch, sequence)
                .expect("fresh workspace and bounded prefetch names");
        let status = loop {
            probe_refresh_phase(42, refresh_request_code(&request.request), sequence);
            let needs_buffer = request_needs_buffer(&request.request);
            let response = execute_library_request(request, needs_buffer, &mut buffer).await;
            let bytes = needs_buffer.then(|| buffer.as_deref_mut().unwrap());
            match generation.accept(response, bytes) {
                Ok(LibraryRunGenerationUpdate::Request(next)) => {
                    let should_yield = matches!(
                        next.request,
                        Request::ScanStep { .. } | Request::LibraryScratchWrite { .. }
                    );
                    request = next;
                    if should_yield {
                        embassy_time::Timer::after_millis(10).await;
                    }
                }
                Ok(LibraryRunGenerationUpdate::Complete(status)) => break status,
                Err(_) => {
                    let mut cleanup = generation.cleanup(LibraryStatus::CardError);
                    let mut cleaned_status = LibraryStatus::CardError;
                    while let Some(cleanup_request) = cleanup {
                        let needs_buffer = request_needs_buffer(&cleanup_request.request);
                        let response =
                            execute_library_request(cleanup_request, needs_buffer, &mut buffer)
                                .await;
                        let bytes = needs_buffer.then(|| buffer.as_deref_mut().unwrap());
                        match generation.accept(response, bytes) {
                            Ok(LibraryRunGenerationUpdate::Request(next)) => cleanup = Some(next),
                            Ok(LibraryRunGenerationUpdate::Complete(value)) => {
                                cleaned_status = value;
                                cleanup = None;
                            }
                            Err(_) => cleanup = None,
                        }
                    }
                    break cleaned_status;
                }
            }
        };
        drop(generation);
        probe_refresh_phase(43, status.code(), workspace.entry_count() as u32);

        let mut publication = None;
        if matches!(status, LibraryStatus::Ready | LibraryStatus::NoBooks) {
            let (mut selector, mut request) =
                LibraryIndexSelectClient::begin(sequence.wrapping_add(1_000));
            let selected = loop {
                let needs_buffer = request_needs_buffer(&request.request);
                let response = execute_library_request(request, needs_buffer, &mut buffer).await;
                let bytes = needs_buffer.then(|| &buffer.as_deref().unwrap()[..]);
                match selector.accept(response, bytes) {
                    Ok(LibraryIndexSelectUpdate::Request(next)) => request = next,
                    Ok(LibraryIndexSelectUpdate::Complete(cursor)) => break cursor,
                    Err(_) => break None,
                }
            };
            drop(selector);
            let retained_generation = selected.map_or(0, |cursor| cursor.generation());
            let (slot, generation_number) = match selected {
                Some(cursor) => {
                    let slot = if cursor.slot() == LibraryIndexSlot::A {
                        LibraryIndexSlot::B
                    } else {
                        LibraryIndexSlot::A
                    };
                    (slot, cursor.generation().wrapping_add(1).max(1))
                }
                None => (LibraryIndexSlot::A, 1),
            };
            match LibraryPublicationClient::begin(
                workspace,
                slot,
                generation_number,
                sequence.wrapping_add(2_000),
            ) {
                Ok((mut publisher, mut request)) => loop {
                    let phase = publisher.phase();
                    probe_refresh_phase(
                        PROBE_PUBLICATION_REQUEST,
                        phase.code(),
                        u32::from(refresh_request_code(&request.request)),
                    );
                    let needs_buffer = request_needs_buffer(&request.request);
                    let response =
                        execute_library_request(request, needs_buffer, &mut buffer).await;
                    let bytes = needs_buffer.then(|| buffer.as_deref_mut().unwrap());
                    match publisher.accept(response, bytes) {
                        Ok(LibraryPublicationUpdate::Request(next)) => {
                            let should_yield = matches!(
                                next.request,
                                Request::LibraryScratchRead { .. }
                                    | Request::UploadWrite { .. }
                                    | Request::Read { .. }
                            );
                            request = next;
                            if should_yield {
                                embassy_time::Timer::after_millis(10).await;
                            }
                        }
                        Ok(LibraryPublicationUpdate::Complete(value)) => {
                            probe_refresh_phase(
                                PROBE_PUBLICATION_COMPLETE,
                                value.status.code(),
                                u32::from(value.book_count),
                            );
                            catalog_publication_event(
                                0,
                                publisher.phase().code(),
                                value.cursor.map_or(0, |cursor| cursor.generation()),
                            );
                            publication = Some(value);
                            break;
                        }
                        Err(error) => {
                            probe_refresh_phase(
                                PROBE_PUBLICATION_FAILURE,
                                phase.code(),
                                error.code(),
                            );
                            catalog_publication_event(1, phase.code(), retained_generation);
                            let mut cleanup = publisher.cleanup();
                            while let Some(cleanup_request) = cleanup {
                                let needs_buffer = request_needs_buffer(&cleanup_request.request);
                                let response = execute_library_request(
                                    cleanup_request,
                                    needs_buffer,
                                    &mut buffer,
                                )
                                .await;
                                let bytes = needs_buffer.then(|| buffer.as_deref_mut().unwrap());
                                cleanup = match publisher.accept(response, bytes) {
                                    Ok(LibraryPublicationUpdate::Request(next)) => Some(next),
                                    _ => None,
                                };
                            }
                            break;
                        }
                    }
                },
                Err(error) => {
                    probe_refresh_phase(PROBE_PUBLICATION_FAILURE, 0, error.code());
                    catalog_publication_event(1, 0, retained_generation);
                }
            }
        }
        sequence = sequence.wrapping_add(64).max(1);
        let ended = embassy_time::Instant::now().as_millis();
        let published_status = publication.as_ref().map_or_else(
            || {
                if matches!(status, LibraryStatus::Ready | LibraryStatus::NoBooks) {
                    LibraryStatus::CardError
                } else {
                    status
                }
            },
            |value| value.status,
        );
        completion
            .send(LibraryRefreshCompletion {
                status: published_status.code(),
                book_count: publication.as_ref().map_or(0, |value| value.book_count),
                elapsed_ms: ended.saturating_sub(started).min(u32::MAX as u64) as u32,
                cursor: publication.as_ref().and_then(|value| value.cursor),
                prepared: publication.map_or_else(heapless::Vec::new, |value| value.prepared),
            })
            .await;
        #[cfg(feature = "diagnostic-console")]
        if let Some(trace_id) = trigger.trace_id {
            if super::TRANSFER_LIBRARY_TIMING_CHANNEL
                .sender()
                .try_send(super::TransferLibraryTiming {
                    trace_id,
                    elapsed_us: ended
                        .saturating_sub(started)
                        .saturating_mul(1_000)
                        .min(u64::from(u32::MAX)) as u32,
                })
                .is_err()
            {
                super::TRANSFER_OWNER_TIMING_DROPS.fetch_add(1, portable_atomic::Ordering::Relaxed);
            }
        }
    }
}

#[cfg(feature = "diagnostic-console")]
fn probe_refresh_phase(phase: u8, status: u8, detail: u32) {
    use binbook_fw::runtime_engine::{RuntimeEvent, RuntimeEventKind};

    let _ = RUNTIME_EVENT_CHANNEL.try_send(RuntimeEvent {
        timestamp_ms: embassy_time::Instant::now().as_millis(),
        kind: RuntimeEventKind::StorageOwnerProbe {
            phase,
            status,
            elapsed_ms: detail,
        },
    });
}

#[cfg(not(feature = "diagnostic-console"))]
fn probe_refresh_phase(_phase: u8, _status: u8, _detail: u32) {}

#[cfg(feature = "diagnostic-console")]
fn catalog_publication_event(outcome: u8, phase: u8, generation: u32) {
    use binbook_fw::runtime_engine::{RuntimeEvent, RuntimeEventKind};

    let _ = RUNTIME_EVENT_CHANNEL.try_send(RuntimeEvent {
        timestamp_ms: embassy_time::Instant::now().as_millis(),
        kind: RuntimeEventKind::LibraryPublication {
            outcome,
            phase,
            generation,
        },
    });
}

#[cfg(not(feature = "diagnostic-console"))]
fn catalog_publication_event(_outcome: u8, _phase: u8, _generation: u32) {}

fn request_needs_buffer(request: &Request) -> bool {
    LibraryPublicationClient::request_needs_buffer(request)
        || matches!(
            request,
            Request::ScanStep { .. } | Request::LibraryScratchWrite { .. }
        )
}

async fn execute_library_request(
    request: binbook_fw::storage_owner::RequestEnvelope,
    needs_buffer: bool,
    buffer: &mut Option<&'static mut [u8]>,
) -> binbook_fw::storage_owner::ResponseEnvelope {
    let lease = needs_buffer.then(|| {
        BufferLease::new(
            BufferSlot::LibraryIndex,
            buffer.take().expect("library buffer must be returned"),
        )
    });
    STORAGE_REQUEST_CHANNEL
        .send(OwnedRequest {
            envelope: request,
            buffer: lease,
        })
        .await;
    let response = STORAGE_LIBRARY_RESPONSE_CHANNEL.receive().await;
    if let Some(lease) = response.buffer {
        *buffer = Some(lease.into_bytes());
    }
    response.envelope
}

#[cfg(feature = "mounted-storage-owner")]
async fn probe_index_save_failure(request_code: u8, error_code: u32) {
    use binbook_fw::runtime_engine::{RuntimeEvent, RuntimeEventKind};

    RUNTIME_EVENT_CHANNEL
        .send(RuntimeEvent {
            timestamp_ms: embassy_time::Instant::now().as_millis(),
            kind: RuntimeEventKind::StorageOwnerProbe {
                phase: 38,
                status: request_code,
                elapsed_ms: error_code,
            },
        })
        .await;
}

#[cfg(feature = "mounted-storage-owner")]
async fn probe_refresh_failure(
    request_code: u8,
    error: binbook_fw::storage_owner::CompletionError,
) {
    use binbook_fw::runtime_engine::{RuntimeEvent, RuntimeEventKind};

    let error_code = match error {
        binbook_fw::storage_owner::CompletionError::Request(_) => 1,
        binbook_fw::storage_owner::CompletionError::Backend => 2,
        binbook_fw::storage_owner::CompletionError::BufferUnavailable => 3,
    };
    RUNTIME_EVENT_CHANNEL
        .send(RuntimeEvent {
            timestamp_ms: embassy_time::Instant::now().as_millis(),
            kind: RuntimeEventKind::StorageOwnerProbe {
                phase: 37,
                status: request_code,
                elapsed_ms: error_code,
            },
        })
        .await;
}

#[cfg(feature = "mounted-storage-owner")]
const fn refresh_request_code(request: &Request) -> u8 {
    match request {
        Request::ScanStart => 1,
        Request::ScanPrefetch { .. } => 2,
        Request::ScanStep { .. } => 3,
        Request::ScanFinish { .. } => 4,
        Request::ScanCancel { .. } => 5,
        Request::UploadBegin { .. } => 6,
        Request::UploadWrite { .. } => 7,
        Request::UploadCommit { .. } => 8,
        Request::UploadAbort { .. } => 9,
        _ => 0,
    }
}

#[embassy_executor::task]
pub(super) async fn library_boot_probe_task(
    active: &'static mut binbook_fw::library::LibraryViewport,
) {
    use binbook_fw::{
        library_index_load::{LibraryIndexLoadUpdate, LibraryViewportLoadClient},
        runtime_engine::{RuntimeEvent, RuntimeEventKind},
        storage_owner::MAX_BOUNDED_IO_BYTES,
    };

    static INDEX_BUFFER: StaticCell<[u8; MAX_BOUNDED_IO_BYTES as usize]> = StaticCell::new();
    static BOOK_BUFFER: StaticCell<[u8; 8192]> = StaticCell::new();
    static OPENED_BOOK: StaticCell<Option<binbook_fw::owner_book::OwnerBook>> = StaticCell::new();
    let mut buffer: Option<&'static mut [u8]> =
        Some(INDEX_BUFFER.init([0; MAX_BOUNDED_IO_BYTES as usize]));

    embassy_time::Timer::after_secs(2).await;
    let started = embassy_time::Instant::now().as_millis();
    RUNTIME_EVENT_CHANNEL
        .send(RuntimeEvent {
            timestamp_ms: started,
            kind: RuntimeEventKind::StorageOwnerProbe {
                phase: 0,
                status: 0,
                elapsed_ms: 0,
            },
        })
        .await;
    let (mut client, mut request) = LibraryViewportLoadClient::begin(active, 0, 10_000);
    let (cache_status, cache_error, cache_detail) = loop {
        let needs_buffer = matches!(request.request, Request::Read { .. });
        let lease = if needs_buffer {
            Some(BufferLease::new(
                BufferSlot::LibraryIndex,
                buffer.take().expect("index buffer must be returned"),
            ))
        } else {
            None
        };
        STORAGE_REQUEST_CHANNEL
            .send(OwnedRequest {
                envelope: request,
                buffer: lease,
            })
            .await;
        let response = STORAGE_LIBRARY_RESPONSE_CHANNEL.receive().await;
        if let Some(lease) = response.buffer {
            buffer = Some(lease.into_bytes());
        }
        let bytes = needs_buffer.then(|| {
            &buffer
                .as_deref()
                .expect("index response must return its buffer")[..]
        });
        match client.accept(response.envelope, bytes) {
            Ok(LibraryIndexLoadUpdate::Request(next)) => request = next,
            Ok(LibraryIndexLoadUpdate::Complete(status)) => break (status, 0u8, 0u32),
            Err(error) => {
                let (code, detail) = match error {
                    binbook_fw::library_index_load::LibraryIndexLoadError::Correlation => (1, 0),
                    binbook_fw::library_index_load::LibraryIndexLoadError::Completion => (2, 0),
                    binbook_fw::library_index_load::LibraryIndexLoadError::Length => (3, 0),
                    binbook_fw::library_index_load::LibraryIndexLoadError::MissingBytes => (4, 0),
                    binbook_fw::library_index_load::LibraryIndexLoadError::HeaderLength {
                        file_len,
                        record_count,
                    } => (record_count.min(u8::MAX as u16) as u8, file_len),
                    binbook_fw::library_index_load::LibraryIndexLoadError::Decode(codec) => {
                        use binbook_fw::library::LibraryIndexError::*;
                        (
                            match codec {
                                Magic => 10,
                                Version => 11,
                                HeaderSize => 12,
                                Reserved => 13,
                                TooManyEntries => 14,
                                Length => 15,
                                Crc => 16,
                                NameLength => 17,
                                NameEncoding => 18,
                                NamePadding => 19,
                                Extension => 20,
                                FilenameHash => 21,
                                DuplicateName => 22,
                                BufferTooSmall => 23,
                                RecordIndex => 24,
                                Status => 25,
                                Generation => 26,
                                Ordering => 27,
                            },
                            0,
                        )
                    }
                    binbook_fw::library_index_load::LibraryIndexLoadError::AlreadyComplete => {
                        (6, 0)
                    }
                };
                break (binbook_fw::library::LibraryStatus::CardError, code, detail);
            }
        }
    };
    drop(client);
    let cache_ended = embassy_time::Instant::now().as_millis();
    RUNTIME_EVENT_CHANNEL
        .send(RuntimeEvent {
            timestamp_ms: cache_ended,
            kind: RuntimeEventKind::StorageOwnerProbe {
                phase: 1,
                status: active.entries().len().min(u8::MAX as usize) as u8,
                elapsed_ms: cache_ended.saturating_sub(started).min(u32::MAX as u64) as u32,
            },
        })
        .await;
    RUNTIME_EVENT_CHANNEL
        .send(RuntimeEvent {
            timestamp_ms: cache_ended,
            kind: RuntimeEventKind::StorageOwnerProbe {
                phase: 4,
                status: cache_error,
                elapsed_ms: cache_detail,
            },
        })
        .await;
    let _ = cache_status;
    let opened = OPENED_BOOK.init(None);
    let mut book_buffer: Option<&'static mut [u8]> = Some(BOOK_BUFFER.init([0; 8192]));
    if let Some(entry) = active.entries().first() {
        use binbook_fw::owner_book::{OwnerBookOpenClient, OwnerBookOpenUpdate};
        let open_started = embassy_time::Instant::now().as_millis();
        if let Ok((mut open, mut request)) =
            OwnerBookOpenClient::begin(entry.name.as_str(), opened, 30_000)
        {
            let open_ok = loop {
                let needs_buffer = matches!(request.request, Request::Read { .. });
                let lease = needs_buffer.then(|| {
                    BufferLease::new(
                        BufferSlot::Reader,
                        book_buffer.take().expect("book buffer must be returned"),
                    )
                });
                STORAGE_REQUEST_CHANNEL
                    .send(OwnedRequest {
                        envelope: request,
                        buffer: lease,
                    })
                    .await;
                let response = super::STORAGE_READER_RESPONSE_CHANNEL.receive().await;
                if let Some(lease) = response.buffer {
                    book_buffer = Some(lease.into_bytes());
                }
                match open.accept(
                    response.envelope,
                    book_buffer
                        .as_deref_mut()
                        .expect("book buffer must be local"),
                ) {
                    Ok(OwnerBookOpenUpdate::Request(next)) => request = next,
                    Ok(OwnerBookOpenUpdate::Complete) => break true,
                    Err(_) => break false,
                }
            };
            drop(open);
            let page_count = if open_ok {
                opened.as_ref().map_or(0, |book| book.page_count())
            } else {
                0
            };
            let open_ended = embassy_time::Instant::now().as_millis();
            RUNTIME_EVENT_CHANNEL
                .send(RuntimeEvent {
                    timestamp_ms: open_ended,
                    kind: RuntimeEventKind::StorageOwnerProbe {
                        phase: 5,
                        status: page_count.min(u8::MAX as u32) as u8,
                        elapsed_ms: open_ended.saturating_sub(open_started).min(u32::MAX as u64)
                            as u32,
                    },
                })
                .await;
        }
    }
    let mut prefetch = heapless::Vec::new();
    for entry in active.entries() {
        if let Ok(name) = binbook_fw::storage_owner::StorageName::new(entry.name.as_str()) {
            let _ = prefetch.push(name);
        }
    }
    LIBRARY_REFRESH_TRIGGER_CHANNEL
        .send(LibraryRefreshTrigger {
            trace_id: None,
            prefetch,
        })
        .await;
    let completion = LIBRARY_REFRESH_COMPLETION_CHANNEL.receive().await;
    RUNTIME_EVENT_CHANNEL
        .send(RuntimeEvent {
            timestamp_ms: embassy_time::Instant::now().as_millis(),
            kind: RuntimeEventKind::StorageOwnerProbe {
                phase: 3,
                status: completion.book_count.min(u8::MAX as u16) as u8,
                elapsed_ms: completion.elapsed_ms,
            },
        })
        .await;
    let _ = completion.status;
    if let Some((name, token)) = completion.prepared.first().and_then(|prepared| {
        active
            .entries()
            .get(usize::from(prepared.absolute_index).checked_sub(active.start())?)
            .and_then(|entry| {
                binbook_fw::storage_owner::StorageName::new(entry.name.as_str())
                    .ok()
                    .map(|name| (name, prepared.token))
            })
    }) {
        use binbook_fw::owner_book::{OwnerBookOpenClient, OwnerBookOpenUpdate};
        let cached_started = embassy_time::Instant::now().as_millis();
        if let Ok((mut open, mut request)) =
            OwnerBookOpenClient::begin_cached(name.as_str(), token, opened, 40_000)
        {
            let open_ok = loop {
                let needs_buffer = matches!(request.request, Request::Read { .. });
                let lease = needs_buffer.then(|| {
                    BufferLease::new(
                        BufferSlot::Reader,
                        book_buffer.take().expect("book buffer must be returned"),
                    )
                });
                STORAGE_REQUEST_CHANNEL
                    .send(OwnedRequest {
                        envelope: request,
                        buffer: lease,
                    })
                    .await;
                let response = super::STORAGE_READER_RESPONSE_CHANNEL.receive().await;
                if let Some(lease) = response.buffer {
                    book_buffer = Some(lease.into_bytes());
                }
                match open.accept(
                    response.envelope,
                    book_buffer
                        .as_deref_mut()
                        .expect("book buffer must be local"),
                ) {
                    Ok(OwnerBookOpenUpdate::Request(next)) => request = next,
                    Ok(OwnerBookOpenUpdate::Complete) => break true,
                    Err(_) => break false,
                }
            };
            drop(open);
            let cached_ended = embassy_time::Instant::now().as_millis();
            RUNTIME_EVENT_CHANNEL
                .send(RuntimeEvent {
                    timestamp_ms: cached_ended,
                    kind: RuntimeEventKind::StorageOwnerProbe {
                        phase: 6,
                        status: if open_ok { 0 } else { 1 },
                        elapsed_ms: cached_ended
                            .saturating_sub(cached_started)
                            .min(u32::MAX as u64) as u32,
                    },
                })
                .await;
        }
    }
}
