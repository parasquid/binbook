#![cfg_attr(target_arch = "riscv32", no_std)]
#![cfg_attr(target_arch = "riscv32", no_main)]

#[cfg(target_arch = "riscv32")]
use embassy_executor::Spawner;
#[cfg(target_arch = "riscv32")]
use esp_backtrace as _;

#[cfg(all(
    target_arch = "riscv32",
    feature = "debug-log",
    not(feature = "diagnostic-console")
))]
use esp_println::println;

#[cfg(all(
    target_arch = "riscv32",
    feature = "debug-log",
    not(feature = "diagnostic-console")
))]
macro_rules! dbgprintln {
    ($($arg:tt)*) => { println!($($arg)*) };
}
#[cfg(all(
    target_arch = "riscv32",
    not(all(feature = "debug-log", not(feature = "diagnostic-console")))
))]
macro_rules! dbgprintln {
    ($($arg:tt)*) => {};
}

#[cfg(target_arch = "riscv32")]
esp_bootloader_esp_idf::esp_app_desc!();

#[cfg(target_arch = "riscv32")]
mod runtime;

#[cfg(target_arch = "riscv32")]
const DISPLAY_SPI_FREQUENCY_MHZ: u32 = 20;
#[cfg(target_arch = "riscv32")]
const PROBE_BOOK: &[u8] = include_bytes!("../fixtures/nav_probe.binbook");
#[cfg(target_arch = "riscv32")]
const BINBOOK_SCRATCH_BYTES: usize = 8192;

#[cfg(target_arch = "riscv32")]
#[embassy_executor::task]
async fn firmware_task(peripherals: runtime::RuntimePeripherals, spawner: Spawner) {
    dbgprintln!("[BOOT] starting firmware tasks");
    runtime::run(spawner, peripherals).await;
}

#[cfg(target_arch = "riscv32")]
#[esp_hal::main]
fn main() -> ! {
    let esp_hal::peripherals::Peripherals {
        ADC1,
        GPIO1,
        GPIO2,
        SPI2,
        GPIO8,
        GPIO10,
        GPIO21,
        GPIO4,
        GPIO5,
        GPIO6,
        #[cfg(feature = "diagnostic-console")]
        USB_DEVICE,
        #[cfg(feature = "diagnostic-console")]
        FLASH,
        TIMG0,
        SW_INTERRUPT,
        ..
    } = esp_hal::init(esp_hal::Config::default());

    let timer = esp_hal::timer::timg::TimerGroup::new(TIMG0);
    let software_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(SW_INTERRUPT);
    esp_rtos::start(timer.timer0, software_interrupt.software_interrupt0);

    let peripherals = runtime::RuntimePeripherals {
        adc1: ADC1,
        gpio1: GPIO1,
        gpio2: GPIO2,
        spi2: SPI2,
        gpio8: GPIO8,
        gpio10: GPIO10,
        gpio21: GPIO21,
        gpio4: GPIO4,
        gpio5: GPIO5,
        gpio6: GPIO6,
        #[cfg(feature = "diagnostic-console")]
        usb_device: USB_DEVICE,
        #[cfg(feature = "diagnostic-console")]
        flash: FLASH,
    };

    static EXECUTOR: static_cell::StaticCell<esp_rtos::embassy::Executor> =
        static_cell::StaticCell::new();
    let executor = EXECUTOR.init(esp_rtos::embassy::Executor::new());
    executor.run(move |spawner| {
        spawner.spawn(firmware_task(peripherals, spawner).expect("failed to create firmware task"));
    })
}

#[cfg(not(target_arch = "riscv32"))]
fn main() {}
