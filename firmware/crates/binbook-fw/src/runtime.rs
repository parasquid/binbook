mod display_backend;
mod display_task;
mod input_task;
#[cfg(all(feature = "sd-storage", feature = "mounted-storage-owner"))]
mod library_refresh_task;
#[cfg(all(feature = "sd-storage", feature = "mounted-storage-owner"))]
mod mounted_display_task;
#[cfg(all(feature = "sd-storage", feature = "mounted-storage-owner"))]
mod mounted_reader_io;
mod render_timing;
#[cfg(feature = "sd-storage")]
mod storage_owner_task;
#[cfg(all(feature = "usb-rx-isr-buffer", target_arch = "riscv32"))]
mod usb_rx_isr;

#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
use core::cell::RefCell;

#[cfg(feature = "diagnostic-console")]
mod diagnostic_aggregator;
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
mod diagnostic_bulk;
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
mod diagnostic_bulk_action;
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
mod diagnostic_bulk_owner;
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
mod diagnostic_bulk_timeout;
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
mod diagnostic_bulk_types;
#[cfg(feature = "diagnostic-console")]
mod diagnostic_console;
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
mod diagnostic_owner_storage;

use embassy_executor::Spawner;
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Receiver, Sender},
};
use portable_atomic::{AtomicU32, AtomicU8};

use binbook_fw::async_refresh::{DisplayRequest, PAGE_TURN_QUEUE_CAPACITY, STARTUP_DISPLAY_MODE};
use binbook_fw::runtime_engine::RuntimeEvent;
#[cfg(feature = "diagnostic-console")]
use binbook_fw::runtime_engine::RuntimeEventKind;

static DISPLAY_MODE: AtomicU8 = AtomicU8::new(STARTUP_DISPLAY_MODE);
#[cfg(feature = "sd-storage")]
static LIBRARY_STATUS: AtomicU8 = AtomicU8::new(2);

#[cfg(feature = "sd-storage")]
const SD_SPI_FREQ_HZ: u32 = 4_000_000;

#[cfg(feature = "sd-storage")]
type RuntimeSdFilesystem = binbook_fw::storage::SdFilesystem<
    RuntimeSdSpi,
    binbook_fw::board::DisplayDelay,
    binbook_fw::storage::FixedTime,
    RuntimeBlockIoObserver,
>;

#[cfg(all(feature = "sd-storage", not(feature = "diagnostic-console")))]
type RuntimeSdSpi =
    binbook_fw::board::FreqManagedSpiDevice<'static, esp_hal::gpio::Output<'static>>;

#[cfg(all(feature = "sd-storage", feature = "diagnostic-console"))]
type RuntimeSdSpi = binbook_fw::sd_spi_trace::TracedSpiDevice<
    binbook_fw::board::FreqManagedSpiDevice<'static, esp_hal::gpio::Output<'static>>,
    RuntimeSdSpiObserver,
>;

#[cfg(feature = "sd-storage")]
#[derive(Clone, Copy)]
struct RuntimeBlockIoObserver;

#[cfg(feature = "sd-storage")]
impl embedded_sd_storage::BlockIoObserver for RuntimeBlockIoObserver {
    fn observe(&self, _event: embedded_sd_storage::BlockIoEvent) {
        #[cfg(feature = "diagnostic-console")]
        {
            let now_us = embassy_time::Instant::now().as_micros();
            TRANSFER_BLOCK_IO.observe(_event, now_us);
            if SD_CAPTURE_OWNER_CLIENT.load(portable_atomic::Ordering::Relaxed) == 3 {
                let contextual = SD_CAPTURE_PURPOSE.load(portable_atomic::Ordering::Relaxed);
                let purpose = SD_SEMANTIC_MAP.lock(|semantic| {
                    semantic.borrow().as_ref().map_or(contextual, |semantic| {
                        match semantic.classify(_event.start_block) {
                            embedded_sd_storage::SectorPurpose::OutsideVolume => 8,
                            embedded_sd_storage::SectorPurpose::Reserved => 1,
                            embedded_sd_storage::SectorPurpose::Fat { copy: 0 } => 2,
                            embedded_sd_storage::SectorPurpose::Fat { .. } => 3,
                            embedded_sd_storage::SectorPurpose::Directory => 4,
                            embedded_sd_storage::SectorPurpose::Journal => 5,
                            embedded_sd_storage::SectorPurpose::Payload => 6,
                            embedded_sd_storage::SectorPurpose::DataOther => contextual,
                        }
                    })
                });
                SD_WRITE_CAPTURE.lock(|capture| {
                    capture.borrow_mut().observe_block_io_with_purpose(
                        _event,
                        now_us as u32,
                        purpose,
                    );
                });
            }
        }
    }

