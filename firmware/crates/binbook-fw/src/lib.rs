#![no_std]

pub mod async_refresh;
pub mod board;
pub mod book;
pub mod error;
pub mod flash;
pub mod input;
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
