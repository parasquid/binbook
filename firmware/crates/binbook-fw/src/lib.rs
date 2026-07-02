#![no_std]

pub extern crate heapless;
pub extern crate xteink_x4_display;

pub mod async_refresh;
pub mod board;
pub mod book;
pub mod error;
pub mod flash;
pub mod input;
pub mod menu;
pub mod resume;
pub mod runtime_engine;

#[cfg(feature = "diagnostic-console")]
pub mod runtime_aggregator;

#[cfg(all(feature = "sd-storage", target_arch = "riscv32"))]
pub mod storage;

#[cfg(feature = "diagnostic-console")]
pub mod diag;
#[cfg(feature = "diagnostic-console")]
pub mod diag_flash;
#[cfg(feature = "diagnostic-console")]
pub mod diag_log;
#[cfg(feature = "diagnostic-console")]
pub mod diag_storage;
