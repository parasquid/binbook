#![no_std]
#![no_main]

use binbook_fw::board::BoardSpiDevice;
use embedded_hal::spi::SpiBus;
use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    spi::{master::Config as SpiConfig, master::Spi, Mode},
    time::Rate,
};
use esp_println::println;

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let delay = Delay::new();
    let miso_input = Input::new(
        peripherals.GPIO7,
        InputConfig::default().with_pull(Pull::Down),
    );
    delay.delay_millis(5);
    println!(
        "SD_PROBE miso_bias pull=down level={:?}",
        miso_input.level()
    );
    let mut miso = miso_input.into_flex();
    miso.apply_input_config(&InputConfig::default().with_pull(Pull::Up));
    delay.delay_millis(5);
    println!("SD_PROBE miso_bias pull=up level={:?}", miso.level());
    miso.apply_input_config(&InputConfig::default());
    let miso_signal = miso.peripheral_input();
    let mut spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_khz(100))
            .with_mode(Mode::_0),
    )
    .expect("SPI2 init")
    .with_sck(peripherals.GPIO8)
    .with_mosi(peripherals.GPIO10)
    .with_miso(miso_signal);
    let mut cs = Output::new(peripherals.GPIO12, Level::High, OutputConfig::default());

    let mut idle_clocks = [0xff; 256];
    cs.set_high();
    SpiBus::transfer_in_place(&mut spi, &mut idle_clocks).expect("idle clocks");
    delay.delay_millis(1);

    for attempt in 0..8 {
        let mut command = [0x40, 0, 0, 0, 0, 0x95];
        let mut response = [0xff; 16];
        cs.set_low();
        SpiBus::transfer_in_place(&mut spi, &mut command).expect("CMD0");
        SpiBus::transfer_in_place(&mut spi, &mut response).expect("CMD0 response");
        cs.set_high();
        println!(
            "SD_PROBE raw_cmd0 attempt={} command_rx={:02x?} response={:02x?}",
            attempt, command, response
        );
        let mut release = [0xff; 2];
        SpiBus::transfer_in_place(&mut spi, &mut release).expect("release clocks");
        delay.delay_millis(10);
    }

    let device = BoardSpiDevice::new(spi, cs);
    let card = embedded_sdmmc::SdCard::new(device, Delay::new());

    println!("SD_PROBE start frequency_hz=100000 pins=sck8,mosi10,miso7,cs12");
    match card.num_bytes() {
        Ok(bytes) => println!("SD_PROBE acquired bytes={}", bytes),
        Err(error) => println!("SD_PROBE failed error={:?}", error),
    }

    loop {
        Delay::new().delay_millis(1_000);
    }
}