    fn observe_read_data(&self, _start_block: u32, _blocks: &[embedded_sd_storage::Block]) {
        #[cfg(feature = "diagnostic-console")]
        for (offset, block) in _blocks.iter().enumerate() {
            let sector = _start_block.saturating_add(offset as u32);
            SD_FAT_PROBE.lock(|probe| {
                let mut probe = probe.borrow_mut();
                if sector == 0 {
                    if let Ok(geometry) =
                        embedded_sd_storage::FatGeometry::from_boot_sector(0, &block.contents)
                    {
                        SD_SEMANTIC_MAP.lock(|slot| {
                            *slot.borrow_mut() =
                                Some(embedded_sd_storage::SemanticSectorMap::new(geometry));
                        });
                    } else if block.contents[510..512] == [0x55, 0xaa] {
                        for entry in 0..4 {
                            let at = 446 + entry * 16;
                            let partition_start = u32::from_le_bytes(
                                block.contents[at + 8..at + 12].try_into().unwrap(),
                            );
                            if partition_start != 0 {
                                *probe = Some(partition_start);
                                break;
                            }
                        }
                    }
                }
                if *probe == Some(sector) {
                    if let Ok(geometry) =
                        embedded_sd_storage::FatGeometry::from_boot_sector(sector, &block.contents)
                    {
                        SD_SEMANTIC_MAP.lock(|slot| {
                            *slot.borrow_mut() =
                                Some(embedded_sd_storage::SemanticSectorMap::new(geometry));
                        });
                    }
                }
            });
        }
    }

    fn observe_write_data(&self, _start_block: u32, _blocks: &[embedded_sd_storage::Block]) {
        #[cfg(feature = "diagnostic-console")]
        SD_SEMANTIC_MAP.lock(|semantic| {
            let mut semantic = semantic.borrow_mut();
            let Some(semantic) = semantic.as_mut() else {
                return;
            };
            for (offset, block) in _blocks.iter().enumerate() {
                if embedded_sd_storage::is_upload_journal_sector(&block.contents) {
                    let sector = _start_block.saturating_add(offset as u32);
                    let _ =
                        semantic.add_range(sector, 1, embedded_sd_storage::SectorPurpose::Journal);
                }
            }
        });
    }
}

#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
#[derive(Clone, Copy)]
struct RuntimeSdSpiObserver;

#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
impl binbook_fw::sd_spi_trace::SpiTraceObserver for RuntimeSdSpiObserver {
    fn now_us(&self) -> u32 {
        embassy_time::Instant::now().as_micros() as u32
    }

    fn observe(&self, event: binbook_fw::sd_spi_trace::SpiOperationEvent) {
        if SD_CAPTURE_OWNER_CLIENT.load(portable_atomic::Ordering::Relaxed) == 3 {
            SD_WRITE_CAPTURE.lock(|capture| capture.borrow_mut().observe_spi(event));
        }
    }
}

type RequestSender =
    Sender<'static, CriticalSectionRawMutex, DisplayRequest, { PAGE_TURN_QUEUE_CAPACITY }>;
type RequestReceiver =
    Receiver<'static, CriticalSectionRawMutex, DisplayRequest, { PAGE_TURN_QUEUE_CAPACITY }>;
type PowerRequestSender =
    Sender<'static, CriticalSectionRawMutex, binbook_fw::power::PowerRequest, 1>;
type PowerRequestReceiver =
    Receiver<'static, CriticalSectionRawMutex, binbook_fw::power::PowerRequest, 1>;
type PowerAckSender = Sender<'static, CriticalSectionRawMutex, binbook_fw::power::PowerAck, 1>;
type PowerAckReceiver = Receiver<'static, CriticalSectionRawMutex, binbook_fw::power::PowerAck, 1>;

static REQUEST_CHANNEL: Channel<
    CriticalSectionRawMutex,
    DisplayRequest,
    { PAGE_TURN_QUEUE_CAPACITY },
> = Channel::new();
static POWER_REQUEST_CHANNEL: Channel<CriticalSectionRawMutex, binbook_fw::power::PowerRequest, 1> =
    Channel::new();
