#[cfg(feature = "serial-device")]
#[path = "partial_window_busy_runtime.rs"]
mod runtime;

#[cfg(feature = "serial-device")]
pub use runtime::{run_partial_window_busy, run_partial_window_busy_io, PartialWindowBusyOptions};
