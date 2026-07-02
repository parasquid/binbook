//! Menu mode state machine with viewport scrolling and wrap-around.
//!
//! Pure logic: no display code, no hardware. The state tracks scroll position
//! (`top`, `selected`) and list length. Navigation actions are computed from
//! button presses through [`MenuState::transition`].
//!
//! Menu rendering draws into a [`Gray2Framebuffer`] via `embedded-graphics`.

use embedded_graphics::{
    geometry::{Point, Size},
    mono_font::MonoTextStyleBuilder,
    prelude::{Drawable, Primitive},
    primitives::{PrimitiveStyleBuilder, Rectangle},
    text::Text,
};
use xteink_x4_display::{
    framebuffer::{Gray2Color, Gray2Framebuffer},
    profile::LOGICAL_WIDTH,
};

/// Whether the device is showing the library menu or reading a book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Menu,
    Reading,
}

/// Actions the menu state machine produces from a button press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// Move selection down (wrap-around at end).
    Next,
    /// Move selection up (wrap-around at start).
    Previous,
    /// Open the currently selected entry.
    Open(usize),
    /// No-op (back in menu at top level).
    NoOp,
}

/// Maximum number of visible rows in the menu viewport.
pub const VIEWPORT_ROWS: usize = 5;

/// State of the library menu viewport and selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuState {
    /// Index into the full book list of the first visible entry.
    top: usize,
    /// Selected entry index (absolute index into the full list).
    selected: usize,
    /// Total number of book entries.
    count: usize,
}

impl MenuState {
    /// Create a new `MenuState` for a list of `count` entries.
    ///
    /// The selection starts at index 0 if the list is non-empty.
    #[must_use]
    pub const fn new(count: usize) -> Self {
        Self {
            top: 0,
            selected: 0,
            count,
        }
    }

    /// The index of the first visible entry in the viewport.
    #[must_use]
    pub const fn top(&self) -> usize {
        self.top
    }

    /// The absolute index of the currently selected entry.
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }

    /// The total number of entries.
    #[must_use]
    pub const fn count(&self) -> usize {
        self.count
    }

    /// The viewport-relative row of the selected entry (0..VIEWPORT_ROWS).
    #[must_use]
    pub fn selected_row(&self) -> usize {
        self.selected.saturating_sub(self.top)
    }

    /// Whether the entry at `index` is visible in the viewport.
    #[must_use]
    pub fn is_visible(&self, index: usize) -> bool {
        index >= self.top && index < self.top + VIEWPORT_ROWS && index < self.count
    }

    /// Number of entries visible in the viewport (bounded to `count`).
    #[must_use]
    pub fn visible_count(&self) -> usize {
        VIEWPORT_ROWS.min(self.count.saturating_sub(self.top))
    }

    /// Rebuild state for a new list length, resetting to top.
    pub fn reset(&mut self, count: usize) {
        self.top = 0;
        self.selected = 0;
        self.count = count;
    }

    /// Apply a menu action and return the new action.
    ///
    /// Returns `NoOp` when the transition would not change the state (empty list).
    #[must_use]
    pub fn transition(&mut self, action: MenuAction) -> MenuAction {
        if self.count == 0 {
            return MenuAction::NoOp;
        }
        match action {
            MenuAction::Next => {
                // Move down; wrap to top when at the last entry.
                if self.selected + 1 < self.count {
                    self.selected += 1;
                } else {
                    // Wrap: go to index 0, scroll viewport to top
                    self.selected = 0;
                    self.top = 0;
                }
                // Scroll viewport if selection moved past the last visible row.
                if self.selected >= self.top + VIEWPORT_ROWS {
                    self.top = self.selected + 1 - VIEWPORT_ROWS;
                }
                MenuAction::Next
            }
            MenuAction::Previous => {
                // Move up; wrap to bottom when at the first entry.
                if self.selected > 0 {
                    self.selected -= 1;
                } else {
                    // Wrap: go to last entry, scroll viewport to bottom
                    self.selected = self.count - 1;
                    self.top = self.count.saturating_sub(VIEWPORT_ROWS);
                }
                // Scroll viewport if selection moved above the viewport.
                if self.selected < self.top {
                    self.top = self.selected;
                }
                MenuAction::Previous
            }
            MenuAction::Open(_) => {
                // The caller resolves the absolute index.
                MenuAction::Open(self.selected)
            }
            MenuAction::NoOp => MenuAction::NoOp,
        }
    }
}

