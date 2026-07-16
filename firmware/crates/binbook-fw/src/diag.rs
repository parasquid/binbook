pub mod command;
mod log_response;
pub mod runtime;
mod serial;
pub mod state;

pub use command::DisplayProbeKind;
pub use command::{dispatch_command, keycode_to_button, CommandContext, DispatchResult};
pub use log_response::{resolve_log_clear, resolve_log_get};
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
pub use runtime::poll_runtime_command_bulk_owner_storage;
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
pub use runtime::poll_runtime_command_bulk_owner_storage_profiled;
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
pub use runtime::poll_runtime_command_owner_storage;
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
pub use runtime::poll_runtime_command_owner_storage_profiled;
#[cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]
pub use runtime::RuntimeProfileEvent;
pub use runtime::{
    complete_pending_command, poll_pending_command, poll_runtime_command, queue_runtime_event,
    queue_runtime_response, RuntimeCommand,
};
pub use serial::{FeedResult, SerialState, TransportError};
pub use state::{
    DiagnosticLoopState, DiagnosticPendingQueue, DiagnosticSnapshot, PendingAction, PendingCommand,
};