static POWER_ACK_CHANNEL: Channel<CriticalSectionRawMutex, binbook_fw::power::PowerAck, 1> =
    Channel::new();
static RUNTIME_EVENT_CHANNEL: Channel<CriticalSectionRawMutex, RuntimeEvent, 32> = Channel::new();
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
static TRANSFER_OWNER_TIMING_CHANNEL: Channel<
    CriticalSectionRawMutex,
    binbook_fw::transfer_owner_timing::OwnerTimingEvent,
    32,
> = Channel::new();
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
static TRANSFER_OWNER_TIMING_DROPS: AtomicU32 = AtomicU32::new(0);
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
static TRANSFER_BLOCK_IO: binbook_fw::transfer_block_io::BlockIoAccumulator =
    binbook_fw::transfer_block_io::BlockIoAccumulator::new();
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
#[cfg(feature = "bulk-transfer-runtime")]
const SD_WRITE_CAPTURE_RECORDS: usize = 64;
#[cfg(all(
    feature = "diagnostic-console",
    feature = "sd-storage",
    not(feature = "bulk-transfer-runtime")
))]
const SD_WRITE_CAPTURE_RECORDS: usize = 384;
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
static SD_WRITE_CAPTURE: Mutex<
    CriticalSectionRawMutex,
    RefCell<binbook_fw::sd_spi_trace::SdWriteCapture<SD_WRITE_CAPTURE_RECORDS>>,
> = Mutex::new(RefCell::new(binbook_fw::sd_spi_trace::SdWriteCapture::new()));
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
static SD_CAPTURE_OWNER_CLIENT: AtomicU8 = AtomicU8::new(0);
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
static SD_CAPTURE_PURPOSE: AtomicU8 = AtomicU8::new(0);
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
static SD_FAT_PROBE: Mutex<CriticalSectionRawMutex, RefCell<Option<u32>>> =
    Mutex::new(RefCell::new(None));
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
static SD_SEMANTIC_MAP: Mutex<
    CriticalSectionRawMutex,
    RefCell<Option<embedded_sd_storage::SemanticSectorMap<4>>>,
> = Mutex::new(RefCell::new(None));
static REQUEST_EPOCH: AtomicU32 = AtomicU32::new(0);

#[cfg(feature = "sd-storage")]
const STORAGE_QUEUE_CAPACITY: usize = 8;
#[cfg(feature = "sd-storage")]
type StorageOwnedRequest = binbook_fw::storage_owner::OwnedRequest<'static>;
#[cfg(feature = "sd-storage")]
type StorageOwnedResponse = binbook_fw::storage_owner::OwnedResponse<'static>;
#[cfg(feature = "sd-storage")]
static STORAGE_REQUEST_CHANNEL: Channel<
    CriticalSectionRawMutex,
    StorageOwnedRequest,
    STORAGE_QUEUE_CAPACITY,
> = Channel::new();
#[cfg(feature = "sd-storage")]
static STORAGE_READER_RESPONSE_CHANNEL: Channel<
    CriticalSectionRawMutex,
    StorageOwnedResponse,
    STORAGE_QUEUE_CAPACITY,
> = Channel::new();
#[cfg(feature = "sd-storage")]
static STORAGE_STATE_RESPONSE_CHANNEL: Channel<
    CriticalSectionRawMutex,
    StorageOwnedResponse,
    STORAGE_QUEUE_CAPACITY,
> = Channel::new();
#[cfg(feature = "sd-storage")]
static STORAGE_DIAGNOSTIC_RESPONSE_CHANNEL: Channel<
    CriticalSectionRawMutex,
    StorageOwnedResponse,
    STORAGE_QUEUE_CAPACITY,
> = Channel::new();
#[cfg(feature = "sd-storage")]
static STORAGE_LIBRARY_RESPONSE_CHANNEL: Channel<
    CriticalSectionRawMutex,
    StorageOwnedResponse,
    STORAGE_QUEUE_CAPACITY,
