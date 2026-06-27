#![no_std]

pub mod book;
pub mod display;
pub mod flash;
pub mod input;
pub mod refresh;

#[cfg(feature = "diagnostic-console")]
pub mod diag;
#[cfg(feature = "diagnostic-console")]
pub mod diag_flash;
#[cfg(feature = "diagnostic-console")]
pub mod diag_log;
