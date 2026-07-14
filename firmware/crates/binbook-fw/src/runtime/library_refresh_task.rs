use binbook_fw::{
    library::Library,
    library_index_save::{LibraryIndexSaveClient, LibraryIndexSaveUpdate},
    library_refresh::{LibraryRefreshClient, LibraryRefreshUpdate},
    storage_owner::{BufferLease, BufferSlot, OwnedRequest, Request, SCAN_BATCH_BYTES},
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

#[embassy_executor::task]
pub(super) async fn library_refresh_task(
    refresh: &'static mut Library,
    trigger: Receiver<'static, CriticalSectionRawMutex, LibraryRefreshTrigger, 1>,
    completion: Sender<'static, CriticalSectionRawMutex, LibraryRefreshCompletion, 1>,
) {
    static BUFFER: StaticCell<[u8; SCAN_BATCH_BYTES]> = StaticCell::new();
    let mut buffer: Option<&'static mut [u8]> = Some(BUFFER.init([0; SCAN_BATCH_BYTES]));
    let mut sequence = 1u32;
    loop {
        let trigger = trigger.receive().await;
        let started = embassy_time::Instant::now().as_millis();
        let (mut client, mut request) =
            LibraryRefreshClient::begin_with_prefetch(refresh, &trigger.prefetch, sequence)
                .expect("viewport prefetch names are already protocol-bounded");
        let status = loop {
            let needs_buffer = matches!(request.request, Request::ScanStep { .. });
            #[cfg(feature = "mounted-storage-owner")]
            let request_code = refresh_request_code(&request.request);
            let lease = if needs_buffer {
                Some(BufferLease::new(
                    BufferSlot::LibraryIndex,
                    buffer.take().expect("refresh buffer must be returned"),
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
            let batch = needs_buffer.then(|| {
                &buffer
                    .as_deref()
                    .expect("refresh response must return its buffer")[..]
            });
            #[cfg(feature = "mounted-storage-owner")]
            if let Err(error) = response.envelope.response {
                probe_refresh_failure(request_code, error).await;
            }
            match client.accept(response.envelope, batch) {
                Ok(LibraryRefreshUpdate::Request(next)) => {
                    let continuing_scan = matches!(next.request, Request::ScanStep { .. });
                    request = next;
                    if continuing_scan {
                        embassy_time::Timer::after_millis(10).await;
                    }
                }
                Ok(LibraryRefreshUpdate::Complete(status)) => break status,
                Err(_) => {
                    if let Some(cancel) = client.cancel() {
                        STORAGE_REQUEST_CHANNEL
                            .send(OwnedRequest {
                                envelope: cancel,
                                buffer: None,
                            })
                            .await;
                        let cancelled = STORAGE_LIBRARY_RESPONSE_CHANNEL.receive().await;
                        let _ = client.accept(cancelled.envelope, None);
                    }
                    break binbook_fw::library::LibraryStatus::CardError;
                }
            }
        };
        drop(client);
        if status == binbook_fw::library::LibraryStatus::Ready {
            let (mut save, mut request) =
                LibraryIndexSaveClient::begin(refresh, sequence.wrapping_add(32));
            loop {
                let needs_buffer = matches!(request.request, Request::UploadWrite { .. });
                #[cfg(feature = "mounted-storage-owner")]
                let request_code = refresh_request_code(&request.request);
                let lease = if needs_buffer {
                    Some(BufferLease::new(
                        BufferSlot::LibraryIndex,
                        buffer.take().expect("index-save buffer must be returned"),
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
                let save_buffer = buffer
                    .as_deref_mut()
                    .expect("index-save buffer must be locally available");
                #[cfg(feature = "mounted-storage-owner")]
                if let Err(error) = response.envelope.response {
                    probe_index_save_failure(
                        request_code,
                        match error {
                            binbook_fw::storage_owner::CompletionError::Request(_) => 1,
                            binbook_fw::storage_owner::CompletionError::Backend => 2,
                            binbook_fw::storage_owner::CompletionError::BufferUnavailable => 3,
                        },
                    )
                    .await;
                }
                match save.accept(response.envelope, save_buffer) {
                    Ok(LibraryIndexSaveUpdate::Request(next)) => {
                        let continuing_write = matches!(next.request, Request::UploadWrite { .. });
                        request = next;
                        if continuing_write {
                            embassy_time::Timer::after_millis(10).await;
                        }
                    }
                    Ok(LibraryIndexSaveUpdate::Complete) => break,
                    Err(_) => {
                        #[cfg(feature = "mounted-storage-owner")]
                        probe_index_save_failure(0, 4).await;
                        break;
                    }
                }
            }
        }
        sequence = sequence.wrapping_add(64).max(1);
        let ended = embassy_time::Instant::now().as_millis();
        let mut prepared = heapless::Vec::new();
        for entry in refresh.entries() {
            let (Ok(name), Some(token)) = (
                binbook_fw::storage_owner::StorageName::new(entry.name.as_str()),
                entry.mount_token,
            ) else {
                continue;
            };
            let _ = prepared.push((name, token));
        }
        completion
            .send(LibraryRefreshCompletion {
                status: status.code(),
                book_count: refresh.entries().len() as u16,
                elapsed_ms: ended.saturating_sub(started).min(u32::MAX as u64) as u32,
                prepared,
            })
            .await;
    }
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
        .send(LibraryRefreshTrigger { prefetch })
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
    if let Some((name, token)) = completion.prepared.first().cloned() {
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