> = Channel::new();
#[cfg(feature = "sd-storage")]
static STORAGE_OWNER_PHASE: AtomicU32 = AtomicU32::new(0);
#[cfg(all(feature = "sd-storage", feature = "mounted-storage-owner"))]
#[derive(Clone)]
struct LibraryRefreshCompletion {
    status: u8,
    book_count: u16,
    elapsed_ms: u32,
    cursor: Option<binbook_fw::library::LibraryIndexCursor>,
    prepared: heapless::Vec<binbook_fw::library_publication::PreparedMount, 15>,
}
#[cfg(all(feature = "sd-storage", feature = "mounted-storage-owner"))]
struct LibraryRefreshTrigger {
    trace_id: Option<u32>,
    prefetch: heapless::Vec<
        binbook_fw::storage_owner::StorageName,
        { binbook_fw::library::LIBRARY_VIEWPORT_CAPACITY },
    >,
}
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
#[derive(Clone, Copy)]
struct TransferLibraryTiming {
    trace_id: u32,
    elapsed_us: u32,
}
#[cfg(all(feature = "sd-storage", feature = "mounted-storage-owner"))]
static LIBRARY_REFRESH_TRIGGER_CHANNEL: Channel<CriticalSectionRawMutex, LibraryRefreshTrigger, 1> =
    Channel::new();
#[cfg(all(feature = "sd-storage", feature = "mounted-storage-owner"))]
static LIBRARY_REFRESH_BARRIER: binbook_fw::library_refresh_barrier::LibraryRefreshBarrier =
    binbook_fw::library_refresh_barrier::LibraryRefreshBarrier::new_pending();
#[cfg(all(feature = "sd-storage", feature = "mounted-storage-owner"))]
static LIBRARY_REFRESH_COMPLETION_CHANNEL: Channel<
    CriticalSectionRawMutex,
    LibraryRefreshCompletion,
    1,
> = Channel::new();
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
static TRANSFER_LIBRARY_TIMING_CHANNEL: Channel<CriticalSectionRawMutex, TransferLibraryTiming, 1> =
    Channel::new();

#[cfg(feature = "diagnostic-console")]
type CommittedCompletion = binbook_fw::runtime_aggregator::CommittedCompletion;

#[cfg(feature = "diagnostic-console")]
#[derive(Clone, Copy)]
enum AggregatorQuery {
    Enqueue {
        pending: binbook_fw::diag::PendingCommand,
        request: DisplayRequest,
    },
    Status,
    LogGet {
        cursor: u32,
        max_bytes: u16,
    },
    LogClear,
    ProtocolErrors(u32),
}

#[cfg(feature = "diagnostic-console")]
#[derive(Clone, Copy)]
enum AggregatorResponse {
    Reserve(Result<(), binbook_fw::runtime_aggregator::ReserveError>),
    Status(binbook_fw::diag::DiagnosticSnapshot),
    Log {
        payload: [u8; binbook_diagnostic_protocol::MAX_PAYLOAD_BYTES],
        len: usize,
    },
    Ack,
}

#[cfg(feature = "diagnostic-console")]
static AGGREGATOR_QUERY_CHANNEL: Channel<CriticalSectionRawMutex, AggregatorQuery, 4> =
    Channel::new();
#[cfg(feature = "diagnostic-console")]
static AGGREGATOR_RESPONSE_CHANNEL: Channel<CriticalSectionRawMutex, AggregatorResponse, 4> =
    Channel::new();
#[cfg(feature = "diagnostic-console")]
static AGGREGATOR_COMPLETION_CHANNEL: Channel<
    CriticalSectionRawMutex,
    CommittedCompletion,
    { binbook_fw::async_refresh::DISPLAY_COMPLETION_CAPACITY },
> = Channel::new();

pub(crate) struct RuntimePeripherals {
    pub(crate) adc1: esp_hal::peripherals::ADC1<'static>,
    pub(crate) gpio1: esp_hal::peripherals::GPIO1<'static>,
    pub(crate) gpio2: esp_hal::peripherals::GPIO2<'static>,
    pub(crate) gpio3: esp_hal::peripherals::GPIO3<'static>,
    pub(crate) lpwr: esp_hal::peripherals::LPWR<'static>,
    pub(crate) spi2: esp_hal::peripherals::SPI2<'static>,
    pub(crate) gpio7: esp_hal::peripherals::GPIO7<'static>,
    pub(crate) gpio8: esp_hal::peripherals::GPIO8<'static>,
    pub(crate) gpio10: esp_hal::peripherals::GPIO10<'static>,
    #[allow(dead_code)]
    pub(crate) gpio12: esp_hal::peripherals::GPIO12<'static>,
    pub(crate) gpio21: esp_hal::peripherals::GPIO21<'static>,
    pub(crate) gpio4: esp_hal::peripherals::GPIO4<'static>,
    pub(crate) gpio5: esp_hal::peripherals::GPIO5<'static>,
    pub(crate) gpio6: esp_hal::peripherals::GPIO6<'static>,
    #[cfg(feature = "diagnostic-console")]
    pub(crate) usb_device: esp_hal::peripherals::USB_DEVICE<'static>,
    #[cfg(feature = "diagnostic-console")]
    pub(crate) flash: esp_hal::peripherals::FLASH<'static>,
}

