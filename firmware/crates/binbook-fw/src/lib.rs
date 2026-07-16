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
#[cfg(feature = "diagnostic-console")]
pub mod legacy_transfer_trace;
pub mod library;
#[cfg(feature = "sd-storage")]
pub mod library_external_sort;
#[cfg(feature = "sd-storage")]
pub mod library_index_load;
#[cfg(feature = "sd-storage")]
pub mod library_index_save;
#[cfg(feature = "sd-storage")]
pub mod library_index_select;
#[cfg(feature = "sd-storage")]
pub mod library_publication;
#[cfg(feature = "sd-storage")]
pub mod library_refresh;
pub mod library_refresh_barrier;
#[cfg(feature = "sd-storage")]
pub mod library_run_generation;
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

pub mod bulk_idle_timeout;
#[cfg(feature = "diagnostic-console")]
pub mod bulk_lease_pool;
#[cfg(feature = "diagnostic-console")]
pub mod bulk_protocol_dispatch;
#[cfg(feature = "diagnostic-console")]
pub mod bulk_readback_pipeline;
#[cfg(feature = "diagnostic-console")]
pub mod bulk_readback_runtime;
#[cfg(feature = "diagnostic-console")]
mod bulk_readback_runtime_control;
#[cfg(feature = "diagnostic-console")]
mod bulk_readback_runtime_types;
#[cfg(feature = "diagnostic-console")]
mod bulk_readback_types;
#[cfg(feature = "diagnostic-console")]
pub mod bulk_reply_tracker;
#[cfg(feature = "diagnostic-console")]
mod bulk_slot_types;
#[cfg(feature = "diagnostic-console")]
pub mod bulk_slots;
#[cfg(feature = "diagnostic-console")]
mod bulk_transfer_control;
#[cfg(feature = "diagnostic-console")]
pub mod bulk_transfer_coordinator;
#[cfg(feature = "diagnostic-console")]
mod bulk_transfer_types;
#[cfg(feature = "diagnostic-console")]
pub mod bulk_upload_pipeline;
#[cfg(feature = "diagnostic-console")]
pub mod bulk_upload_runtime;
#[cfg(feature = "diagnostic-console")]
mod bulk_upload_runtime_control;
#[cfg(feature = "diagnostic-console")]
mod bulk_upload_runtime_types;
#[cfg(feature = "diagnostic-console")]
mod bulk_upload_types;
#[cfg(feature = "diagnostic-console")]
pub mod runtime_aggregator;
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
pub mod sd_spi_trace;
#[cfg(feature = "diagnostic-console")]
#[cfg(feature = "sd-storage")]
pub mod transfer_block_io;
#[cfg(feature = "diagnostic-console")]
pub mod transfer_metrics;
#[cfg(feature = "diagnostic-console")]
mod transfer_metrics_snapshot;
#[cfg(feature = "diagnostic-console")]
pub mod transfer_owner_timing;
#[cfg(feature = "diagnostic-console")]
pub mod transfer_trace_command;
#[cfg(feature = "diagnostic-console")]
pub mod transfer_trace_store;

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
