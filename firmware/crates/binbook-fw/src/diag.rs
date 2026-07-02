pub mod command;
mod log_response;
pub mod runtime;
mod serial;
pub mod state;

pub use command::DisplayProbeKind;
pub use command::{dispatch_command, keycode_to_button, CommandContext, DispatchResult};
pub use log_response::{resolve_log_clear, resolve_log_get};
pub use runtime::{
    complete_pending_command, poll_pending_command, poll_runtime_command, queue_runtime_response,
    RuntimeCommand,
};
pub use serial::{FeedResult, SerialState, TransportError};
pub use state::{
    DiagnosticLoopState, DiagnosticPendingQueue, DiagnosticSnapshot, PendingAction, PendingCommand,
};
