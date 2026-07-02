use super::*;

#[test]
fn menu_up_maps_to_prev() {
    assert_eq!(
        button_to_request(Button::Up, MODE_MENU),
        Some(DisplayRequest::MenuPrev)
    );
}

#[test]
fn menu_left_maps_to_prev() {
    assert_eq!(
        button_to_request(Button::Left, MODE_MENU),
        Some(DisplayRequest::MenuPrev)
    );
}

#[test]
fn menu_down_maps_to_next() {
    assert_eq!(
        button_to_request(Button::Down, MODE_MENU),
        Some(DisplayRequest::MenuNext)
    );
}

#[test]
fn menu_right_maps_to_next() {
    assert_eq!(
        button_to_request(Button::Right, MODE_MENU),
        Some(DisplayRequest::MenuNext)
    );
}

#[test]
fn menu_select_maps_to_select() {
    assert_eq!(
        button_to_request(Button::Select, MODE_MENU),
        Some(DisplayRequest::MenuSelect)
    );
}

#[test]
fn menu_back_and_power_are_noop() {
    assert_eq!(button_to_request(Button::Back, MODE_MENU), None);
    assert_eq!(button_to_request(Button::Power, MODE_MENU), None);
}

#[test]
fn reading_up_maps_to_previous_turn() {
    assert_eq!(
        button_to_request(Button::Up, MODE_READING),
        Some(DisplayRequest::Turn {
            turn: PageTurn::Previous,
            completion_sequence: None,
        })
    );
}

#[test]
fn reading_down_maps_to_next_turn() {
    assert_eq!(
        button_to_request(Button::Down, MODE_READING),
        Some(DisplayRequest::Turn {
            turn: PageTurn::Next,
            completion_sequence: None,
        })
    );
}

#[test]
#[cfg(not(feature = "sd-storage"))]
fn startup_up_and_down_map_to_page_turns_without_library_menu() {
    assert_eq!(
        button_to_request(Button::Up, STARTUP_DISPLAY_MODE),
        Some(DisplayRequest::Turn {
            turn: PageTurn::Previous,
            completion_sequence: None,
        })
    );
    assert_eq!(
        button_to_request(Button::Down, STARTUP_DISPLAY_MODE),
        Some(DisplayRequest::Turn {
            turn: PageTurn::Next,
            completion_sequence: None,
        })
    );
}

#[test]
#[cfg(feature = "sd-storage")]
fn startup_up_and_down_map_to_menu_navigation_with_library_menu() {
    assert_eq!(
        button_to_request(Button::Up, STARTUP_DISPLAY_MODE),
        Some(DisplayRequest::MenuPrev)
    );
    assert_eq!(
        button_to_request(Button::Down, STARTUP_DISPLAY_MODE),
        Some(DisplayRequest::MenuNext)
    );
}

#[test]
#[cfg(feature = "sd-storage")]
fn reading_select_and_back_map_to_menuback() {
    assert_eq!(
        button_to_request(Button::Select, MODE_READING),
        Some(DisplayRequest::MenuBack)
    );
    assert_eq!(
        button_to_request(Button::Back, MODE_READING),
        Some(DisplayRequest::MenuBack)
    );
}

#[test]
#[cfg(not(feature = "sd-storage"))]
fn reading_select_and_back_are_noop_without_library_menu() {
    assert_eq!(button_to_request(Button::Select, MODE_READING), None);
    assert_eq!(button_to_request(Button::Back, MODE_READING), None);
}

#[test]
fn reading_power_is_noop() {
    assert_eq!(button_to_request(Button::Power, MODE_READING), None);
}

#[test]
fn unknown_mode_returns_none() {
    assert_eq!(button_to_request(Button::Up, 99), None);
    assert_eq!(button_to_request(Button::Select, 99), None);
}
