use binbook_fw::{
    owner_book::{
        OwnerBook, OwnerBookOpenClient, OwnerBookOpenUpdate, OwnerPageLoadClient, OwnerPlaneStream,
        OwnerPlaneStreamUpdate,
    },
    reader_state::{ReaderState, MAX_STATE_BYTES},
    state_owner::{OwnerStateSlots, StateLoadUpdate, StateSaveUpdate},
    storage_owner::{BufferLease, BufferSlot, OwnedRequest, Request},
};
use static_cell::StaticCell;
use xteink_x4_display::panel::{PanelRamTarget, X4Panel};

use super::{
    REQUEST_EPOCH, RUNTIME_EVENT_CHANNEL, STORAGE_LIBRARY_RESPONSE_CHANNEL,
    STORAGE_READER_RESPONSE_CHANNEL, STORAGE_REQUEST_CHANNEL, STORAGE_STATE_RESPONSE_CHANNEL,
};

pub(super) const OWNER_READER_BUFFER_BYTES: usize = 8192;
pub(super) const OWNER_ROWS_BYTES: usize = 1600;

const LATENCY_STATE_UPLOAD_BEGIN: u8 = 64;
const LATENCY_STATE_UPLOAD_WRITE: u8 = 65;
const LATENCY_STATE_UPLOAD_COMMIT: u8 = 66;
const LATENCY_STATE_READBACK: u8 = 67;
const LATENCY_STATE_UPLOAD_ABORT: u8 = 68;
const LATENCY_PLANE_STORAGE: u8 = 69;
const LATENCY_PLANE_DECODE: u8 = 70;
const LATENCY_PLANE_PANEL_WRITE: u8 = 71;

pub(super) struct OwnerReaderBuffers {
    io: Option<&'static mut [u8]>,
    rows: &'static mut [u8],
}

pub(super) struct OwnerStateBuffer {
    bytes: Option<&'static mut [u8]>,
}

impl OwnerStateBuffer {
    pub(super) fn init() -> Self {
        static BYTES: StaticCell<[u8; MAX_STATE_BYTES]> = StaticCell::new();
        Self {
            bytes: Some(BYTES.init([0; MAX_STATE_BYTES])),
        }
    }

    fn bytes_mut(&mut self) -> &mut [u8] {
        self.bytes.as_deref_mut().expect("state buffer is local")
    }

    async fn exchange(
        &mut self,
        request: binbook_fw::storage_owner::RequestEnvelope,
    ) -> binbook_fw::storage_owner::ResponseEnvelope {
        let needs_buffer = matches!(
            request.request,
            Request::Read { .. } | Request::UploadWrite { .. }
        );
        let lease = needs_buffer.then(|| {
            BufferLease::new(
                BufferSlot::State,
                self.bytes.take().expect("state buffer must be returned"),
            )
        });
        STORAGE_REQUEST_CHANNEL
            .send(OwnedRequest {
                envelope: request,
                buffer: lease,
            })
            .await;
        let response = STORAGE_STATE_RESPONSE_CHANNEL.receive().await;
        if let Some(lease) = response.buffer {
            self.bytes = Some(lease.into_bytes());
        }
        response.envelope
    }
}

pub(super) async fn load_state(
    slots: &mut OwnerStateSlots,
    state: &mut ReaderState,
    buffer: &mut OwnerStateBuffer,
    sequence: &mut u32,
) -> Result<bool, ()> {
    let (mut client, mut request) = slots.begin_load(state, *sequence);
    loop {
        let response = buffer.exchange(request).await;
        match client
            .accept(response, buffer.bytes_mut())
            .map_err(|_| ())?
        {
            StateLoadUpdate::Request(next) => request = next,
            StateLoadUpdate::Complete { warning } => {
                *sequence = sequence.wrapping_add(256).max(1);
                return Ok(warning);
            }
        }
    }
}