pub(crate) async fn run(spawner: Spawner, peripherals: RuntimePeripherals) {
    use static_cell::StaticCell;

    use binbook_fw::board::SharedSpi2;

    static SPI2_BUS: StaticCell<SharedSpi2> = StaticCell::new();
    let shared_spi2 = SPI2_BUS.init(SharedSpi2::new(
        peripherals.spi2,
        peripherals.gpio8,
        peripherals.gpio10,
        peripherals.gpio7,
    ));

    spawner.spawn(
        input_task::input_task(
            peripherals.adc1,
            peripherals.gpio1,
            peripherals.gpio2,
            REQUEST_CHANNEL.sender(),
            peripherals.gpio3,
            peripherals.lpwr,
            POWER_REQUEST_CHANNEL.sender(),
            POWER_ACK_CHANNEL.receiver(),
        )
        .unwrap(),
    );
    #[cfg(feature = "diagnostic-console")]
    spawner.spawn(diagnostic_aggregator::runtime_event_aggregator_task().unwrap());
    #[cfg(feature = "diagnostic-console")]
    RUNTIME_EVENT_CHANNEL
        .sender()
        .send(RuntimeEvent {
            timestamp_ms: embassy_time::Instant::now().as_millis(),
            kind: RuntimeEventKind::WakeCause {
                cause: if matches!(
                    esp_hal::system::wakeup_cause(),
                    esp_hal::system::SleepSource::Gpio
                ) {
                    1
                } else {
                    0
                },
            },
        })
        .await;

    #[cfg(all(feature = "diagnostic-console", not(feature = "sd-storage")))]
    spawner.spawn(
        diagnostic_console::diagnostic_task(peripherals.usb_device, peripherals.flash).unwrap(),
    );

    #[cfg(all(feature = "mounted-storage-owner", feature = "sd-storage"))]
    {
        use binbook_fw::board::{DisplayDelay, FreqManagedSpiDevice};
        use binbook_fw::storage::{FixedTime, SdFilesystem};
        use esp_hal::gpio::{Level, Output, OutputConfig};

        let sd_cs = Output::new(peripherals.gpio12, Level::High, OutputConfig::default());
        let sd_spi = FreqManagedSpiDevice::new(shared_spi2, sd_cs, SD_SPI_FREQ_HZ);
        #[cfg(feature = "diagnostic-console")]
        let sd_spi = binbook_fw::sd_spi_trace::TracedSpiDevice::new(sd_spi, RuntimeSdSpiObserver);
        let sd_fs =
            SdFilesystem::new_measured(sd_spi, DisplayDelay, FixedTime, RuntimeBlockIoObserver);
        spawner.spawn(
            storage_owner_task::mounted_storage_owner_task(
                sd_fs,
                STORAGE_REQUEST_CHANNEL.receiver(),
            )
            .unwrap(),
        );
        use static_cell::StaticCell;
        static ACTIVE_VIEWPORT: StaticCell<binbook_fw::library::LibraryViewport> =
            StaticCell::new();
        static REFRESH_WORKSPACE: StaticCell<
            binbook_fw::library_external_sort::LibraryRefreshWorkspace,
        > = StaticCell::new();
        let active_viewport = ACTIVE_VIEWPORT.init(binbook_fw::library::LibraryViewport::empty());
        let refresh_workspace = REFRESH_WORKSPACE
            .init(binbook_fw::library_external_sort::LibraryRefreshWorkspace::new());
        spawner.spawn(
            library_refresh_task::library_refresh_task(
                refresh_workspace,
                LIBRARY_REFRESH_TRIGGER_CHANNEL.receiver(),
                LIBRARY_REFRESH_COMPLETION_CHANNEL.sender(),
            )
            .unwrap(),
        );
        spawner.spawn(
            mounted_display_task::mounted_display_task(
                shared_spi2,
                peripherals.gpio21,
                peripherals.gpio4,
                peripherals.gpio5,
                peripherals.gpio6,
                REQUEST_CHANNEL.receiver(),
                POWER_REQUEST_CHANNEL.receiver(),
                POWER_ACK_CHANNEL.sender(),
                active_viewport,
            )
            .unwrap(),
        );
        #[cfg(feature = "diagnostic-console")]
        spawner.spawn(
            diagnostic_console::diagnostic_task(peripherals.usb_device, peripherals.flash).unwrap(),
        );
        core::future::pending::<()>().await;
        return;
    }

    #[cfg(not(feature = "mounted-storage-owner"))]
    {
        // -----------------------------------------------------------------------
        // SD card boot mount
        // -----------------------------------------------------------------------
        // Attempt to mount the SD card, enumerate .binbook files, and log the
        // count.  If the card is absent or unreadable the firmware continues
        // (display-only mode).  The SdFilesystem handle is kept alive in a static
        // for B/C to consume; no reader/menu exists yet (those are sub-project
        // C).
        // -----------------------------------------------------------------------
        #[cfg(feature = "sd-storage")]
        let sd_handle = {
            use esp_hal::gpio::{Level, Output, OutputConfig};
            use static_cell::StaticCell;

            use binbook_fw::board::{DisplayDelay, FreqManagedSpiDevice};
            use binbook_fw::library::{Library, ScanErrorKind};
            use binbook_fw::storage::{FixedTime, SdFilesystem};
            use binbook_fw::storage_access::{LibraryIndexStore, SharedSdFilesystem};

            static SD_FS: StaticCell<SharedSdFilesystem<RuntimeSdFilesystem>> = StaticCell::new();

            let sd_cs = Output::new(peripherals.gpio12, Level::High, OutputConfig::default());
            let sd_spi = FreqManagedSpiDevice::new(shared_spi2, sd_cs, SD_SPI_FREQ_HZ);
            #[cfg(feature = "diagnostic-console")]
            let sd_spi =
                binbook_fw::sd_spi_trace::TracedSpiDevice::new(sd_spi, RuntimeSdSpiObserver);
            let sd_fs = SD_FS.init(SharedSdFilesystem::new(SdFilesystem::new_measured(
                sd_spi,
                DisplayDelay,
                FixedTime,
                RuntimeBlockIoObserver,
            )));
            let mut sd_handle = sd_fs.handle();

            static LIBRARY: StaticCell<Library> = StaticCell::new();
            let library = LIBRARY.init(Library::empty());
            let mut library_index = LibraryIndexStore::new(sd_handle);
            if library_index.load_into(library).is_err() {
                library.scan_in_place(&mut sd_handle, |error| match error {
                    binbook_fw::storage::SdError::NoCard => ScanErrorKind::NoCard,
                    _ => ScanErrorKind::CardError,
                });
                if matches!(
                    library.status(),
                    binbook_fw::library::LibraryStatus::Ready
                        | binbook_fw::library::LibraryStatus::NoBooks
                ) {
                    let _ = library_index.save(library);
                }
            }
            LIBRARY_STATUS.store(library.status().code(), portable_atomic::Ordering::Relaxed);

            spawner.spawn(
                storage_owner_task::storage_owner_task(
                    sd_handle,
                    STORAGE_REQUEST_CHANNEL.receiver(),
                )
                .unwrap(),
            );

            #[cfg(feature = "storage-owner-probe")]
            if let Some(entry) = library.entries().first() {
                if let Ok(name) = binbook_fw::storage_owner::StorageName::new(entry.name.as_str()) {
                    spawner.spawn(storage_owner_task::storage_owner_probe_task(name).unwrap());
                }
            }

            #[cfg(all(feature = "diagnostic-console", target_arch = "riscv32"))]
            spawner.spawn(
                diagnostic_console::diagnostic_task_with_sd(
                    peripherals.usb_device,
                    peripherals.flash,
                    sd_handle,
                )
                .unwrap(),
            );

            (sd_handle, library)
        };

        spawner.spawn(
            display_task::display_task(
                shared_spi2,
                peripherals.gpio21,
                peripherals.gpio4,
                peripherals.gpio5,
                peripherals.gpio6,
                REQUEST_CHANNEL.receiver(),
                POWER_REQUEST_CHANNEL.receiver(),
                POWER_ACK_CHANNEL.sender(),
                #[cfg(feature = "sd-storage")]
                sd_handle.0,
                #[cfg(feature = "sd-storage")]
                sd_handle.1,
            )
            .unwrap(),
        );
    }
}
