//! Generic SPI-mode SD + FAT16/32 storage over `embedded-hal`.
//!
//! Wraps `embedded-sdmmc`. Takes an `SpiDevice<u8>` (CS-managed, shareable via
//! `embedded-hal-bus`) + a `DelayNs`. Knows nothing about X4 pins or BinBook.

#![no_std]

pub mod sd_filesystem;
pub use sd_filesystem::SdStorage;