// ---------------------------------------------------------------------------
// Menu name cache — bounded string storage for library entries
// ---------------------------------------------------------------------------

/// Maximum number of book entries the menu cache can hold.
pub const MAX_MENU_ENTRIES: usize = 200;
/// Maximum length (in bytes) of a single entry name.
pub const MAX_NAME_LEN: usize = 48;

/// A single menu entry name.
pub type MenuName = heapless::String<MAX_NAME_LEN>;
/// A fixed-capacity vector of menu entry names.
pub type MenuNames = heapless::Vec<MenuName, MAX_MENU_ENTRIES>;

// ---------------------------------------------------------------------------
// Menu rendering constants
// ---------------------------------------------------------------------------

/// Vertical pixel offset for the first menu row.
pub const MENU_ROW_START_Y: i32 = 76;
/// Vertical stride between consecutive menu rows.
pub const MENU_ROW_STRIDE: i32 = 52;
/// Padding from the left edge for entry text.
pub const MENU_TEXT_LEFT: i32 = 16;
/// Horizontal padding from the right edge for page-count suffix.
pub const MENU_TEXT_RIGHT: i32 = 16;
/// Thickness of the selection highlight stroke.
pub const HIGHLIGHT_STROKE: u32 = 1;

// ---------------------------------------------------------------------------
// Menu rendering
// ---------------------------------------------------------------------------

