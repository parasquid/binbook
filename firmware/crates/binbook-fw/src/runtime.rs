mod display_backend;
mod display_task;
mod input_task;

#[cfg(feature = "diagnostic-console")]
mod diagnostic_aggregator;
#[cfg(feature = "diagnostic-console")]
mod diagnostic_console;

use embassy_executor::Spawner;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Receiver, Sender},
};
use portable_atomic::{AtomicU32, AtomicU8};

use binbook_fw::async_refresh::{DisplayRequest, PAGE_TURN_QUEUE_CAPACITY};
use binbook_fw::runtime_engine::RuntimeEvent;

static DISPLAY_MODE: AtomicU8 = AtomicU8::new(0);

#[cfg(feature = "sd-storage")]
use embassy_sync::mutex::Mutex;
#[cfg(feature = "sd-storage")]
static MENU_BOOK_NAMES: Mutex<CriticalSectionRawMutex, binbook_fw::heapless::Vec<binbook_fw::menu::MenuName, 200>> =
    Mutex::new(binbook_fw::heapless::Vec::new());

#[cfg(all(feature = "firmware-bin", target_arch = "riscv32"))]
#[expect(dead_code)]
const RESUME_FLASH_OFFSET: u32 = 0x00FC_FF00;

type RequestSender =
    Sender<'static, CriticalSectionRawMutex, DisplayRequest, { PAGE_TURN_QUEUE_CAPACITY }>;
type RequestReceiver =
    Receiver<'static, CriticalSectionRawMutex, DisplayRequest, { PAGE_TURN_QUEUE_CAPACITY }>;

static REQUEST_CHANNEL: Channel<
    CriticalSectionRawMutex,
    DisplayRequest,
    { PAGE_TURN_QUEUE_CAPACITY },
> = Channel::new();
static RUNTIME_EVENT_CHANNEL: Channel<CriticalSectionRawMutex, RuntimeEvent, 32> = Channel::new();
static REQUEST_EPOCH: AtomicU32 = AtomicU32::new(0);

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
        )
        .unwrap(),
    );
    spawner.spawn(
        display_task::display_task(
            shared_spi2,
            peripherals.gpio21,
            peripherals.gpio4,
            peripherals.gpio5,
            peripherals.gpio6,
            REQUEST_CHANNEL.receiver(),
        )
        .unwrap(),
    );

    #[cfg(feature = "diagnostic-console")]
    spawner.spawn(diagnostic_aggregator::runtime_event_aggregator_task().unwrap());

    #[cfg(feature = "diagnostic-console")]
    spawner.spawn(
        diagnostic_console::diagnostic_task(peripherals.usb_device, peripherals.flash).unwrap(),
    );

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
    {
        use esp_hal::gpio::{Level, Output, OutputConfig};
        use static_cell::StaticCell;

        use binbook_fw::board::{DisplayDelay, FreqManagedSpiDevice};
        use binbook_fw::storage::{FixedTime, SdFilesystem};
        use binbook_fw::menu::MenuName;
        use binbook_storage::filesystem::Filesystem;

        static SD_FS: StaticCell<SdFilesystem<
            FreqManagedSpiDevice<'static, esp_hal::gpio::Output<'static>>,
            DisplayDelay,
            FixedTime,
        >> = StaticCell::new();

        let sd_cs = Output::new(peripherals.gpio12, Level::High, OutputConfig::default());
        let sd_spi = FreqManagedSpiDevice::new(shared_spi2, sd_cs, 400_000);
        let mut sd_fs = SD_FS.init(SdFilesystem::new(
            sd_spi,
            DisplayDelay,
            FixedTime,
        ));

        let mut book_names = MENU_BOOK_NAMES.lock().await;
        let mut found_books = false;

        match sd_fs.for_each_entry(&mut |name, _size| {
            if name.ends_with(".binbook") {
                if let Ok(name_buf) = MenuName::try_from(name) {
                    let _ = book_names.push(name_buf);
                    found_books = true;
                }
            }
        }) {
            Ok(()) => {
                #[cfg(feature = "debug-log")]
                esp_println::println!("[SD] enumerated {} .binbook files", book_names.len());
            }
            Err(_) => {
                #[cfg(feature = "debug-log")]
                esp_println::println!("[SD] mount failed — no card?");
            }
        }

        if !found_books {
            let _ = book_names.push(MenuName::try_from("nav_probe.binbook").unwrap());
            #[cfg(feature = "debug-log")]
            esp_println::println!("[SD] no books — using embedded nav_probe fallback");
        }

        #[cfg(feature = "firmware-bin")]
        {
            use esp_hal::peripherals::FLASH;
            use binbook_fw::resume::ResumeStorage;

            static_cell::const_static!(
                FLASH_CELL,
                FLASH,
                unsafe { FLASH::steal() }
            );
            let mut flash_storage = ResumeStorage::new(FLASH_CELL.get_mut(), RESUME_FLASH_OFFSET);

            match flash_storage.read() {
                Ok(Some(resume)) if !resume.is_empty() => {
                    let book_name = core::str::from_utf8(&resume.last_book_name)
                        .ok()
                        .and_then(|s| s.trim_end_matches('\0').into())
                        .unwrap_or("nav_probe.binbook");

                    if book_names.iter().any(|name| name.as_str() == book_name) {
                        DISPLAY_MODE.store(1, portable_atomic::Ordering::Relaxed);
                        #[cfg(feature = "debug-log")]
                        esp_println::println!("[RESUME] restoring reading mode: book={}, page={}", book_name, resume.last_page);
                    } else {
                        DISPLAY_MODE.store(0, portable_atomic::Ordering::Relaxed);
                        #[cfg(feature = "debug-log")]
                        esp_println!("[RESUME] book not found, restoring menu mode");
                    }
                }
                _ => {
                    DISPLAY_MODE.store(0, portable_atomic::Ordering::Relaxed);
                    #[cfg(feature = "debug-log")]
                    esp_println!("[RESUME] no valid resume state, starting in menu mode");
                }
            }
        }
    }
}
