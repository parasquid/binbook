use binbook_diagnostic_protocol::KeyCode;

#[cfg(feature = "serial-device")]
#[path = "page_turn_cadence_runtime.rs"]
mod runtime;
#[cfg(feature = "serial-device")]
pub use runtime::{run_page_turn_cadence, run_page_turn_cadence_io, PageTurnCadenceOptions};

pub fn key_name_to_code(name: &str) -> Option<KeyCode> {
    match name {
        "LEFT" => Some(KeyCode::Left),
        "RIGHT" => Some(KeyCode::Right),
        "UP" => Some(KeyCode::Up),
        "DOWN" => Some(KeyCode::Down),
        _ => None,
    }
}
