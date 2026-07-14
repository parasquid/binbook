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

#[cfg(feature = "diagnostic-console")]
mod diagnostic_aggregator;
#[cfg(feature = "diagnostic-console")]
mod diagnostic_console;
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
mod diagnostic_owner_storage;

use embassy_executor::Spawner;
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
    binbook_fw::board::FreqManagedSpiDevice<'static, esp_hal::gpio::Output<'static>>,
    binbook_fw::board::DisplayDelay,
    binbook_fw::storage::FixedTime,
>;

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
    prepared: heapless::Vec<
        (
            binbook_fw::storage_owner::StorageName,
            binbook_fw::library::MountFileToken,
        ),
        { binbook_fw::library::LIBRARY_VIEWPORT_CAPACITY },
    >,
}
#[cfg(all(feature = "sd-storage", feature = "mounted-storage-owner"))]
struct LibraryRefreshTrigger {
    prefetch: heapless::Vec<
        binbook_fw::storage_owner::StorageName,
        { binbook_fw::library::LIBRARY_VIEWPORT_CAPACITY },
    >,
}
#[cfg(all(feature = "sd-storage", feature = "mounted-storage-owner"))]
static LIBRARY_REFRESH_TRIGGER_CHANNEL: Channel<CriticalSectionRawMutex, LibraryRefreshTrigger, 1> =
    Channel::new();
#[cfg(all(feature = "sd-storage", feature = "mounted-storage-owner"))]
static LIBRARY_REFRESH_COMPLETION_CHANNEL: Channel<
    CriticalSectionRawMutex,
    LibraryRefreshCompletion,
    1,
> = Channel::new();

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
        let sd_fs = SdFilesystem::new(sd_spi, DisplayDelay, FixedTime);
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
        static REFRESH_LIBRARY: StaticCell<binbook_fw::library::Library> = StaticCell::new();
        let active_viewport = ACTIVE_VIEWPORT.init(binbook_fw::library::LibraryViewport::empty());
        let refresh_library = REFRESH_LIBRARY.init(binbook_fw::library::Library::empty());
        spawner.spawn(
            library_refresh_task::library_refresh_task(
                refresh_library,
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
            let sd_fs = SD_FS.init(SharedSdFilesystem::new(SdFilesystem::new(
                sd_spi,
                DisplayDelay,
                FixedTime,
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
