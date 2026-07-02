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
pub mod protocol;

#[cfg(feature = "serial-device")]
pub mod exercise;
#[cfg(feature = "serial-device")]
mod exercise_decode;
#[cfg(feature = "serial-device")]
mod exercise_validation;
#[cfg(feature = "serial-device")]
pub mod serial_transport;

pub use args::{
    Cli, Commands, DiagCommand, ExerciseCommand, FontChoice, InputFormat, PageAction, PixelFormat,
    ProbeCommand, Profile, StorageCommand,
};
pub use decode::run_decode;
pub use encode::run_encode;
pub use error::CliError;
pub use inspect::run_inspect;

pub const DISPLAY_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(70);