/// Render the current menu viewport into `fb`.
///
/// Clears the framebuffer to white, draws the visible entry names in black
/// text centered in each row, and strokes a `HIGHLIGHT_STROKE`-pixel rectangle
/// around the selected row.
pub fn render_menu(fb: &mut Gray2Framebuffer, state: &MenuState, names: &MenuNames) {
    fb.clear(Gray2Color::WHITE);

    let visible = state.visible_count();
    for row in 0..visible {
        let index = state.top() + row;
        let name = names.get(index).map_or_else(|| "?", |n| n.as_str());
        let y = MENU_ROW_START_Y + row as i32 * MENU_ROW_STRIDE;

        if !name.is_empty() {
            let style = MonoTextStyleBuilder::new()
                .font(&profont::PROFONT_12_POINT)
                .text_color(Gray2Color::BLACK)
                .build();
            let _ = Text::new(name, Point::new(MENU_TEXT_LEFT, y + 16), style).draw(fb);
        }

        if index == state.selected() {
            let rect = Rectangle::new(
                Point::new(HIGHLIGHT_STROKE as i32, y),
                Size::new(
                    (LOGICAL_WIDTH as i32 - 2 * HIGHLIGHT_STROKE as i32) as u32,
                    MENU_ROW_STRIDE as u32 - 2 * HIGHLIGHT_STROKE,
                ),
            );
            let _ = rect
                .into_styled(
                    PrimitiveStyleBuilder::new()
                        .stroke_width(HIGHLIGHT_STROKE)
                        .stroke_color(Gray2Color::BLACK)
                        .build(),
                )
                .draw(fb);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_first_entry() {
        let state = MenuState::new(7);
        assert_eq!(state.top(), 0);
        assert_eq!(state.selected(), 0);
        assert_eq!(state.count(), 7);
    }

    #[test]
    fn single_entry_stays_at_zero() {
        let mut state = MenuState::new(1);
        let _ = state.transition(MenuAction::Next);
        assert_eq!(state.selected(), 0);
        assert_eq!(state.top(), 0);
    }

    #[test]
    fn empty_list_always_noop() {
        let mut state = MenuState::new(0);
        assert_eq!(state.transition(MenuAction::Next), MenuAction::NoOp);
        assert_eq!(state.transition(MenuAction::Previous), MenuAction::NoOp);
        assert_eq!(state.transition(MenuAction::Open(0)), MenuAction::NoOp);
    }

    #[test]
    fn from_top_five_downs_scrolls_viewport() {
        // 7-item list at top, 5×down: top scrolls to 1, selected=5
        let mut state = MenuState::new(7);
        for _ in 0..5 {
            let _ = state.transition(MenuAction::Next);
        }
        assert_eq!(state.top(), 1);
        assert_eq!(state.selected(), 5);
        assert_eq!(state.selected_row(), 4);
    }

    #[test]
    fn down_at_last_item_wraps_to_top() {
        let mut state = MenuState::new(7);
        for _ in 0..7 {
            let _ = state.transition(MenuAction::Next);
        }
        // After 7×down from 0: 1→2→3→4→5→6→wrap to 0
        assert_eq!(state.selected(), 0);
        assert_eq!(state.top(), 0);
    }

    #[test]
    fn up_at_top_wraps_to_last() {
        let mut state = MenuState::new(7);
        let _ = state.transition(MenuAction::Previous);
        assert_eq!(state.selected(), 6);
        assert_eq!(state.top(), 2); // 7 - 5 = 2
    }

    #[test]
    fn select_returns_selected_index() {
        let mut state = MenuState::new(7);
        // Navigate to index 3
        for _ in 0..3 {
            let _ = state.transition(MenuAction::Next);
        }
        assert_eq!(state.transition(MenuAction::Open(0)), MenuAction::Open(3));
    }

    #[test]
    fn back_is_noop() {
        let mut state = MenuState::new(7);
        assert_eq!(state.transition(MenuAction::NoOp), MenuAction::NoOp);
        // State unchanged
        assert_eq!(state.selected(), 0);
    }

    #[test]
    fn reset_reinitializes_state() {
        let mut state = MenuState::new(7);
        let _ = state.transition(MenuAction::Next);
        let _ = state.transition(MenuAction::Next);
        assert_eq!(state.selected(), 2);
        state.reset(3);
        assert_eq!(state.selected(), 0);
        assert_eq!(state.top(), 0);
        assert_eq!(state.count(), 3);
    }

    #[test]
    fn viewport_scrolls_up_after_wrapping() {
        let mut state = MenuState::new(10);
        // Go to last item
        let _ = state.transition(MenuAction::Previous);
        assert_eq!(state.selected(), 9);
        assert_eq!(state.top(), 5);

        // Go up once more (now at 8)
        let _ = state.transition(MenuAction::Previous);
        assert_eq!(state.selected(), 8);
        assert_eq!(state.top(), 5);

        // Go up enough to scroll viewport
        let _ = state.transition(MenuAction::Previous);
        let _ = state.transition(MenuAction::Previous);
        let _ = state.transition(MenuAction::Previous); // now at 5
        assert_eq!(state.selected(), 5);
        assert_eq!(state.top(), 5);

        // One more up scrolls viewport
        let _ = state.transition(MenuAction::Previous);
        assert_eq!(state.selected(), 4, "selected should be 4");
        assert_eq!(state.top(), 4, "top should scroll to 4");
    }

    #[test]
    fn is_visible_correct() {
        let mut state = MenuState::new(10);
        assert!(state.is_visible(0));
        assert!(state.is_visible(4));
        assert!(!state.is_visible(5));

        let _ = state.transition(MenuAction::Next); // selected=1
        let _ = state.transition(MenuAction::Next); // selected=2
        let _ = state.transition(MenuAction::Next); // selected=3
        let _ = state.transition(MenuAction::Next); // selected=4, top still 0
        assert!(state.is_visible(4));
        assert!(!state.is_visible(5));
    }

    #[test]
    fn visible_count_bounded() {
        let state = MenuState::new(3);
        assert_eq!(state.visible_count(), 3);
        let state = MenuState::new(10);
        assert_eq!(state.visible_count(), 5);
    }

    // -----------------------------------------------------------------------
    // Menu rendering tests
    // -----------------------------------------------------------------------

    #[test]
    fn render_menu_with_three_entries_selected_middle() {
        let mut fb = Gray2Framebuffer::new();
        let mut state = MenuState::new(3);
        let _ = state.transition(MenuAction::Next);

        let mut names: MenuNames = heapless::Vec::new();
        let mut s1: MenuName = heapless::String::new();
        s1.push_str("Book One").unwrap();
        names.push(s1).unwrap();
        let mut s2: MenuName = heapless::String::new();
        s2.push_str("Book Two").unwrap();
        names.push(s2).unwrap();
        let mut s3: MenuName = heapless::String::new();
        s3.push_str("Book Three").unwrap();
        names.push(s3).unwrap();

        render_menu(&mut fb, &state, &names);

        let highlight_y = MENU_ROW_START_Y + MENU_ROW_STRIDE;
        let highlight_bottom = highlight_y + MENU_ROW_STRIDE - 2 * HIGHLIGHT_STROKE as i32 - 1;

        let highlight_pixels_nonwhite = |y: i32, x_start: i32| -> bool {
            for x in x_start..LOGICAL_WIDTH as i32 {
                let pixel = fb.get_pixel(x as u16, y as u16);
                if pixel.value() < 3 {
                    return true;
                }
            }
            false
        };

        assert!(
            highlight_pixels_nonwhite(highlight_y, HIGHLIGHT_STROKE as i32),
            "highlight top border should have black pixels"
        );
        assert!(
            highlight_pixels_nonwhite(highlight_bottom, HIGHLIGHT_STROKE as i32),
            "highlight bottom border should have black pixels"
        );
        assert!(
            highlight_pixels_nonwhite(MENU_ROW_START_Y + 16, MENU_TEXT_LEFT),
            "row 0 text should have non-white pixels"
        );
        assert!(
            highlight_pixels_nonwhite(MENU_ROW_START_Y + 2 * MENU_ROW_STRIDE + 16, MENU_TEXT_LEFT),
            "row 2 text should have non-white pixels"
        );
        let y_overflow = MENU_ROW_START_Y + 3 * MENU_ROW_STRIDE + 10;
        assert!(
            !highlight_pixels_nonwhite(y_overflow, 0),
            "overflow area should be all white"
        );
    }

    #[test]
    fn render_menu_empty_cache_uses_placeholder() {
        let mut fb = Gray2Framebuffer::new();
        let state = MenuState::new(3);
        let names: MenuNames = heapless::Vec::new();

        render_menu(&mut fb, &state, &names);

        let row_text_has_pixels = |y: i32| -> bool {
            for x in MENU_TEXT_LEFT..MENU_TEXT_LEFT + 50 {
                let pixel = fb.get_pixel(x as u16, y as u16);
                if pixel.value() < 3 {
                    return true;
                }
            }
            false
        };

        assert!(
            row_text_has_pixels(MENU_ROW_START_Y + 16),
            "row 0 should have placeholder text pixels"
        );
    }

    #[test]
    fn render_menu_selected_top_highlights_correct_row() {
        let mut fb = Gray2Framebuffer::new();
        let state = MenuState::new(3);

        let mut names: MenuNames = heapless::Vec::new();
        let mut s1: MenuName = heapless::String::new();
        s1.push_str("Alpha").unwrap();
        names.push(s1).unwrap();
        let mut s2: MenuName = heapless::String::new();
        s2.push_str("Beta").unwrap();
        names.push(s2).unwrap();
        let mut s3: MenuName = heapless::String::new();
        s3.push_str("Gamma").unwrap();
        names.push(s3).unwrap();

        render_menu(&mut fb, &state, &names);

        let highlight_y = MENU_ROW_START_Y;

        let has_black_at_border = |y: i32| -> bool {
            for x in 0..10 {
                let pixel = fb.get_pixel(x as u16, y as u16);
                if pixel.value() < 3 {
                    return true;
                }
            }
            false
        };

        assert!(
            has_black_at_border(highlight_y),
            "highlight top row should have black pixels"
        );
    }
}
