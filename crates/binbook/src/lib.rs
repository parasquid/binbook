mod args;
mod atomic_output;
mod decode;
pub mod diag_protocol;
mod diag_response;
mod encode;
mod error;
mod input;
mod inspect;
pub mod nav_burst;
pub mod page_turn_cadence;
pub mod partial_window_busy;
#[cfg(feature = "serial-device")]
pub mod progress;
pub mod protocol;

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
    ProbeCommand, Profile, ProgressMode, RefreshModeAction, RefreshModeChoice, StorageCommand,
    UploadArgs,
};
pub use decode::run_decode;
pub use encode::run_encode;
pub use error::CliError;
pub use inspect::run_inspect;

pub const DISPLAY_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(70);
