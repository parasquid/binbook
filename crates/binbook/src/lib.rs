mod args;
mod atomic_output;
mod bulk_readback;
#[cfg(feature = "serial-device")]
mod bulk_readback_io;
#[cfg(feature = "serial-device")]
pub mod bulk_readback_stream;
#[cfg(feature = "serial-device")]
mod bulk_readback_transport;
#[cfg(feature = "serial-device")]
pub mod bulk_serial;
#[cfg(feature = "serial-device")]
mod bulk_serial_readback;
pub mod bulk_state;
#[cfg(feature = "serial-device")]
pub mod bulk_upload;
mod decode;
pub mod diag_protocol;
mod diag_response;
mod encode;
mod error;
pub mod frame_router;
#[cfg(feature = "serial-device")]
mod host_transfer_metrics;
mod input;
mod inspect;
pub mod nav_burst;
pub mod page_turn_cadence;
pub mod partial_window_busy;
#[cfg(feature = "serial-device")]
pub mod progress;
pub mod protocol;
#[cfg(feature = "serial-device")]
pub mod sd_write_trace_client;
pub mod transfer_negotiation;
#[cfg(feature = "serial-device")]
pub mod transfer_probe;
#[cfg(feature = "serial-device")]
pub mod transfer_report;
#[cfg(feature = "serial-device")]
mod transfer_trace_client;

#[cfg(feature = "serial-device")]
pub mod diag_cli;
#[cfg(feature = "serial-device")]
pub mod exercise;
#[cfg(feature = "serial-device")]
mod exercise_decode;
#[cfg(feature = "serial-device")]
mod exercise_validation;
#[cfg(feature = "serial-device")]
pub mod serial_transport;
#[cfg(feature = "serial-device")]
pub mod storage_readback;
#[cfg(feature = "serial-device")]
pub mod upload;
#[cfg(feature = "serial-device")]
pub mod upload_command;
#[cfg(feature = "serial-device")]
mod upload_library;
#[cfg(feature = "serial-device")]
mod upload_protocol;
#[cfg(feature = "serial-device")]
mod upload_serial_batch;
#[cfg(feature = "serial-device")]
mod upload_source;
#[cfg(feature = "serial-device")]
mod upload_types;

pub use args::{
    Cli, Commands, DiagCommand, ExerciseCommand, FontChoice, InputFormat, PageAction, PixelFormat,
    ProbeCommand, Profile, ProgressMode, RefreshModeAction, RefreshModeChoice,
    RequestedTransferMode, RequestedTransferWindow, StorageCommand, UploadArgs,
};
pub use decode::run_decode;
pub use encode::run_encode;
pub use error::CliError;
pub use inspect::run_inspect;

pub const DISPLAY_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(70);