pub(super) async fn save_state(
    slots: &mut OwnerStateSlots,
    state: &mut ReaderState,
    buffer: &mut OwnerStateBuffer,
    sequence: &mut u32,
) -> Result<(), ()> {
    let (mut client, mut request) = slots
        .begin_save(state, buffer.bytes_mut(), *sequence)
        .map_err(|_| ())?;
    loop {
        let continuing_write = matches!(request.request, Request::UploadWrite { .. });
        let request_code = owner_request_code(&request.request);
        let request_started = embassy_time::Instant::now().as_millis();
        let response = buffer.exchange(request).await;
        emit_state_save_latency_probe(
            state_save_latency_phase(request_code),
            response.response.is_ok(),
            request_started,
        )
        .await;
        if let Err(error) = response.response {
            emit_state_save_probe(41, request_code, completion_error_code(error)).await;
        }
        let update = match client.accept(response, buffer.bytes_mut()) {
            Ok(update) => update,
            Err(error) => {
                emit_state_save_probe(42, state_save_error_code(error), request_code.into()).await;
                return Err(());
            }
        };
        match update {
            StateSaveUpdate::Request(next) => {
                request = next;
                if continuing_write {
                    embassy_time::Timer::after_millis(10).await;
                }
            }
            StateSaveUpdate::Complete => {
                *sequence = sequence.wrapping_add(256).max(1);
                return Ok(());
            }
        }
    }
}

const fn owner_request_code(request: &Request) -> u8 {
    match request {
        Request::UploadBegin { .. } => 1,
        Request::UploadWrite { .. } => 2,
        Request::UploadCommit { .. } => 3,
        Request::Read { .. } => 4,
        Request::UploadAbort { .. } => 5,
        _ => 0,
    }
}

const fn state_save_latency_phase(request_code: u8) -> u8 {
    match request_code {
        1 => LATENCY_STATE_UPLOAD_BEGIN,
        2 => LATENCY_STATE_UPLOAD_WRITE,
        3 => LATENCY_STATE_UPLOAD_COMMIT,
        4 => LATENCY_STATE_READBACK,
        5 => LATENCY_STATE_UPLOAD_ABORT,
        _ => 0,
    }
}

const fn completion_error_code(error: binbook_fw::storage_owner::CompletionError) -> u32 {
    match error {
        binbook_fw::storage_owner::CompletionError::Request(_) => 1,
        binbook_fw::storage_owner::CompletionError::Backend => 2,
        binbook_fw::storage_owner::CompletionError::BufferUnavailable => 3,
    }
}

const fn state_save_error_code(error: binbook_fw::state_owner::StateSaveError) -> u8 {
    match error {
        binbook_fw::state_owner::StateSaveError::Correlation => 1,
        binbook_fw::state_owner::StateSaveError::Completion => 2,
        binbook_fw::state_owner::StateSaveError::Codec(_) => 3,
        binbook_fw::state_owner::StateSaveError::Mismatch => 4,
    }
}

async fn emit_state_save_probe(phase: u8, status: u8, detail: u32) {
    RUNTIME_EVENT_CHANNEL
        .send(binbook_fw::runtime_engine::RuntimeEvent {
            timestamp_ms: embassy_time::Instant::now().as_millis(),
            kind: binbook_fw::runtime_engine::RuntimeEventKind::StorageOwnerProbe {
                phase,
                status,
                elapsed_ms: detail,
            },
        })
        .await;
}

async fn emit_state_save_latency_probe(phase: u8, succeeded: bool, started_ms: u64) {
    #[cfg(feature = "diagnostic-console")]
    if phase != 0 {
        emit_state_save_probe(
            phase,
            u8::from(!succeeded),
            embassy_time::Instant::now()
                .as_millis()
                .saturating_sub(started_ms)
                .min(u32::MAX as u64) as u32,
        )
        .await;
    }
    #[cfg(not(feature = "diagnostic-console"))]
    let _ = (phase, succeeded, started_ms);
}

