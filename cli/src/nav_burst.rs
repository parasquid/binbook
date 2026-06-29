use binbook_diagnostic_protocol::KeyCode;

#[cfg(feature = "serial-device")]
#[path = "nav_burst_runtime.rs"]
mod runtime;
#[cfg(feature = "serial-device")]
pub use runtime::{run_nav_burst, run_nav_burst_io, NavBurstOptions};

pub const INTERIOR_BURST: [KeyCode; 16] = [
    KeyCode::Down,
    KeyCode::Down,
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Down,
    KeyCode::Up,
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Down,
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Up,
    KeyCode::Up,
    KeyCode::Down,
];

pub const INTERIOR_EXPECTED: [u32; 16] =
    [9, 10, 9, 10, 11, 10, 9, 10, 9, 10, 11, 10, 11, 10, 9, 10];

pub const BOUNDARY_BURST: [KeyCode; 5] = [
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Up,
    KeyCode::Up,
    KeyCode::Down,
];

pub fn expected_pages(start: u32, page_count: u32, keys: &[KeyCode]) -> Vec<u32> {
    let mut current = start;
    keys.iter()
        .map(|key| {
            current = match key {
                KeyCode::Left | KeyCode::Up => current.saturating_sub(1),
                KeyCode::Right | KeyCode::Down => {
                    current.saturating_add(1).min(page_count.saturating_sub(1))
                }
                KeyCode::Select | KeyCode::Back | KeyCode::Power => current,
            };
            current
        })
        .collect()
}
