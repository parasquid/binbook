use embedded_graphics::{
    geometry::Point, mono_font::MonoTextStyleBuilder, prelude::Drawable, text::Text,
};
use xteink_x4_display::{
    framebuffer::{Gray2Color, Gray2Framebuffer},
    profile::LOGICAL_WIDTH,
};

use crate::library::{Library, LibraryStatus};
use crate::menu::{MenuNames, MenuState};

pub trait MenuEntries {
    fn entry_name(&self, index: usize) -> Option<&str>;
}

impl MenuEntries for MenuNames {
    fn entry_name(&self, index: usize) -> Option<&str> {
        self.get(index).map(|name| name.as_str())
    }
}

impl MenuEntries for Library {
    fn entry_name(&self, index: usize) -> Option<&str> {
        self.entries().get(index).map(|entry| entry.name.as_str())
    }
}

impl MenuEntries for crate::library::LibraryViewport {
    fn entry_name(&self, index: usize) -> Option<&str> {
        let local = index.checked_sub(self.start())?;
        self.entries().get(local).map(|entry| entry.name.as_str())
    }
}

/// Vertical pixel offset for the first menu row.
pub const MENU_ROW_START_Y: i32 = 76;
/// Vertical stride between consecutive menu rows.
pub const MENU_ROW_STRIDE: i32 = 96;
/// Padding from the left edge for entry text.
pub const MENU_TEXT_LEFT: i32 = 16;
/// Horizontal padding from the right edge for page-count suffix.
pub const MENU_TEXT_RIGHT: i32 = 16;
/// White margin between the screen edge and selection dither.
pub const HIGHLIGHT_INSET: u16 = 4;
/// Pixel period of the 25% ordered dither inside the selection highlight.
pub const HIGHLIGHT_DITHER_PERIOD: i32 = 2;

/// Render the current menu viewport into `fb`.
///
/// Clears the framebuffer to white, draws the visible entry names in black
/// text centered in each row, and draws the selected row with a sparse ordered
/// dither without a solid border. The sparse fill reduces the amount of ink
/// moved during navigation and therefore reduces panel ghosting.
pub fn render_menu<N: MenuEntries + ?Sized>(
    fb: &mut Gray2Framebuffer,
    state: &MenuState,
    names: &N,
) {
    fb.clear(Gray2Color::WHITE);

    let visible = state.visible_count();
    for row in 0..visible {
        render_menu_row(fb, state, names, row);
    }
}

fn render_menu_row<N: MenuEntries + ?Sized>(
    fb: &mut Gray2Framebuffer,
    state: &MenuState,
    names: &N,
    row: usize,
) {
    let index = state.top() + row;
    let name = names.entry_name(index).unwrap_or("?");
    let y = MENU_ROW_START_Y + row as i32 * MENU_ROW_STRIDE;
    let selected = index == state.selected();

    if selected {
        let dither_left = i32::from(HIGHLIGHT_INSET);
        let dither_right = i32::from(LOGICAL_WIDTH - HIGHLIGHT_INSET);
        let dither_bottom = y + MENU_ROW_STRIDE - i32::from(HIGHLIGHT_INSET);
        for py in y..dither_bottom {
            for px in dither_left..dither_right {
                if px.rem_euclid(HIGHLIGHT_DITHER_PERIOD) == 0
                    && py.rem_euclid(HIGHLIGHT_DITHER_PERIOD) == 0
                {
                    fb.set_pixel(px as u16, py as u16, Gray2Color::BLACK);
                }
            }
        }
    }

    if !name.is_empty() {
        let style = MonoTextStyleBuilder::new()
            .font(&profont::PROFONT_24_POINT)
            .text_color(Gray2Color::BLACK)
            .build();
        let _ = Text::new(name, Point::new(MENU_TEXT_LEFT, y + 36), style).draw(fb);
    }
}

pub fn render_library_status(fb: &mut Gray2Framebuffer, status: LibraryStatus) {
    fb.clear(Gray2Color::WHITE);
    let (title, hint) = match status {
        LibraryStatus::NoCard => ("NO SD CARD", "INSERT CARD + SELECT"),
        LibraryStatus::CardError => ("SD CARD ERROR", "CHECK CARD + SELECT"),
        LibraryStatus::NoBooks => ("NO BINBOOKS", "ADD TO /BOOKS + SELECT"),
        LibraryStatus::TooManyBooks => ("TOO MANY BOOKS", "LIMIT IS 200"),
        LibraryStatus::UnsupportedName => ("NAME TOO LONG", "MAX 121 BYTES"),
        LibraryStatus::Ready => ("LIBRARY READY", "SELECT A BOOK"),
    };
    let style = MonoTextStyleBuilder::new()
        .font(&profont::PROFONT_24_POINT)
        .text_color(Gray2Color::BLACK)
        .build();
    let _ = Text::new(title, Point::new(24, 300), style).draw(fb);
    let _ = Text::new(hint, Point::new(24, 360), style).draw(fb);
}

pub fn render_book_open_error(fb: &mut Gray2Framebuffer, name: &str) {
    fb.clear(Gray2Color::WHITE);
    let style = MonoTextStyleBuilder::new()
        .font(&profont::PROFONT_24_POINT)
        .text_color(Gray2Color::BLACK)
        .build();
    let _ = Text::new("CANNOT OPEN BOOK", Point::new(24, 280), style).draw(fb);
    let _ = Text::new(name, Point::new(24, 340), style).draw(fb);
    let _ = Text::new("BACK OR SELECT TO RETRY", Point::new(24, 400), style).draw(fb);
}

pub fn render_progress_not_saved(fb: &mut Gray2Framebuffer) {
    render_notice(fb, "PROGRESS NOT SAVED", "WILL RETRY ON CLOSE/SLEEP");
}

pub fn render_sleep_cancelled(fb: &mut Gray2Framebuffer) {
    render_notice(fb, "CANNOT SAVE", "SLEEP CANCELLED");
}

fn render_notice(fb: &mut Gray2Framebuffer, title: &str, hint: &str) {
    fb.clear(Gray2Color::WHITE);
    let style = MonoTextStyleBuilder::new()
        .font(&profont::PROFONT_24_POINT)
        .text_color(Gray2Color::BLACK)
        .build();
    let _ = Text::new(title, Point::new(24, 300), style).draw(fb);
    let _ = Text::new(hint, Point::new(24, 360), style).draw(fb);
}