pub(super) async fn load_viewport(
    viewport: &mut binbook_fw::library::LibraryViewport,
    start: usize,
    buffers: &mut OwnerReaderBuffers,
    sequence: &mut u32,
) -> Result<binbook_fw::library::LibraryStatus, ()> {
    use binbook_fw::library_index_load::{LibraryIndexLoadUpdate, LibraryViewportLoadClient};

    let (mut client, mut request) = LibraryViewportLoadClient::begin(viewport, start, *sequence);
    loop {
        let needs_buffer = matches!(request.request, Request::Read { .. });
        let lease = needs_buffer.then(|| {
            BufferLease::new(
                BufferSlot::LibraryIndex,
                buffers
                    .io
                    .take()
                    .expect("viewport IO buffer must be returned"),
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
            buffers.io = Some(lease.into_bytes());
        }
        let bytes = needs_buffer.then(|| buffers.io.as_deref().unwrap());
        match client.accept(response.envelope, bytes).map_err(|_| ())? {
            LibraryIndexLoadUpdate::Request(next) => request = next,
            LibraryIndexLoadUpdate::Complete(status) => {
                *sequence = sequence.wrapping_add(64).max(1);
                return Ok(status);
            }
        }
    }
}

pub(super) async fn load_viewport_window(
    viewport: &mut binbook_fw::library::LibraryViewport,
    cursor: binbook_fw::library::LibraryIndexCursor,
    start: usize,
    buffers: &mut OwnerReaderBuffers,
    sequence: &mut u32,
) -> Result<(), ()> {
    use binbook_fw::library_index_load::LibraryViewportWindowClient;

    let (mut client, request) =
        LibraryViewportWindowClient::begin(viewport, cursor, start, *sequence).map_err(|_| ())?;
    let lease = BufferLease::new(
        BufferSlot::LibraryIndex,
        buffers
            .io
            .take()
            .expect("viewport window buffer must be returned"),
    );
    STORAGE_REQUEST_CHANNEL
        .send(OwnedRequest {
            envelope: request,
            buffer: Some(lease),
        })
        .await;
    let response = STORAGE_LIBRARY_RESPONSE_CHANNEL.receive().await;
    if let Some(lease) = response.buffer {
        buffers.io = Some(lease.into_bytes());
    }
    client
        .accept(response.envelope, buffers.io.as_deref().ok_or(())?)
        .map_err(|_| ())?;
    *sequence = sequence.wrapping_add(1).max(1);
    Ok(())
}

pub(super) async fn prune_state_from_index(
    state: &mut ReaderState,
    buffers: &mut OwnerReaderBuffers,
    sequence: &mut u32,
) -> Result<usize, ()> {
    use binbook_fw::library_index_load::{LibraryIndexPruneClient, LibraryIndexPruneUpdate};

    let (mut client, mut request) = LibraryIndexPruneClient::begin(state, *sequence);
    loop {
        let needs_buffer = matches!(request.request, Request::Read { .. });
        let lease = needs_buffer.then(|| {
            BufferLease::new(
                BufferSlot::LibraryIndex,
                buffers.io.take().expect("prune IO buffer must be returned"),
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
            buffers.io = Some(lease.into_bytes());
        }
        let bytes = needs_buffer.then(|| buffers.io.as_deref().unwrap());
        match client.accept(response.envelope, bytes).map_err(|_| ())? {
            LibraryIndexPruneUpdate::Request(next) => request = next,
            LibraryIndexPruneUpdate::Complete { removed } => {
                *sequence = sequence.wrapping_add(64).max(1);
                return Ok(removed);
            }
        }
    }
}

impl OwnerReaderBuffers {
    pub(super) fn init() -> Self {
        static IO: StaticCell<[u8; OWNER_READER_BUFFER_BYTES]> = StaticCell::new();
        static ROWS: StaticCell<[u8; OWNER_ROWS_BYTES]> = StaticCell::new();
        Self {
            io: Some(IO.init([0; OWNER_READER_BUFFER_BYTES])),
            rows: ROWS.init([0; OWNER_ROWS_BYTES]),
        }
    }

    fn io_mut(&mut self) -> &mut [u8] {
        self.io.as_deref_mut().expect("reader IO buffer is local")
    }

    async fn exchange(
        &mut self,
        request: binbook_fw::storage_owner::RequestEnvelope,
    ) -> binbook_fw::storage_owner::ResponseEnvelope {
        let needs_buffer = matches!(request.request, Request::Read { .. });
        let lease = needs_buffer.then(|| {
            BufferLease::new(
                BufferSlot::Reader,
                self.io.take().expect("reader IO buffer must be returned"),
            )
        });
        STORAGE_REQUEST_CHANNEL
            .send(OwnedRequest {
                envelope: request,
                buffer: lease,
            })
            .await;
        let response = STORAGE_READER_RESPONSE_CHANNEL.receive().await;
        if let Some(lease) = response.buffer {
            self.io = Some(lease.into_bytes());
        }
        response.envelope
    }
}

pub(super) async fn open_book(
    name: &str,
    token: Option<binbook_fw::library::MountFileToken>,
    target: &mut Option<OwnerBook>,
    buffers: &mut OwnerReaderBuffers,
    sequence: &mut u32,
) -> Result<(), ()> {
    let (mut client, mut request) = if let Some(token) = token {
        OwnerBookOpenClient::begin_cached(name, token, target, *sequence).map_err(|_| ())?
    } else {
        OwnerBookOpenClient::begin(name, target, *sequence).map_err(|_| ())?
    };
    loop {
        let response = buffers.exchange(request).await;
        match client.accept(response, buffers.io_mut()).map_err(|_| ())? {
            OwnerBookOpenUpdate::Request(next) => request = next,
            OwnerBookOpenUpdate::Complete => {
                *sequence = sequence.wrapping_add(32).max(1);
                return Ok(());
            }
        }
    }
}

pub(super) async fn load_page(
    book: &OwnerBook,
    page: u32,
    target: &mut Option<binbook_core::PageInfo>,
    buffers: &mut OwnerReaderBuffers,
    sequence: &mut u32,
) -> Result<(), ()> {
    let (mut client, request) =
        OwnerPageLoadClient::begin(book, page, target, *sequence).map_err(|_| ())?;
    let response = buffers.exchange(request).await;
    client.accept(response, buffers.io_mut()).map_err(|_| ())?;
    *sequence = sequence.wrapping_add(1).max(1);
    Ok(())
}

pub(super) async fn stage_plane<SPI, DC, RST, BUSY>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    book: &OwnerBook,
    plane: binbook_core::PlaneDescriptor,
    target: PanelRamTarget,
    buffers: &mut OwnerReaderBuffers,
    sequence: &mut u32,
) -> Result<(), ()>
where
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
{
    stage_plane_inner(panel, book, plane, target, buffers, sequence, None)
        .await
        .map(|_| ())
}

pub(super) async fn stage_plane_for_epoch<SPI, DC, RST, BUSY>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    book: &OwnerBook,
    plane: binbook_core::PlaneDescriptor,
    target: PanelRamTarget,
    buffers: &mut OwnerReaderBuffers,
    sequence: &mut u32,
    expected_epoch: u32,
) -> Result<bool, ()>
where
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
{
    stage_plane_inner(
        panel,
        book,
        plane,
        target,
        buffers,
        sequence,
        Some(expected_epoch),
    )
    .await
}

async fn stage_plane_inner<SPI, DC, RST, BUSY>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    book: &OwnerBook,
    plane: binbook_core::PlaneDescriptor,
    target: PanelRamTarget,
    buffers: &mut OwnerReaderBuffers,
    sequence: &mut u32,
    expected_epoch: Option<u32>,
) -> Result<bool, ()>
where
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
{
    let (mut stream, mut request) =
        OwnerPlaneStream::begin(book, plane, *sequence, 4096).map_err(|_| ())?;
    let mut first_row = 0u16;
    #[cfg(feature = "diagnostic-console")]
    let mut storage_ms = 0u64;
    #[cfg(feature = "diagnostic-console")]
    let mut decode_ms = 0u64;
    #[cfg(feature = "diagnostic-console")]
    let mut panel_write_ms = 0u64;
    loop {
        if expected_epoch
            .is_some_and(|epoch| REQUEST_EPOCH.load(portable_atomic::Ordering::Acquire) != epoch)
        {
            return Ok(false);
        }
        #[cfg(feature = "diagnostic-console")]
        let storage_started = embassy_time::Instant::now().as_millis();
        let response = buffers.exchange(request).await;
        #[cfg(feature = "diagnostic-console")]
        {
            storage_ms = storage_ms.saturating_add(
                embassy_time::Instant::now()
                    .as_millis()
                    .saturating_sub(storage_started),
            );
        }
        let input = buffers.io.as_deref().ok_or(())?;
        #[cfg(feature = "diagnostic-console")]
        let decode_started = embassy_time::Instant::now().as_millis();
        let mut update = stream
            .accept(response, input, buffers.rows)
            .map_err(|_| ())?;
        #[cfg(feature = "diagnostic-console")]
        {
            decode_ms = decode_ms.saturating_add(
                embassy_time::Instant::now()
                    .as_millis()
                    .saturating_sub(decode_started),
            );
        }
        loop {
            match update {
                OwnerPlaneStreamUpdate::Request(next) => {
                    request = next;
                    break;
                }
                OwnerPlaneStreamUpdate::RowsReady {
                    row_count,
                    complete,
                } => {
                    if expected_epoch.is_some_and(|epoch| {
                        REQUEST_EPOCH.load(portable_atomic::Ordering::Acquire) != epoch
                    }) {
                        return Ok(false);
                    }
                    let len = usize::from(row_count) * xteink_x4_display::profile::ROW_BYTES;
                    #[cfg(feature = "diagnostic-console")]
                    let panel_write_started = embassy_time::Instant::now().as_millis();
                    panel
                        .write_prepared_rows(first_row, &buffers.rows[..len], target)
                        .map_err(|_| ())?;
                    #[cfg(feature = "diagnostic-console")]
                    {
                        panel_write_ms = panel_write_ms.saturating_add(
                            embassy_time::Instant::now()
                                .as_millis()
                                .saturating_sub(panel_write_started),
                        );
                    }
                    first_row = first_row.checked_add(row_count).ok_or(())?;
                    if complete {
                        if first_row != xteink_x4_display::profile::PHYSICAL_HEIGHT {
                            return Err(());
                        }
                        *sequence = sequence.wrapping_add(64).max(1);
                        #[cfg(feature = "diagnostic-console")]
                        {
                            emit_state_save_probe(
                                LATENCY_PLANE_STORAGE,
                                0,
                                storage_ms.min(u32::MAX as u64) as u32,
                            )
                            .await;
                            emit_state_save_probe(
                                LATENCY_PLANE_DECODE,
                                0,
                                decode_ms.min(u32::MAX as u64) as u32,
                            )
                            .await;
                            emit_state_save_probe(
                                LATENCY_PLANE_PANEL_WRITE,
                                0,
                                panel_write_ms.min(u32::MAX as u64) as u32,
                            )
                            .await;
                        }
                        return Ok(true);
                    }
                    #[cfg(feature = "diagnostic-console")]
                    let decode_started = embassy_time::Instant::now().as_millis();
                    update = stream.advance(input, buffers.rows).map_err(|_| ())?;
                    #[cfg(feature = "diagnostic-console")]
                    {
                        decode_ms = decode_ms.saturating_add(
                            embassy_time::Instant::now()
                                .as_millis()
                                .saturating_sub(decode_started),
                        );
                    }
                }
            }
        }
    }
}
