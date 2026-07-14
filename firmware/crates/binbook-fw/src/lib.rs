#![no_std]

pub extern crate heapless;
pub extern crate xteink_x4_display;

pub mod async_refresh;
pub mod board;
pub mod book;
pub mod crc32;
pub mod error;
pub mod flash;
pub mod input;
pub mod library;
#[cfg(feature = "sd-storage")]
pub mod library_index_load;
#[cfg(feature = "sd-storage")]
pub mod library_index_save;
#[cfg(feature = "sd-storage")]
pub mod library_refresh;
pub mod menu;
pub mod menu_render;
#[cfg(feature = "sd-storage")]
pub mod owner_book;
pub mod owner_reader;
pub mod power;
pub mod reader_state;
pub mod runtime_engine;
#[cfg(feature = "sd-storage")]
pub mod state_owner;
pub mod storage_access;
pub mod storage_owner;

#[cfg(feature = "diagnostic-console")]
pub mod runtime_aggregator;

#[cfg(feature = "sd-storage")]
pub mod storage;

#[cfg(feature = "diagnostic-console")]
pub mod diag;
#[cfg(feature = "diagnostic-console")]
pub mod diag_flash;
#[cfg(feature = "diagnostic-console")]
pub mod diag_log;
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
pub mod diag_owner_storage;
#[cfg(feature = "diagnostic-console")]
pub mod diag_storage;
