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
use portable_atomic::AtomicU32;

use binbook_fw::async_refresh::{DisplayRequest, PAGE_TURN_QUEUE_CAPACITY};
use binbook_fw::runtime_engine::RuntimeEvent;

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
    pub(crate) gpio8: esp_hal::peripherals::GPIO8<'static>,
    pub(crate) gpio10: esp_hal::peripherals::GPIO10<'static>,
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
            peripherals.spi2,
            peripherals.gpio8,
            peripherals.gpio10,
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
}
